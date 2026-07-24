//! Live review TUI. Runs the whole review in the background and renders each
//! step as it happens — indexing, hypotheses, verification, findings landing —
//! so users watch it work instead of staring at a blank prompt.

use std::time::{Duration, Instant};
use std::{panic, sync};

use anyhow::Result;
use aster_ai::{AiClient, ChatMessage};
use aster_harness::Progress;
use aster_models::ReviewReport;
use ratatui::Frame;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Padding, Paragraph, Wrap};

use crate::review::{Job, execute};

const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const ACCENT: Color = Color::Magenta;

/// The review-agent persona, shared with `aster chat` and the desktop app.
const AGENT_PROMPT: &str = include_str!("../prompts/aster-agent.md");
const CHAT_TEMPERATURE: f32 = 0.4;

/// A spawned chat turn: the agent's reply plus the files it edited.
type ChatTurn = tokio::task::JoinHandle<Result<(String, Vec<String>)>>;

/// Findings from the just-finished review, formatted as ground truth the chat
/// agent can answer follow-ups from. Mirrors the desktop's review context.
fn review_context(report: &ReviewReport, min_confidence: f32) -> String {
    let findings: Vec<_> = report
        .findings
        .iter()
        .filter(|f| f.confidence.unwrap_or(1.0) >= min_confidence)
        .collect();
    if findings.is_empty() {
        return "I ran a code review and found no issues; the diff is clean.".to_string();
    }
    let lines: Vec<String> = findings
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let conf = f
                .confidence
                .map(|c| format!(" [confidence {c:.2}]"))
                .unwrap_or_default();
            format!(
                "{}. {} — {} ({}:{}){conf}: {} Fix: {}",
                i + 1,
                f.severity.to_uppercase(),
                f.title,
                f.file_path,
                f.line,
                f.description,
                f.suggestion,
            )
        })
        .collect();
    format!(
        "Here are the findings from the code review I ran ({} total):\n{}\n{}",
        findings.len(),
        report.summary,
        lines.join("\n"),
    )
}

/// Filled block glyphs for A S T E R, 5 rows each, assembled at draw time.
const LETTERS: [[&str; 5]; 5] = [
    [" █████ ", "██   ██", "███████", "██   ██", "██   ██"],
    ["███████", "██     ", "███████", "     ██", "███████"],
    ["███████", "   ██  ", "   ██  ", "   ██  ", "   ██  "],
    ["███████", "██     ", "██████ ", "██     ", "███████"],
    ["██████ ", "██   ██", "██████ ", "██   ██", "██   ██"],
];

/// Sunset gradient, top row (pink) to bottom (gold).
const SUNSET: [Color; 5] = [
    Color::Rgb(255, 94, 120),
    Color::Rgb(255, 122, 92),
    Color::Rgb(255, 154, 71),
    Color::Rgb(255, 186, 63),
    Color::Rgb(255, 214, 92),
];

pub async fn run(job: Job, min_confidence: f32) -> Result<()> {
    let (tx, rx) = sync::mpsc::channel::<Progress>();
    // Usage counters are Arc-shared, so this clone reflects live token spend as
    // the moved-in client works.
    let usage_handle = job.ai_client.clone();
    let mut task = tokio::spawn(async move { execute(job, &Some(tx)).await });

    // Guarantees the terminal leaves raw/alt-screen mode on every exit path:
    // normal return, an early `?` IO error in the render loop, or a panic.
    let guard = TuiGuard::install();

    let mut terminal = ratatui::init();
    let mut app = App::new(min_confidence);
    // Kept so results can be reprinted to the real terminal after the TUI's
    // alternate screen is torn down — otherwise quitting erases everything.
    let mut response: Option<ReviewReport> = None;
    // The chat client shares usage counters with the review client, so its
    // spend rolls into the same footer meter.
    let chat_client = usage_handle.clone();
    let mut chat_task: Option<tokio::task::JoinHandle<Result<String>>> = None;

    let outcome = loop {
        terminal.draw(|frame| app.draw(frame))?;

        // Non-blocking so the feed keeps updating between keystrokes.
        if event::poll(Duration::from_millis(80))?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            // Ctrl+C always exits, mid-review or mid-chat.
            let ctrl_c = key.modifiers.contains(KeyModifiers::CONTROL)
                && matches!(key.code, KeyCode::Char('c'));
            if ctrl_c {
                if !app.finished {
                    task.abort();
                }
                if let Some(t) = &chat_task {
                    t.abort();
                }
                break Ok(());
            }

            if !app.finished {
                // During the review the log is read-only; q or Esc cancels.
                if matches!(key.code, KeyCode::Char('q') | KeyCode::Esc) {
                    task.abort();
                    break Ok(());
                }
            } else {
                // After the review the footer becomes a chat input.
                match key.code {
                    KeyCode::Esc => break Ok(()),
                    KeyCode::Enter if !app.chatting && !app.input.trim().is_empty() => {
                        let text = std::mem::take(&mut app.input);
                        app.push_user(&text);
                        let msgs = app.build_chat(&text);
                        let client = chat_client.clone();
                        app.chatting = true;
                        app.status = "thinking".into();
                        chat_task = Some(tokio::spawn(async move {
                            client.complete_messages(&msgs, CHAT_TEMPERATURE).await
                        }));
                    }
                    KeyCode::Backspace => {
                        app.input.pop();
                    }
                    KeyCode::Char(c) => app.input.push(c),
                    _ => {}
                }
            }
        }

        while let Ok(event) = rx.try_recv() {
            app.apply(event);
        }
        app.set_usage(usage_handle.usage_snapshot());
        app.tick();

        if !app.finished && task.is_finished() {
            match (&mut task).await {
                Ok(Ok(resp)) => {
                    app.set_report_context(&resp);
                    response = Some(resp);
                    app.mark_done();
                }
                Ok(Err(e)) => break Err(e),
                Err(e) if e.is_cancelled() => break Ok(()),
                Err(e) => break Err(anyhow::anyhow!(e)),
            }
        }

        if chat_task.as_ref().is_some_and(|t| t.is_finished()) {
            match chat_task.take().expect("checked is_some").await {
                Ok(Ok(reply)) => app.push_reply(&reply),
                Ok(Err(e)) => app.push_error(&format!("{e:#}")),
                Err(e) => app.push_error(&format!("chat failed: {e}")),
            }
            app.chatting = false;
            app.status = "ready".into();
        }
    };

    // Restore before printing so the summary lands on the normal screen, not
    // the alternate screen that is about to be torn down.
    drop(guard);
    if let Some(resp) = &response {
        print_summary(resp, min_confidence);
    }
    outcome
}

/// Restores the terminal and panic hook on drop, so no code path (early `?`,
/// panic, or normal exit) can leave the shell in raw/alt-screen mode.
struct TuiGuard;

impl TuiGuard {
    fn install() -> Self {
        let original = panic::take_hook();
        panic::set_hook(Box::new(move |info| {
            ratatui::restore();
            original(info);
        }));
        TuiGuard
    }
}

impl Drop for TuiGuard {
    fn drop(&mut self) {
        ratatui::restore();
        // Drop our hook so a later, unrelated panic does not emit stray restore
        // escape sequences to an already-restored terminal.
        let _ = panic::take_hook();
    }
}

struct App {
    lines: Vec<Line<'static>>,
    status: String,
    spinner: usize,
    finished: bool,
    found: usize,
    /// Chars streamed from the model in the current phase — shown live so a long
    /// call reads as working, not hung.
    stream_chars: usize,
    /// Total chars streamed across the whole review, used for a live token
    /// counter that climbs continuously (like a streaming token meter) rather
    /// than jumping only when a request's real usage lands.
    total_stream_chars: usize,
    started: Instant,
    /// Frozen wall-clock at completion so the header timer stops counting once
    /// the review is done.
    elapsed: Option<Duration>,
    /// Token spend so far, polled from the shared client each frame.
    usage: Option<aster_ai::UsageSnapshot>,
    min_confidence: f32,
    /// Chat: current input line, in-flight flag, conversation history, and the
    /// review context the agent answers from. Enabled once the review finishes.
    input: String,
    chatting: bool,
    chat_msgs: Vec<ChatMessage>,
    ctx: Option<String>,
}

impl App {
    fn new(min_confidence: f32) -> Self {
        Self {
            lines: Vec::new(),
            status: "starting".into(),
            spinner: 0,
            finished: false,
            found: 0,
            stream_chars: 0,
            total_stream_chars: 0,
            started: Instant::now(),
            elapsed: None,
            usage: None,
            min_confidence,
            input: String::new(),
            chatting: false,
            chat_msgs: Vec::new(),
            ctx: None,
        }
    }

    fn set_usage(&mut self, usage: aster_ai::UsageSnapshot) {
        self.usage = Some(usage);
    }

    fn tick(&mut self) {
        if !self.finished || self.chatting {
            self.spinner = (self.spinner + 1) % SPINNER.len();
        }
    }

    /// Capture the review's findings as chat context once it completes.
    fn set_report_context(&mut self, report: &ReviewReport) {
        self.ctx = Some(review_context(report, self.min_confidence));
    }

    /// The messages for one chat turn: persona, review context, prior turns,
    /// then the new question. The user message is recorded so the next turn
    /// carries it forward.
    fn build_chat(&mut self, text: &str) -> Vec<ChatMessage> {
        let mut msgs = vec![ChatMessage {
            role: "system".into(),
            content: AGENT_PROMPT.into(),
        }];
        if let Some(ctx) = &self.ctx {
            msgs.push(ChatMessage {
                role: "assistant".into(),
                content: ctx.clone(),
            });
        }
        self.chat_msgs.push(ChatMessage {
            role: "user".into(),
            content: text.into(),
        });
        msgs.extend(self.chat_msgs.clone());
        msgs
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

    fn push_reply(&mut self, reply: &str) {
        self.chat_msgs.push(ChatMessage {
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
    }

    fn push_error(&mut self, msg: &str) {
        self.lines.push(Line::from(Span::styled(
            format!("  ! {msg}"),
            Style::default().fg(Color::Red),
        )));
    }

    /// Elapsed time, frozen once the review finishes.
    fn elapsed(&self) -> Duration {
        self.elapsed.unwrap_or_else(|| self.started.elapsed())
    }

    fn mark_done(&mut self) {
        self.finished = true;
        self.elapsed.get_or_insert_with(|| self.started.elapsed());
    }

    fn apply(&mut self, event: Progress) {
        match event {
            Progress::Phase(name) => {
                self.status = name.to_lowercase();
                self.stream_chars = 0;
                self.lines.push(Line::from(""));
                self.lines.push(Line::from(vec![
                    Span::styled("▶ ", Style::default().fg(ACCENT)),
                    Span::styled(
                        name,
                        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                    ),
                ]));
            }
            Progress::Token { delta, .. } => {
                // Don't dump raw tokens into the step log, but count them so the
                // header shows live motion during a long model call.
                let n = delta.chars().count();
                self.stream_chars += n;
                self.total_stream_chars += n;
            }
            Progress::Hypothesized { count } => {
                self.lines
                    .push(dim(format!("  {count} candidate(s) to verify")));
            }
            Progress::Verifying {
                index,
                total,
                title,
            } => {
                self.status = format!("verifying {index}/{total}");
                self.lines.push(Line::from(vec![
                    Span::styled(
                        format!("  → [{index}/{total}] "),
                        Style::default().fg(Color::DarkGray),
                    ),
                    Span::raw(title),
                ]));
            }
            Progress::Confirmed(f) => {
                if f.confidence.unwrap_or(1.0) < self.min_confidence {
                    return;
                }
                self.found += 1;
                let conf = f
                    .confidence
                    .map(|c| format!("  ·  {:.0}%", c * 100.0))
                    .unwrap_or_default();
                self.lines.push(Line::from(vec![
                    Span::styled(
                        "  ✓ ",
                        Style::default()
                            .fg(Color::Green)
                            .add_modifier(Modifier::BOLD),
                    ),
                    severity_chip(&f.severity),
                    Span::styled(
                        format!("  {}", f.title),
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                ]));
                self.lines.push(Line::from(vec![
                    Span::raw("      "),
                    Span::styled(
                        format!("{}:{}{}", f.file_path, f.line, conf),
                        Style::default().fg(Color::Cyan),
                    ),
                ]));
                self.lines.push(dim(format!("      {}", f.description)));
            }
            Progress::Refuted { title, .. } => {
                self.lines.push(Line::from(vec![
                    Span::styled("  ✗ ", Style::default().fg(Color::Red)),
                    Span::styled(
                        format!("refuted  {title}"),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]));
            }
            Progress::Done { total, .. } => {
                self.found = total;
                self.status = "done".into();
                self.mark_done();
            }
        }
    }

    fn draw(&self, frame: &mut Frame) {
        let area = frame.area();
        // Show the wordmark only when there's room; fall back to a compact header.
        let banner = area.width >= 42 && area.height >= 16;
        // The chat input appears once the review finishes.
        let show_input = self.finished;

        let mut constraints = Vec::new();
        if banner {
            constraints.push(Constraint::Length(1));
            constraints.push(Constraint::Length(5));
        }
        constraints.push(Constraint::Length(1));
        constraints.push(Constraint::Min(0));
        if show_input {
            constraints.push(Constraint::Length(3));
        }
        constraints.push(Constraint::Length(1));
        let rows = Layout::vertical(constraints).split(area);

        let mut i = 0;
        if banner {
            draw_banner(frame, rows[1]);
            i = 2;
        }
        let status = rows[i];
        i += 1;
        let body = rows[i];
        i += 1;
        let input = if show_input {
            let a = rows[i];
            i += 1;
            Some(a)
        } else {
            None
        };
        let footer = rows[i];

        self.draw_header(frame, status, !banner);

        let visible = body.height.saturating_sub(2) as usize;
        let scroll = self.lines.len().saturating_sub(visible) as u16;
        frame.render_widget(
            Paragraph::new(self.lines.clone())
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_type(BorderType::Rounded)
                        .border_style(Style::default().fg(Color::DarkGray))
                        .padding(Padding::horizontal(1)),
                )
                .wrap(Wrap { trim: false })
                .scroll((scroll, 0)),
            body,
        );

        if let Some(input) = input {
            self.draw_input(frame, input);
        }
        self.draw_footer(frame, footer);
    }

    /// The chat input box: a rounded field that shows a placeholder, the typed
    /// text with a caret, or a "thinking" spinner while a reply is in flight.
    fn draw_input(&self, frame: &mut Frame, area: Rect) {
        draw_input_box(
            frame,
            area,
            &self.input,
            self.chatting,
            self.spinner,
            "Ask Aster about these findings…",
        );
    }

    fn draw_header(&self, frame: &mut Frame, area: Rect, show_name: bool) {
        let [left, right] =
            Layout::horizontal([Constraint::Min(0), Constraint::Length(12)]).areas(area);

        let mark = if self.finished {
            Span::styled("✳ ", Style::default().fg(Color::Green))
        } else {
            Span::styled(
                format!("{} ", SPINNER[self.spinner]),
                Style::default().fg(ACCENT),
            )
        };
        let mut spans = vec![mark];
        if show_name {
            spans.push(Span::styled(
                "Aster",
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ));
            spans.push(Span::styled(
                format!("  {}", self.status),
                Style::default().fg(Color::DarkGray),
            ));
        } else {
            spans.push(Span::styled(
                self.status.clone(),
                Style::default().fg(Color::DarkGray),
            ));
        }
        // Live token meter that climbs continuously as content streams, so a
        // long model call visibly progresses. ~4 chars per token.
        if !self.finished && self.total_stream_chars > 0 {
            spans.push(Span::styled(
                format!("  ▸ {} tokens", human_count(self.total_stream_chars / 4)),
                Style::default().fg(ACCENT),
            ));
        }
        frame.render_widget(Paragraph::new(Line::from(spans)), left);

        let elapsed = format!("{:.1}s ", self.elapsed().as_secs_f64());
        frame.render_widget(
            Paragraph::new(Line::from(elapsed))
                .alignment(Alignment::Right)
                .style(Style::default().fg(Color::DarkGray)),
            right,
        );
    }

    fn draw_footer(&self, frame: &mut Frame, area: Rect) {
        let lead = if self.finished {
            let label = if self.found == 1 {
                "finding"
            } else {
                "findings"
            };
            format!("  {} {label}", self.found)
        } else {
            "  working…".to_string()
        };
        let hint = if self.chatting {
            "esc to quit"
        } else if self.finished {
            "enter to ask  ·  esc to quit"
        } else {
            "q to cancel"
        };
        let text = format!("{lead}{}  ·  {hint}", self.usage_label());
        frame.render_widget(
            Paragraph::new(Line::from(text)).style(Style::default().fg(Color::DarkGray)),
            area,
        );
    }

    /// Context (input) and output token spend, with cost when priced. Input
    /// tokens are the context length fed to the model.
    fn usage_label(&self) -> String {
        let Some(u) = self.usage.filter(|u| u.total_tokens > 0) else {
            return String::new();
        };
        let approx = if u.estimated { "~" } else { "" };
        let mut s = format!(
            "  ·  ctx {approx}{} in / {approx}{} out",
            human_count(u.prompt_tokens as usize),
            human_count(u.completion_tokens as usize),
        );
        if let Some(cost) = u.estimated_cost_usd {
            s.push_str(&format!("  ·  ~${cost:.4}"));
        }
        s
    }
}

/// The ASTER sunset wordmark, centered. Shared by the review and chat TUIs.
fn draw_banner(frame: &mut Frame, area: Rect) {
    let lines: Vec<Line> = (0..5)
        .map(|r| {
            let text = LETTERS.iter().map(|g| g[r]).collect::<Vec<_>>().join(" ");
            Line::from(Span::styled(text, Style::default().fg(SUNSET[r])))
        })
        .collect();
    frame.render_widget(Paragraph::new(lines).alignment(Alignment::Center), area);
}

/// A rounded input field showing a placeholder, the typed text with a caret,
/// or a "thinking" spinner while a reply is in flight.
fn draw_input_box(
    frame: &mut Frame,
    area: Rect,
    input: &str,
    thinking: bool,
    spinner: usize,
    placeholder: &str,
) {
    let (line, border) = if thinking {
        (
            Line::from(Span::styled(
                format!("{} Aster is thinking…", SPINNER[spinner]),
                Style::default().fg(ACCENT),
            )),
            ACCENT,
        )
    } else if input.is_empty() {
        (
            Line::from(Span::styled(
                placeholder.to_string(),
                Style::default().fg(Color::DarkGray),
            )),
            Color::DarkGray,
        )
    } else {
        (
            Line::from(vec![
                Span::raw(input.to_string()),
                Span::styled("▏", Style::default().fg(ACCENT)),
            ]),
            ACCENT,
        )
    };
    frame.render_widget(
        Paragraph::new(line)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(border))
                    .padding(Padding::horizontal(1)),
            )
            .wrap(Wrap { trim: false }),
        area,
    );
}

/// A standalone conversational chat TUI: the full agent (read/search, and edits
/// when allowed), driven from `aster chat --tui`. Each turn runs as a spawned
/// task so the UI keeps animating while the model works.
pub async fn run_chat(
    mut client: AiClient,
    repo_root: std::path::PathBuf,
    allow_edits: bool,
    seed: Option<String>,
) -> Result<()> {
    let guard = TuiGuard::install();
    let mut terminal = ratatui::init();
    let mut app = ChatApp::new(allow_edits, client.model.clone());
    let mut turn: Option<ChatTurn> = None;

    if let Some(seed) = seed.filter(|s| !s.trim().is_empty()) {
        turn = Some(app.submit(&seed, &client, &repo_root));
    }

    let outcome = loop {
        terminal.draw(|frame| app.draw(frame))?;

        if event::poll(Duration::from_millis(80))?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            let ctrl_c = key.modifiers.contains(KeyModifiers::CONTROL)
                && matches!(key.code, KeyCode::Char('c'));
            if ctrl_c || matches!(key.code, KeyCode::Esc) {
                if let Some(t) = &turn {
                    t.abort();
                }
                break Ok(());
            }
            match key.code {
                KeyCode::Enter if turn.is_none() && !app.input.trim().is_empty() => {
                    let text = std::mem::take(&mut app.input);
                    // A leading slash is a local command (e.g. /model), not a
                    // message to the model.
                    if let Some(cmd) = text.trim().strip_prefix('/') {
                        app.handle_command(cmd, &mut client);
                    } else {
                        turn = Some(app.submit(&text, &client, &repo_root));
                    }
                }
                KeyCode::Backspace => {
                    app.input.pop();
                }
                KeyCode::Char(c) => app.input.push(c),
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

struct ChatApp {
    lines: Vec<Line<'static>>,
    input: String,
    thinking: bool,
    spinner: usize,
    usage: Option<aster_ai::UsageSnapshot>,
    allow_edits: bool,
    /// The active model id, shown in the header and changed with `/model`.
    model: String,
    /// Full conversation (user/assistant turns), carried forward each turn.
    history: Vec<ChatMessage>,
}

impl ChatApp {
    fn new(allow_edits: bool, model: String) -> Self {
        Self {
            lines: Vec::new(),
            input: String::new(),
            thinking: false,
            spinner: 0,
            usage: None,
            allow_edits,
            model,
            history: Vec::new(),
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
            "help" | "h" => {
                self.push_system("commands: /model <id> to switch, /model to show, /help")
            }
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
        let allow_edits = self.allow_edits;
        tokio::spawn(async move {
            crate::chat::agent_turn(client, repo_root, history, allow_edits).await
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

    fn draw(&self, frame: &mut Frame) {
        let area = frame.area();
        let banner = area.width >= 42 && area.height >= 16;

        let mut constraints = Vec::new();
        if banner {
            constraints.push(Constraint::Length(1));
            constraints.push(Constraint::Length(5));
        }
        constraints.push(Constraint::Length(1));
        constraints.push(Constraint::Min(0));
        constraints.push(Constraint::Length(3));
        constraints.push(Constraint::Length(1));
        let rows = Layout::vertical(constraints).split(area);

        let mut i = 0;
        if banner {
            draw_banner(frame, rows[1]);
            i = 2;
        }
        let header = rows[i];
        let body = rows[i + 1];
        let input = rows[i + 2];
        let footer = rows[i + 3];

        // Header: mark + "Aster chat" + live status.
        let mark = if self.thinking {
            Span::styled(
                format!("{} ", SPINNER[self.spinner]),
                Style::default().fg(ACCENT),
            )
        } else {
            Span::styled("✳ ", Style::default().fg(ACCENT))
        };
        let status = if self.thinking { "thinking" } else { "chat" };
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                mark,
                Span::styled(
                    "Aster",
                    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                ),
                Span::styled(format!("  {status}"), Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("  ·  {}", self.model),
                    Style::default().fg(Color::DarkGray),
                ),
            ])),
            header,
        );

        // Body: the conversation, or an empty-state hint.
        let content: Vec<Line> = if self.lines.is_empty() {
            let mut hint = vec![Line::from(""), dim("  Ask Aster anything about this repo.")];
            hint.push(dim(if self.allow_edits {
                "  It can read, search, and edit files here."
            } else {
                "  It can read and search files here."
            }));
            hint.push(Line::from(""));
            hint.push(dim("  /model <id> to switch models  ·  /help for commands"));
            hint
        } else {
            self.lines.clone()
        };
        let visible = body.height.saturating_sub(2) as usize;
        let scroll = content.len().saturating_sub(visible) as u16;
        frame.render_widget(
            Paragraph::new(content)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_type(BorderType::Rounded)
                        .border_style(Style::default().fg(Color::DarkGray))
                        .padding(Padding::horizontal(1)),
                )
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
        frame.render_widget(
            Paragraph::new(Line::from(format!("{usage}  ·  {hint}")))
                .style(Style::default().fg(Color::DarkGray)),
            footer,
        );
    }
}

/// Reprint the outcome to the real terminal after the TUI closes, so results
/// survive in scrollback instead of vanishing with the alternate screen.
fn print_summary(resp: &ReviewReport, min_confidence: f32) {
    let findings: Vec<_> = resp
        .findings
        .iter()
        .filter(|f| f.confidence.unwrap_or(1.0) >= min_confidence)
        .collect();

    println!("\n{}\n", resp.summary);
    for (i, f) in findings.iter().enumerate() {
        let conf = f
            .confidence
            .map(|c| format!("  {:.0}%", c * 100.0))
            .unwrap_or_default();
        println!(
            "[{}] {}  ({}/{})  {}:{}{}",
            i + 1,
            f.title,
            f.severity,
            f.category,
            f.file_path,
            f.line,
            conf
        );
        println!("    {}", f.description);
        println!("    fix: {}\n", f.suggestion);
    }
}

/// Compact number formatter (e.g. 1234 -> "1.2k"). Unitless; callers add the
/// unit so the same helper serves token and count displays.
fn human_count(n: usize) -> String {
    if n >= 1000 {
        format!("{:.1}k", n as f64 / 1000.0)
    } else {
        n.to_string()
    }
}

fn dim(text: impl Into<String>) -> Line<'static> {
    Line::from(Span::styled(
        text.into(),
        Style::default().fg(Color::DarkGray),
    ))
}

fn severity_chip(severity: &str) -> Span<'static> {
    let (bg, label) = match severity {
        "critical" => (Color::Red, "CRIT"),
        "high" => (Color::LightRed, "HIGH"),
        "medium" => (Color::Yellow, "MED"),
        "low" => (Color::Blue, "LOW"),
        _ => (Color::DarkGray, "INFO"),
    };
    Span::styled(
        format!(" {label} "),
        Style::default()
            .fg(Color::Black)
            .bg(bg)
            .add_modifier(Modifier::BOLD),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_command_model_switches_client_and_app() {
        let mut client = AiClient::new("http://localhost", "k", "openai/gpt-4o-mini");
        let mut app = ChatApp::new(false, client.model.clone());
        app.handle_command("model anthropic/claude-sonnet-5", &mut client);
        assert_eq!(client.model, "anthropic/claude-sonnet-5");
        assert_eq!(app.model, "anthropic/claude-sonnet-5");
    }

    #[test]
    fn chat_command_model_without_arg_keeps_model() {
        let mut client = AiClient::new("http://localhost", "k", "m1");
        let mut app = ChatApp::new(false, client.model.clone());
        app.handle_command("model", &mut client);
        assert_eq!(client.model, "m1");
        assert_eq!(app.model, "m1");
    }

    #[test]
    fn chat_command_unknown_is_reported() {
        let mut client = AiClient::new("http://localhost", "k", "m1");
        let mut app = ChatApp::new(false, client.model.clone());
        let before = app.lines.len();
        app.handle_command("bogus", &mut client);
        assert!(app.lines.len() > before);
    }
}
