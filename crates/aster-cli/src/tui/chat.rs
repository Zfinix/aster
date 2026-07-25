//! The standalone conversational agent TUI ([`ChatApp`]), driven from
//! `aster chat --tui`. [`run_chat`] owns the render loop; each turn runs as a
//! spawned task so the UI keeps animating while the model works.

use std::sync;
use std::time::Duration;

use anyhow::Result;
use aster_ai::{AiClient, ChatMessage};
use aster_policy::{Mode, PermissionsConfig, Policy};
use ratatui::Frame;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Padding, Paragraph, Wrap};
use tokio::sync::mpsc;

use super::guard::TuiGuard;
use super::helpers::{dim, draw_input_box, human_count, mark_lines, short_path};
use super::{ACCENT, SPINNER};
use crate::chat::{ApprovalRequest, ApprovalSender};

/// A spawned chat turn: the agent's reply plus the files it edited.
type ChatTurn = tokio::task::JoinHandle<Result<(String, Vec<String>)>>;

/// A standalone conversational chat TUI: the full agent (read/search, and edits
/// when allowed), driven from `aster chat --tui`. Each turn runs as a spawned
/// task so the UI keeps animating while the model works.
pub async fn run_chat(
    mut client: AiClient,
    repo_root: std::path::PathBuf,
    allow_edits: bool,
    perms: PermissionsConfig,
    seed: Option<String>,
) -> Result<()> {
    let guard = TuiGuard::install();
    let mut terminal = ratatui::init();
    // Depth 1 is enough: the agent awaits each approval before proposing the next.
    let (approval_tx, mut approval_rx) = mpsc::channel::<ApprovalRequest>(1);
    let endpoint = crate::init::provider_label(client.base_url());
    let cwd = short_path(&repo_root);

    // Prebuild the ask/auto policies so `/edits` switches gating instantly; the
    // config's deny/protected rules are preserved, only the fall-through changes.
    let policy_for = |mode: Mode| {
        let mut c = perms.clone();
        c.mode = mode;
        sync::Arc::new(Policy::compile(&c).unwrap_or_else(|_| Policy::permissive()))
    };
    // Start from the config's stance: --allow-edits enables editing (ask, unless
    // the config already opts into auto); without it, chat is read-only.
    let edit_mode = if !allow_edits {
        EditMode::Off
    } else if perms.mode == Mode::Auto {
        EditMode::Auto
    } else {
        EditMode::Ask
    };
    let mut app = ChatApp::new(
        edit_mode,
        client.model.clone(),
        policy_for(Mode::Ask),
        policy_for(Mode::Auto),
        approval_tx,
        endpoint,
        cwd,
    );
    let mut turn: Option<ChatTurn> = None;

    if let Some(seed) = seed.filter(|s| !s.trim().is_empty()) {
        turn = Some(app.submit(&seed, &client, &repo_root));
    }

    let outcome = loop {
        terminal.draw(|frame| app.draw(frame))?;

        // Surface a pending edit approval (one at a time).
        if app.pending_approval.is_none()
            && let Ok(req) = approval_rx.try_recv()
        {
            app.begin_approval(req);
        }

        if event::poll(Duration::from_millis(80))?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            let ctrl_c = key.modifiers.contains(KeyModifiers::CONTROL)
                && matches!(key.code, KeyCode::Char('c'));

            // While an approval is pending, keys answer it instead of editing input.
            if app.pending_approval.is_some() {
                match key.code {
                    KeyCode::Char('y') | KeyCode::Char('Y') => app.resolve_approval(true),
                    KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                        app.resolve_approval(false)
                    }
                    _ if ctrl_c => {
                        app.resolve_approval(false);
                        if let Some(t) = &turn {
                            t.abort();
                        }
                        break Ok(());
                    }
                    _ => {}
                }
                continue;
            }

            if ctrl_c || matches!(key.code, KeyCode::Esc) {
                if let Some(t) = &turn {
                    t.abort();
                }
                break Ok(());
            }
            let menu_open = !app.command_matches().is_empty();
            match key.code {
                // Arrow keys and Tab drive the slash-command menu when it's open.
                KeyCode::Up if menu_open => app.menu_move(-1),
                KeyCode::Down if menu_open => app.menu_move(1),
                KeyCode::Tab if menu_open => app.complete_command(),
                // With the menu closed, the arrows scroll the conversation.
                KeyCode::Up => app.scroll = app.scroll.saturating_add(1),
                KeyCode::Down => app.scroll = app.scroll.saturating_sub(1),
                KeyCode::PageUp => app.scroll = app.scroll.saturating_add(10),
                KeyCode::PageDown => app.scroll = app.scroll.saturating_sub(10),
                KeyCode::Home => app.scroll = u16::MAX,
                KeyCode::End => app.scroll = 0,
                KeyCode::Enter if turn.is_none() && !app.input.trim().is_empty() => {
                    // A leading slash is a local command (e.g. /model), not a
                    // message to the model.
                    if app.input.trim_start().starts_with('/') {
                        let cmd = app.command_to_run();
                        app.input.clear();
                        app.menu_sel = 0;
                        app.handle_command(&cmd, &mut client);
                        if app.should_quit {
                            break Ok(());
                        }
                    } else {
                        let text = std::mem::take(&mut app.input);
                        turn = Some(app.submit(&text, &client, &repo_root));
                        app.scroll = 0;
                    }
                }
                KeyCode::Backspace => {
                    app.input.pop();
                    app.menu_sel = 0;
                    app.flash = None;
                }
                KeyCode::Char(c) => {
                    app.input.push(c);
                    app.menu_sel = 0;
                    app.flash = None;
                }
                _ => {}
            }
        }

        app.usage = Some(client.usage_snapshot());
        if app.thinking {
            app.spinner = (app.spinner + 1) % SPINNER.len();
        }

        if turn.as_ref().is_some_and(|t| t.is_finished()) {
            match turn.take().expect("checked is_some").await {
                Ok(Ok((reply, edited))) => app.push_reply(&reply, &edited),
                Ok(Err(e)) => app.fail_turn(&format!("{e:#}")),
                Err(e) => app.fail_turn(&format!("chat failed: {e}")),
            }
            app.thinking = false;
        }
    };

    drop(guard);
    outcome
}

/// How the agent's file edits are gated in chat, cycled with `/edits`. Maps onto
/// the permission [`Mode`]: Ask/Auto also decide whether each write prompts.
#[derive(Clone, Copy, PartialEq, Eq)]
enum EditMode {
    /// No edit tool at all: read and search only.
    Off,
    /// Edits are offered but every write asks for confirmation first.
    Ask,
    /// Edits apply without asking (protected paths are still blocked).
    Auto,
}

impl EditMode {
    fn short(self) -> &'static str {
        match self {
            EditMode::Off => "off",
            EditMode::Ask => "ask",
            EditMode::Auto => "auto",
        }
    }

    fn desc(self) -> &'static str {
        match self {
            EditMode::Off => "off · read & search only",
            EditMode::Ask => "ask · confirm each edit",
            EditMode::Auto => "auto · edit without asking",
        }
    }

    /// Cycle Off → Ask → Auto → Off.
    fn next(self) -> Self {
        match self {
            EditMode::Off => EditMode::Ask,
            EditMode::Ask => EditMode::Auto,
            EditMode::Auto => EditMode::Off,
        }
    }
}

/// A slash command surfaced in the chat menu. `takes_arg` decides whether Tab
/// completes to `/name ` (ready for an argument) or `/name`.
struct ChatCommand {
    name: &'static str,
    takes_arg: bool,
    desc: &'static str,
}

const CHAT_COMMANDS: &[ChatCommand] = &[
    ChatCommand {
        name: "model",
        takes_arg: true,
        desc: "Switch the active model, or show it with no argument",
    },
    ChatCommand {
        name: "edits",
        takes_arg: false,
        desc: "Cycle edit gating: off → ask → auto",
    },
    ChatCommand {
        name: "clear",
        takes_arg: false,
        desc: "Clear the conversation and start fresh",
    },
    ChatCommand {
        name: "help",
        takes_arg: false,
        desc: "List the available commands",
    },
    ChatCommand {
        name: "quit",
        takes_arg: false,
        desc: "Exit the chat",
    },
];

struct ChatApp {
    lines: Vec<Line<'static>>,
    input: String,
    thinking: bool,
    spinner: usize,
    usage: Option<aster_ai::UsageSnapshot>,
    /// How edits are gated (off/ask/auto), cycled with `/edits`.
    edit_mode: EditMode,
    /// The active model id, shown in the header and changed with `/model`.
    model: String,
    /// Full conversation (user/assistant turns), carried forward each turn.
    history: Vec<ChatMessage>,
    /// Prebuilt policies for ask/auto, so `/edits` switches gating with no rebuild.
    ask_policy: sync::Arc<Policy>,
    auto_policy: sync::Arc<Policy>,
    /// Handed to each turn so `ask` mode can request confirmation.
    approval_tx: ApprovalSender,
    /// An edit awaiting the user's y/n answer, if any.
    pending_approval: Option<ApprovalRequest>,
    /// Highlighted row in the slash-command menu (when it is showing).
    menu_sel: usize,
    /// Set by `/quit`, read by the event loop to break out.
    should_quit: bool,
    /// Provider name (e.g. `OpenRouter`), shown on the welcome panel.
    endpoint: String,
    /// Short working directory, shown on the welcome panel.
    cwd: String,
    /// A transient one-line status in the footer (e.g. after `/edits`), cleared
    /// on the next keystroke so it never disturbs the conversation view.
    flash: Option<String>,
    /// How many lines the view is scrolled up from the bottom. `0` follows the
    /// latest output; higher pins the reader above it. Clamped to the scrollable
    /// range each frame in `draw`, and left untouched as new output streams in so
    /// reading history is never yanked back to the bottom.
    scroll: u16,
}

impl ChatApp {
    fn new(
        edit_mode: EditMode,
        model: String,
        ask_policy: sync::Arc<Policy>,
        auto_policy: sync::Arc<Policy>,
        approval_tx: ApprovalSender,
        endpoint: String,
        cwd: String,
    ) -> Self {
        Self {
            lines: Vec::new(),
            input: String::new(),
            thinking: false,
            spinner: 0,
            usage: None,
            edit_mode,
            model,
            history: Vec::new(),
            ask_policy,
            auto_policy,
            approval_tx,
            pending_approval: None,
            menu_sel: 0,
            should_quit: false,
            endpoint,
            cwd,
            flash: None,
            scroll: 0,
        }
    }

    /// True when the agent should be offered the edit tool this turn.
    fn edits_enabled(&self) -> bool {
        self.edit_mode != EditMode::Off
    }

    /// The policy governing this turn's edits: `auto` applies writes directly,
    /// everything else prompts through the approval channel.
    fn turn_policy(&self) -> sync::Arc<Policy> {
        match self.edit_mode {
            EditMode::Auto => self.auto_policy.clone(),
            _ => self.ask_policy.clone(),
        }
    }

    /// The slash commands matching the current input, or empty when the menu
    /// should not show (no leading `/`, or an argument is being typed).
    fn command_matches(&self) -> Vec<&'static ChatCommand> {
        let Some(rest) = self.input.strip_prefix('/') else {
            return Vec::new();
        };
        // Once a space is typed the user is entering an argument, so hide the menu.
        if rest.contains(char::is_whitespace) {
            return Vec::new();
        }
        CHAT_COMMANDS
            .iter()
            .filter(|c| c.name.starts_with(rest))
            .collect()
    }

    /// The highlighted command in the menu, clamped to the current matches.
    fn selected_command(&self) -> Option<&'static ChatCommand> {
        let matches = self.command_matches();
        matches
            .get(self.menu_sel)
            .or_else(|| matches.first())
            .copied()
    }

    fn menu_move(&mut self, delta: isize) {
        let len = self.command_matches().len();
        if len == 0 {
            return;
        }
        let cur = self.menu_sel.min(len - 1) as isize;
        self.menu_sel = (cur + delta).rem_euclid(len as isize) as usize;
    }

    /// Complete the input to the highlighted command (Tab), adding a trailing
    /// space when the command takes an argument.
    fn complete_command(&mut self) {
        if let Some(cmd) = self.selected_command() {
            self.input = format!("/{}{}", cmd.name, if cmd.takes_arg { " " } else { "" });
        }
    }

    /// The command line to execute on Enter: an explicitly typed command with
    /// args runs as-is; a bare prefix runs the highlighted menu entry.
    fn command_to_run(&self) -> String {
        let rest = self.input.trim_start_matches('/').trim().to_string();
        if rest.contains(char::is_whitespace) {
            return rest;
        }
        self.selected_command()
            .map(|c| c.name.to_string())
            .unwrap_or(rest)
    }

    /// Show a pending edit and prompt for confirmation.
    fn begin_approval(&mut self, req: ApprovalRequest) {
        for line in req.preview.lines() {
            self.push_system(line);
        }
        self.push_system("apply this edit? [y/n]");
        self.pending_approval = Some(req);
    }

    /// Answer the pending approval, replying to the waiting turn.
    fn resolve_approval(&mut self, approved: bool) {
        if let Some(req) = self.pending_approval.take() {
            let _ = req.respond.send(approved);
            self.push_system(if approved {
                "edit approved"
            } else {
                "edit rejected"
            });
        }
    }

    /// Handle a `/`-prefixed input line. The model change takes effect on the
    /// next turn, since each turn clones the client.
    fn handle_command(&mut self, cmd: &str, client: &mut AiClient) {
        let mut parts = cmd.splitn(2, char::is_whitespace);
        let name = parts.next().unwrap_or("");
        let arg = parts.next().map(str::trim).filter(|s| !s.is_empty());
        match name {
            "model" | "m" => match arg {
                Some(model) => {
                    client.model = model.to_string();
                    self.model = model.to_string();
                    self.push_system(&format!("model set to {model}"));
                }
                None => self.push_system(&format!("current model: {}", self.model)),
            },
            "edits" | "e" => {
                self.edit_mode = self.edit_mode.next();
                // A footer flash, not a scrollback line, so the view doesn't jump.
                self.flash = Some(format!("edits {}", self.edit_mode.desc()));
            }
            "clear" | "c" => {
                self.lines.clear();
                self.history.clear();
                self.push_system("conversation cleared");
            }
            "help" | "h" => {
                self.push_system("commands:");
                for c in CHAT_COMMANDS {
                    self.push_system(&format!("  /{:<7} {}", c.name, c.desc));
                }
            }
            "quit" | "q" | "exit" => self.should_quit = true,
            other => self.push_system(&format!("unknown command: /{other} (try /help)")),
        }
    }

    fn push_system(&mut self, msg: &str) {
        self.lines.push(Line::from(Span::styled(
            format!("  · {msg}"),
            Style::default().fg(ACCENT),
        )));
    }

    /// Record the question, then spawn the agent turn over the whole history.
    fn submit(&mut self, text: &str, client: &AiClient, repo_root: &std::path::Path) -> ChatTurn {
        self.push_user(text);
        self.history.push(ChatMessage {
            role: "user".into(),
            content: text.into(),
        });
        self.thinking = true;
        let client = client.clone();
        let repo_root = repo_root.to_path_buf();
        let history = self.history.clone();
        let allow_edits = self.edits_enabled();
        let policy = self.turn_policy();
        let approver = Some(self.approval_tx.clone());
        tokio::spawn(async move {
            crate::chat::agent_turn(client, repo_root, history, allow_edits, policy, approver).await
        })
    }

    fn push_user(&mut self, text: &str) {
        self.lines.push(Line::from(""));
        self.lines.push(Line::from(vec![
            Span::styled("❯ ", Style::default().fg(ACCENT)),
            Span::styled(
                text.to_string(),
                Style::default().add_modifier(Modifier::BOLD),
            ),
        ]));
    }

    fn push_reply(&mut self, reply: &str, edited: &[String]) {
        self.history.push(ChatMessage {
            role: "assistant".into(),
            content: reply.into(),
        });
        self.lines.push(Line::from(""));
        for (i, l) in reply.lines().enumerate() {
            if i == 0 {
                self.lines.push(Line::from(vec![
                    Span::styled("✳ ", Style::default().fg(Color::Green)),
                    Span::raw(l.to_string()),
                ]));
            } else {
                self.lines.push(Line::from(Span::raw(l.to_string())));
            }
        }
        for path in edited {
            self.lines.push(Line::from(Span::styled(
                format!("  ✎ edited {path}"),
                Style::default().fg(ACCENT),
            )));
        }
    }

    fn push_error(&mut self, msg: &str) {
        self.lines.push(Line::from(Span::styled(
            format!("  ! {msg}"),
            Style::default().fg(Color::Red),
        )));
    }

    /// Show the error and drop the unanswered question from history, so a retry
    /// resends just that question instead of stacking a duplicate user turn.
    fn fail_turn(&mut self, msg: &str) {
        if self.history.last().is_some_and(|m| m.role == "user") {
            self.history.pop();
        }
        self.push_error(msg);
    }

    fn draw(&mut self, frame: &mut Frame) {
        let area = frame.area();

        // No banner or header row: the welcome panel already carries the identity,
        // so the conversation sits right at the top with minimal chrome.
        let rows = Layout::vertical([
            Constraint::Min(0),
            Constraint::Length(3),
            Constraint::Length(1),
        ])
        .split(area);
        let body = rows[0];
        let input = rows[1];
        let footer = rows[2];

        // Body: the conversation, or the welcome panel, both aligned to the top-left.
        // No border around the output; the messages just flow, with a single space
        // of left padding to breathe.
        let visible = body.height as usize;
        let content: Vec<Line> = if self.lines.is_empty() {
            self.welcome_lines()
        } else {
            self.lines.clone()
        };
        let max_scroll = content.len().saturating_sub(visible) as u16;
        // Clamp so scrollback can't overshoot the content, and so it re-pins to
        // the bottom as the range shrinks (e.g. after `/clear`).
        self.scroll = self.scroll.min(max_scroll);
        let scroll = max_scroll - self.scroll;
        frame.render_widget(
            Paragraph::new(content)
                .block(Block::default().padding(Padding::horizontal(1)))
                .wrap(Wrap { trim: false })
                .scroll((scroll, 0)),
            body,
        );

        draw_input_box(
            frame,
            input,
            &self.input,
            self.thinking,
            self.spinner,
            "Message Aster…",
        );

        // Slash-command menu floats just above the input, drawn last so it wins.
        if !self.thinking && self.pending_approval.is_none() {
            self.draw_command_menu(frame, input);
        }

        // Footer: usage + hints.
        let usage = self
            .usage
            .filter(|u| u.total_tokens > 0)
            .map(|u| {
                let approx = if u.estimated { "~" } else { "" };
                let cost = u
                    .estimated_cost_usd
                    .map(|c| format!("  ·  ~${c:.4}"))
                    .unwrap_or_default();
                format!(
                    "  ctx {approx}{} in / {approx}{} out{cost}",
                    human_count(u.prompt_tokens as usize),
                    human_count(u.completion_tokens as usize),
                )
            })
            .unwrap_or_default();
        let hint = if self.thinking {
            "esc to quit"
        } else {
            "enter to send  ·  esc to quit"
        };
        let dark = Style::default().fg(Color::DarkGray);
        let edit_color = match self.edit_mode {
            EditMode::Auto => Color::Green,
            EditMode::Ask => Color::Yellow,
            EditMode::Off => Color::DarkGray,
        };
        let mut spans = vec![Span::styled(
            format!("  ✎ edits {}", self.edit_mode.short()),
            Style::default().fg(edit_color),
        )];
        if !usage.is_empty() {
            spans.push(Span::styled(format!("  ·{usage}"), dark));
        }
        spans.push(Span::styled("  ·  ", dark));
        // A transient status (e.g. after /edits) replaces the hint until the next key.
        match &self.flash {
            Some(msg) => spans.push(Span::styled(msg.clone(), Style::default().fg(ACCENT))),
            None => spans.push(Span::styled(hint.to_string(), dark)),
        }
        frame.render_widget(Paragraph::new(Line::from(spans)), footer);
    }

    /// The empty-state welcome panel: the wordmark, session context, and tips.
    fn welcome_lines(&self) -> Vec<Line<'static>> {
        let field = |k: &str, v: String| {
            Line::from(vec![
                Span::styled(format!("  {k:<9}"), Style::default().fg(Color::DarkGray)),
                Span::raw(v),
            ])
        };

        // The asterisk mark + wordmark, matching the desktop app's logo. Each mark
        // row is indented two spaces to line up with the session fields below it.
        let mut lines: Vec<Line<'static>> = mark_lines()
            .into_iter()
            .map(|line| {
                let mut spans = vec![Span::raw("  ")];
                spans.extend(line.spans);
                Line::from(spans)
            })
            .collect();
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled(
                "  Aster",
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  ·  AI code review  ·  v{}", env!("CARGO_PKG_VERSION")),
                Style::default().fg(Color::DarkGray),
            ),
        ]));
        lines.push(Line::from(""));
        lines.push(field("model", self.model.clone()));
        lines.push(field("provider", self.endpoint.clone()));
        lines.push(field("cwd", self.cwd.clone()));
        lines.push(field("mode", self.edit_mode.desc().into()));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  Getting started",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        )));
        lines.push(dim(
            "  • Ask about a file, a diff, or anything in this repo",
        ));
        lines.push(dim(
            "  • Type  /  to browse commands (model, edits, clear…)",
        ));
        lines
    }

    /// Render the filtered slash-command menu as a popup anchored just above the
    /// input box. Does nothing when no command matches the current input.
    fn draw_command_menu(&self, frame: &mut Frame, input: Rect) {
        let matches = self.command_matches();
        if matches.is_empty() {
            return;
        }
        let sel = self.menu_sel.min(matches.len() - 1);

        let rows: Vec<Line> = matches
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let active = i == sel;
                let name_style = if active {
                    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::Gray)
                };
                Line::from(vec![
                    Span::styled(if active { " › " } else { "   " }, name_style),
                    Span::styled(format!("/{:<7}", c.name), name_style),
                    Span::styled(
                        format!("  {}", c.desc),
                        Style::default().fg(Color::DarkGray),
                    ),
                ])
            })
            .collect();

        let height = matches.len() as u16 + 2;
        let width = input.width;
        let y = input.y.saturating_sub(height);
        let popup = Rect {
            x: input.x,
            y,
            width,
            height,
        };

        frame.render_widget(Clear, popup);
        frame.render_widget(
            Paragraph::new(rows).block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(ACCENT))
                    .title(" commands ")
                    .title_style(Style::default().fg(Color::DarkGray))
                    .padding(Padding::horizontal(1)),
            ),
            popup,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chat_app(model: String) -> ChatApp {
        let (tx, _rx) = mpsc::channel(1);
        ChatApp::new(
            EditMode::Off,
            model,
            sync::Arc::new(Policy::permissive()),
            sync::Arc::new(Policy::permissive()),
            tx,
            "OpenRouter".into(),
            "~/repo".into(),
        )
    }

    #[test]
    fn chat_command_model_switches_client_and_app() {
        let mut client = AiClient::new("http://localhost", "k", "openai/gpt-4o-mini");
        let mut app = chat_app(client.model.clone());
        app.handle_command("model anthropic/claude-sonnet-5", &mut client);
        assert_eq!(client.model, "anthropic/claude-sonnet-5");
        assert_eq!(app.model, "anthropic/claude-sonnet-5");
    }

    #[test]
    fn chat_command_model_without_arg_keeps_model() {
        let mut client = AiClient::new("http://localhost", "k", "m1");
        let mut app = chat_app(client.model.clone());
        app.handle_command("model", &mut client);
        assert_eq!(client.model, "m1");
        assert_eq!(app.model, "m1");
    }

    #[test]
    fn chat_command_unknown_is_reported() {
        let mut client = AiClient::new("http://localhost", "k", "m1");
        let mut app = chat_app(client.model.clone());
        let before = app.lines.len();
        app.handle_command("bogus", &mut client);
        assert!(app.lines.len() > before);
    }
}
