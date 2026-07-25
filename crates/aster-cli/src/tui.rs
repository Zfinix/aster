//! Live review TUI. Runs the whole review in the background and renders each
//! step as it happens — indexing, hypotheses, verification, findings landing —
//! so users watch it work instead of staring at a blank prompt.

use std::time::{Duration, Instant};
use std::{panic, sync};

use anyhow::Result;
use aster_ai::{AiClient, ChatMessage};
use aster_harness::Progress;
use aster_models::ReviewReport;
use aster_policy::{Mode, PermissionsConfig, Policy};
use ratatui::Frame;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Padding, Paragraph, Wrap};
use tokio::sync::mpsc;

use crate::chat::{ApprovalRequest, ApprovalSender};
use crate::review::{Job, execute};

const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
/// Brand orange, matching the desktop app's `--accent` (#f2764f).
const ACCENT: Color = Color::Rgb(242, 118, 79);

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

        if response.is_none() && task.is_finished() {
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
        // Show the mark only when there's room; fall back to a compact header.
        let banner = area.width >= 42 && area.height >= 18;
        // The chat input appears once the review finishes.
        let show_input = self.finished;

        let mut constraints = Vec::new();
        if banner {
            constraints.push(Constraint::Length(1));
            constraints.push(Constraint::Length(7));
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

/// A short, home-relative path for display, e.g. `~/projects/aster`.
fn short_path(path: &std::path::Path) -> String {
    let full = path.display().to_string();
    if let Some(home) = dirs::home_dir()
        && let Ok(rel) = path.strip_prefix(&home)
    {
        return format!("~/{}", rel.display());
    }
    full
}

/// The Aster mark: the 13×13 pixel asterisk from
/// `desktop/src/assets/aster-mark.svg`, rendered with half-block glyphs so two
/// pixel rows share one terminal cell. Sunset gradient, pink (top) to gold
/// (bottom), matching the desktop logo. Reused by the review banner and the chat
/// welcome panel.
fn mark_lines() -> Vec<Line<'static>> {
    // Per-row color, top to bottom, straight from the SVG.
    const ROW: [Color; 13] = [
        Color::Rgb(239, 90, 111),
        Color::Rgb(239, 90, 111),
        Color::Rgb(240, 102, 79),
        Color::Rgb(242, 118, 79),
        Color::Rgb(244, 134, 78),
        Color::Rgb(247, 155, 77),
        Color::Rgb(247, 155, 77),
        Color::Rgb(246, 168, 84),
        Color::Rgb(247, 180, 88),
        Color::Rgb(247, 193, 92),
        Color::Rgb(247, 193, 92),
        Color::Rgb(248, 203, 102),
        Color::Rgb(248, 203, 102),
    ];
    // Lit columns per row, from the SVG rects: a vertical and horizontal bar
    // through the center plus diagonal rays.
    const ON: [&[usize]; 13] = [
        &[6],
        &[6],
        &[2, 6, 10],
        &[3, 6, 9],
        &[4, 6, 8],
        &[5, 6, 7],
        &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12],
        &[5, 6, 7],
        &[4, 6, 8],
        &[3, 6, 9],
        &[2, 6, 10],
        &[6],
        &[6],
    ];
    let lit = |row: usize, col: usize| ON[row].contains(&col);
    // Two pixel rows per output line: the top pixel colors the upper half of the
    // cell, the bottom pixel the lower half.
    (0..13)
        .step_by(2)
        .map(|top| {
            let bottom = top + 1;
            let spans = (0..13)
                .map(|col| {
                    let t = lit(top, col);
                    let b = bottom < 13 && lit(bottom, col);
                    match (t, b) {
                        (true, true) => {
                            Span::styled("▀", Style::default().fg(ROW[top]).bg(ROW[bottom]))
                        }
                        (true, false) => Span::styled("▀", Style::default().fg(ROW[top])),
                        (false, true) => Span::styled("▄", Style::default().fg(ROW[bottom])),
                        (false, false) => Span::raw(" "),
                    }
                })
                .collect::<Vec<_>>();
            Line::from(spans)
        })
        .collect()
}

fn draw_banner(frame: &mut Frame, area: Rect) {
    frame.render_widget(
        Paragraph::new(mark_lines()).alignment(Alignment::Center),
        area,
    );
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
    let prompt = Span::styled(
        "❯ ",
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
    );
    let (line, border) = if thinking {
        (
            Line::from(vec![
                Span::styled(
                    format!("{} ", SPINNER[spinner]),
                    Style::default().fg(ACCENT),
                ),
                Span::styled("Aster is thinking…", Style::default().fg(ACCENT)),
            ]),
            ACCENT,
        )
    } else if input.is_empty() {
        (
            Line::from(vec![
                prompt,
                Span::styled(
                    placeholder.to_string(),
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::ITALIC),
                ),
            ]),
            ACCENT,
        )
    } else {
        (
            Line::from(vec![
                prompt,
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

    fn draw(&self, frame: &mut Frame) {
        let area = frame.area();

        // No banner or header row: the welcome panel already carries the identity,
        // so the conversation box sits right at the top with minimal chrome.
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
        let visible = body.height.saturating_sub(2) as usize;
        let content: Vec<Line> = if self.lines.is_empty() {
            self.welcome_lines()
        } else {
            self.lines.clone()
        };
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
    use aster_models::{Finding, ReviewReport};

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

    fn finding(title: &str) -> Finding {
        Finding {
            file_path: "src/handlers.rs".into(),
            line: 4,
            start_line: None,
            side: None,
            severity: "critical".into(),
            category: "security".into(),
            title: title.into(),
            description: "desc".into(),
            suggestion: "fix it".into(),
            code_snippet: None,
            confidence: Some(0.97),
        }
    }

    #[test]
    fn review_tui_chat_carries_findings_into_messages() {
        // Regression: the review's findings must reach the follow-up chat. The
        // pipeline's Done event marks the app finished, so context capture must
        // not be gated on `finished`.
        let mut app = App::new(0.0);
        let report = ReviewReport::new(
            "summary".into(),
            vec![finding("SQL Injection vulnerability")],
            vec![],
        );
        app.set_report_context(&report);
        let msgs = app.build_chat("how do i fix it");
        assert!(
            msgs.iter()
                .any(|m| m.content.contains("SQL Injection vulnerability")),
            "chat messages must include the review findings as context"
        );
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
