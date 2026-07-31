//! Live review TUI: renders each step as it lands, then hands off to chat.

use std::sync;
use std::time::{Duration, Instant};

use anyhow::Result;
use aster_ai::ChatMessage;
use aster_harness::Progress;
use aster_models::ReviewReport;
use ratatui::Frame;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Padding, Paragraph, Wrap};

use super::guard::TuiGuard;
use super::helpers::{dim, draw_banner, draw_input_box, human_count, severity_chip};
use super::summary::print_summary;
use super::{SPINNER, theme};
use crate::review::{Job, execute};

const AGENT_PROMPT: &str = include_str!("../../prompts/aster-agent.md");
const CHAT_TEMPERATURE: f32 = 0.4;

/// Findings formatted as ground truth the chat agent answers follow-ups from.
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
    // Arc-shared usage counters: this clone reflects live token spend.
    let usage_handle = job.ai_client.clone();
    let mut task = tokio::spawn(async move { execute(job, &Some(tx)).await });

    theme::set(theme::Theme::DEFAULT);
    let guard = TuiGuard::install(ratatui::restore);

    let mut terminal = ratatui::init();
    let mut app = App::new(min_confidence);
    // Kept so results can be reprinted once the alternate screen is torn down.
    let mut response: Option<ReviewReport> = None;
    // Shares usage counters with the review client, into the same footer meter.
    let chat_client = usage_handle.clone();
    let mut chat_task: Option<tokio::task::JoinHandle<Result<String>>> = None;

    let outcome = loop {
        terminal.draw(|frame| app.draw(frame))?;

        // Non-blocking so the feed keeps updating between keystrokes.
        if event::poll(Duration::from_millis(80))?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
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

            // Scrollback works in both modes, from keys the active mode ignores.
            match key.code {
                KeyCode::PageUp => app.scroll = app.scroll.saturating_add(10),
                KeyCode::PageDown => app.scroll = app.scroll.saturating_sub(10),
                KeyCode::Home => app.scroll = u16::MAX,
                KeyCode::End => app.scroll = 0,
                _ => {}
            }

            if !app.finished {
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => {
                        task.abort();
                        break Ok(());
                    }
                    KeyCode::Up => app.scroll = app.scroll.saturating_add(1),
                    KeyCode::Down => app.scroll = app.scroll.saturating_sub(1),
                    _ => {}
                }
            } else {
                match key.code {
                    KeyCode::Esc => break Ok(()),
                    KeyCode::Up => app.scroll = app.scroll.saturating_add(1),
                    KeyCode::Down => app.scroll = app.scroll.saturating_sub(1),
                    KeyCode::Enter if !app.chatting && !app.input.trim().is_empty() => {
                        let text = std::mem::take(&mut app.input);
                        app.push_user(&text);
                        app.scroll = 0;
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

    // Restore before printing so the summary lands on the normal screen.
    drop(guard);
    if let Some(resp) = &response {
        print_summary(resp, min_confidence);
    }
    outcome
}

struct App {
    lines: Vec<Line<'static>>,
    status: String,
    spinner: usize,
    finished: bool,
    found: usize,
    stream_chars: usize,
    /// Chars streamed across the whole review; drives a continuously climbing
    /// token meter rather than one that jumps only when real usage lands.
    total_stream_chars: usize,
    started: Instant,
    /// Frozen at completion so the header timer stops once the review is done.
    elapsed: Option<Duration>,
    usage: Option<aster_ai::UsageSnapshot>,
    min_confidence: f32,
    input: String,
    chatting: bool,
    chat_msgs: Vec<ChatMessage>,
    ctx: Option<String>,
    /// Lines scrolled up from the bottom; `0` follows the live stream. Clamped in
    /// `draw`, left alone as lines stream in so reading back is never yanked down.
    scroll: u16,
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
            scroll: 0,
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

    fn set_report_context(&mut self, report: &ReviewReport) {
        self.ctx = Some(review_context(report, self.min_confidence));
    }

    /// One chat turn: persona, review context, prior turns, then the new
    /// question. Records the user message so the next turn carries it forward.
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
            Span::styled("❯ ", theme::get().accent_style()),
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
                    Span::styled("✳ ", Style::default().fg(theme::get().success)),
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
            Style::default().fg(theme::get().error),
        )));
    }

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
                    Span::styled("▶ ", theme::get().accent_style()),
                    Span::styled(
                        name,
                        theme::get().accent_style().add_modifier(Modifier::BOLD),
                    ),
                ]));
            }
            Progress::Token { delta, .. } => {
                // Count tokens for the header meter without dumping them in the log.
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
                        Style::default().fg(theme::get().faint),
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
                            .fg(theme::get().success)
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
                        Style::default().fg(theme::get().blue),
                    ),
                ]));
                self.lines.push(dim(format!("      {}", f.description)));
            }
            Progress::Refuted { title, .. } => {
                self.lines.push(Line::from(vec![
                    Span::styled("  ✗ ", Style::default().fg(theme::get().error)),
                    Span::styled(
                        format!("refuted  {title}"),
                        Style::default().fg(theme::get().faint),
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

    fn draw(&mut self, frame: &mut Frame) {
        let area = frame.area();
        // Show the mark only when there's room; fall back to a compact header.
        let banner = area.width >= 42 && area.height >= 16;
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

        let visible = body.height as usize;
        let max_scroll = self.lines.len().saturating_sub(visible) as u16;
        self.scroll = self.scroll.min(max_scroll);
        let scroll = max_scroll - self.scroll;
        frame.render_widget(
            Paragraph::new(self.lines.clone())
                .block(Block::default().padding(Padding::horizontal(1)))
                .wrap(Wrap { trim: false })
                .scroll((scroll, 0)),
            body,
        );

        if let Some(input) = input {
            self.draw_input(frame, input);
        }
        self.draw_footer(frame, footer);
    }

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
            Span::styled("✳ ", Style::default().fg(theme::get().success))
        } else {
            Span::styled(
                format!("{} ", SPINNER[self.spinner]),
                theme::get().accent_style(),
            )
        };
        let mut spans = vec![mark];
        if show_name {
            spans.push(Span::styled(
                "Aster",
                theme::get().accent_style().add_modifier(Modifier::BOLD),
            ));
            spans.push(Span::styled(
                format!("  {}", self.status),
                Style::default().fg(theme::get().faint),
            ));
        } else {
            spans.push(Span::styled(
                self.status.clone(),
                Style::default().fg(theme::get().faint),
            ));
        }
        // Continuously climbing meter so a long call visibly progresses. ~4 chars/token.
        if !self.finished && self.total_stream_chars > 0 {
            spans.push(Span::styled(
                format!("  ▸ {} tokens", human_count(self.total_stream_chars / 4)),
                theme::get().accent_style(),
            ));
        }
        frame.render_widget(Paragraph::new(Line::from(spans)), left);

        let elapsed = format!("{:.1}s ", self.elapsed().as_secs_f64());
        frame.render_widget(
            Paragraph::new(Line::from(elapsed))
                .alignment(Alignment::Right)
                .style(Style::default().fg(theme::get().faint)),
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
            Paragraph::new(Line::from(text)).style(Style::default().fg(theme::get().faint)),
            area,
        );
    }

    /// Input and output token spend, with cost when priced.
    fn usage_label(&self) -> String {
        let Some(u) = self.usage.filter(|u| u.total_tokens > 0) else {
            return String::new();
        };
        let approx = if u.estimated { "~" } else { "" };
        let mut s = format!(
            "  ·  ↑{approx}{} ↓{approx}{}",
            human_count(u.prompt_tokens as usize),
            human_count(u.completion_tokens as usize),
        );
        if let Some(cost) = u.estimated_cost_usd {
            s.push_str(&format!("  ·  ~${cost:.4}"));
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aster_models::{Finding, ReviewReport};

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
        // Regression: findings must reach chat; context capture must not be gated on `finished`.
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
}
