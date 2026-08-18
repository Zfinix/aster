//! Transcript cells.
//!
//! Every cell renders to a finished block of lines, already wrapped to the
//! terminal width, that the chat loop pushes into the terminal's own scrollback
//! and never touches again. That is what gives the transcript native scrolling,
//! selection and copy, and it is why nothing here needs mutable state.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use crate::chat::PlanStepStatus;
use crate::tui::theme;
use crate::tui::wrap;

/// Columns reserved for the `• ` / `  ` gutter every cell hangs from.
const GUTTER: usize = 2;
/// Output lines shown before the middle is elided.
const HEAD: usize = 4;
/// Output lines shown after an elision.
const TAIL: usize = 4;

#[allow(dead_code)]
fn dim() -> Style {
    theme::get().dim_style()
}

fn body_width(width: usize) -> usize {
    width.saturating_sub(GUTTER + 2).max(8)
}

/// Prefix `lines` with the cell bullet and hang the rest off a matching indent.
fn hang(lines: Vec<Line<'static>>, bullet: Span<'static>, width: usize) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    let mut first = true;
    for line in lines {
        for wrapped in wrap::wrap_line(line, body_width(width)) {
            let lead = if first {
                bullet.clone()
            } else {
                Span::raw(" ".repeat(GUTTER))
            };
            let mut spans = vec![lead];
            spans.extend(wrapped.spans);
            out.push(Line::from(spans).style(wrapped.style));
            first = false;
        }
    }
    out
}

/// Every cell hangs off this bullet; it is quiet on purpose, so the headline
/// beside it carries the emphasis.
fn bullet() -> Span<'static> {
    Span::styled("• ", theme::get().dimmer_style())
}

/// The branch a cell's children hang from: `└ ` on the first row, aligned
/// blanks after it.
fn branch(first: bool) -> Span<'static> {
    if first {
        Span::styled("└ ", theme::get().faint_style())
    } else {
        Span::raw("  ")
    }
}

/// Turn summary: how long the work took and what it cost in tokens.
#[allow(dead_code)]
pub(super) fn worked_summary(
    elapsed: std::time::Duration,
    down: u64,
    up: u64,
    estimated: bool,
    width: usize,
) -> Vec<Line<'static>> {
    let time = super::helpers::elapsed(elapsed.as_secs());
    let approx = if estimated { "~" } else { "" };
    let label = format!(
        "Worked for {time} · ↓ {approx}{} ↑ {approx}{}",
        super::helpers::human_count(down as usize),
        super::helpers::human_count(up as usize),
    );
    let pad = width.saturating_sub(label.chars().count() + 1);
    prepend_blank(vec![Line::from(vec![
        Span::styled(label, theme::get().dimmer_style()),
        Span::styled(
            format!(" {}", "─".repeat(pad.max(1))),
            theme::get().faint_style(),
        ),
    ])])
}

/// A message the user sent: a coral rail on a filled band, the only chapter
/// mark in the transcript.
pub(super) fn user(text: &str, width: usize) -> Vec<Line<'static>> {
    let fill = Style::default()
        .fg(theme::get().text)
        .bg(theme::get().rail_bg);
    let body = body_width(width);
    let mut out = Vec::new();
    let mut first = true;
    for raw in text.lines() {
        for chunk in wrap::lines(raw, body) {
            let lead = if first { "❯ " } else { "  " };
            let line = Line::from(vec![
                Span::styled("▌", theme::get().accent_style().bg(theme::get().rail_bg)),
                Span::styled(lead, theme::get().accent_style().bg(theme::get().rail_bg)),
                Span::styled(chunk, fill),
            ]);
            out.push(wrap::pad_to(line, width.max(1), fill));
            first = false;
        }
    }
    prepend_blank(out)
}

/// Pre-rendered markdown from the model. `first` draws the bullet; later
/// chunks of the same message continue under it.
pub(super) fn assistant(
    lines: Vec<Line<'static>>,
    first: bool,
    width: usize,
) -> Vec<Line<'static>> {
    if lines.is_empty() {
        return Vec::new();
    }
    let lead = if first {
        bullet()
    } else {
        Span::raw(" ".repeat(GUTTER))
    };
    let block = hang(lines, lead, width);
    if first { prepend_blank(block) } else { block }
}

/// A short status line from the harness rather than the model.
pub(super) fn notice(text: &str, width: usize) -> Vec<Line<'static>> {
    hang(
        vec![Line::from(Span::styled(
            text.to_string(),
            theme::get().dimmer_style(),
        ))],
        Span::styled("· ", theme::get().dimmer_style()),
        width,
    )
}

/// Web-search source citations rendered as dim links below the answer.
pub(super) fn citations(
    sources: &[crate::tui::chat::Citation],
    width: usize,
) -> Vec<Line<'static>> {
    let dim = theme::get().dim_style();
    let mut lines = vec![Line::from(Span::styled("Sources", dim))];
    for src in sources {
        let label = src.title.as_deref().unwrap_or(&src.url);
        let text = format!("{label} — {url}", url = src.url);
        lines.push(Line::from(Span::styled(text, dim)));
    }
    hang(
        lines,
        Span::styled("· ", theme::get().dimmer_style()),
        width,
    )
}

pub(super) fn error(text: &str, width: usize) -> Vec<Line<'static>> {
    let lines = text
        .lines()
        .map(|l| {
            Line::from(Span::styled(
                l.to_string(),
                Style::default().fg(theme::get().error),
            ))
        })
        .collect();
    prepend_blank(hang(
        lines,
        Span::styled("✗ ", Style::default().fg(theme::get().error)),
        width,
    ))
}

/// Failures framed in a red rounded box so they cannot be read as notes.
pub(super) fn error_box(texts: &[String], width: usize) -> Vec<Line<'static>> {
    let style = theme::get().error_style();
    let inner = body_width(width).saturating_sub(4).max(8);
    let rows: Vec<String> = texts
        .iter()
        .flat_map(|text| wrap::lines(text, inner))
        .collect();
    let Some(box_width) = rows.iter().map(|row| wrap::width(row)).max() else {
        return Vec::new();
    };
    let margin = " ".repeat(GUTTER);
    let edge = "─".repeat(box_width + 2);
    let mut out = vec![Line::from(Span::styled(format!("{margin}╭{edge}╮"), style))];
    for row in rows {
        let pad = " ".repeat(box_width - wrap::width(&row));
        out.push(Line::from(Span::styled(
            format!("{margin}│ {row}{pad} │"),
            style,
        )));
    }
    out.push(Line::from(Span::styled(format!("{margin}╰{edge}╯"), style)));
    prepend_blank(out)
}

/// One read-only tool call, emitted the moment it lands. The first row of a
/// run opens the cell with its header; the rest hang off the same branch, so a
/// twelve-file sweep still reads as a single step while it prints live.
pub(super) fn explored_row(label: &str, open: bool, width: usize) -> Vec<Line<'static>> {
    let row = Line::from(vec![
        branch(!open),
        Span::styled(label.to_string(), Style::default().fg(theme::get().blue)),
    ]);
    if open {
        return hang(vec![row], Span::raw(" ".repeat(GUTTER)), width);
    }
    let header = Line::from(Span::styled(
        "Explored".to_string(),
        Style::default()
            .fg(theme::get().text)
            .add_modifier(Modifier::BOLD),
    ));
    prepend_blank(hang(vec![header, row], bullet(), width))
}

/// The agent's plan: a count line over one row per step. Done steps are struck
/// back to dim, the running one is accented, so the eye lands on what is live.
pub(super) fn plan(steps: &[(PlanStepStatus, String)], width: usize) -> Vec<Line<'static>> {
    if steps.is_empty() {
        return Vec::new();
    }
    let count = |want: PlanStepStatus| steps.iter().filter(|(s, _)| *s == want).count();
    let mut parts = vec![format!("{} done", count(PlanStepStatus::Done))];
    if count(PlanStepStatus::InProgress) > 0 {
        parts.push(format!("{} in progress", count(PlanStepStatus::InProgress)));
    }
    parts.push(format!("{} open", count(PlanStepStatus::Pending)));
    for (status, label) in [
        (PlanStepStatus::Blocked, "blocked"),
        (PlanStepStatus::Skipped, "skipped"),
    ] {
        if count(status) > 0 {
            parts.push(format!("{} {label}", count(status)));
        }
    }

    let mut lines = vec![Line::from(vec![
        Span::styled(
            format!(
                "{} task{}",
                steps.len(),
                if steps.len() == 1 { "" } else { "s" }
            ),
            Style::default()
                .fg(theme::get().text)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" ({})", parts.join(", ")),
            theme::get().dimmer_style(),
        ),
    ])];

    for (status, label) in steps {
        let (glyph, style) = match status {
            PlanStepStatus::Done => ("✔", theme::get().dimmer_style()),
            PlanStepStatus::InProgress => ("◼", theme::get().accent_style()),
            PlanStepStatus::Pending => ("◻", Style::default().fg(theme::get().dim)),
            PlanStepStatus::Skipped => ("⊘", theme::get().faint_style()),
            PlanStepStatus::Blocked => ("✖", Style::default().fg(theme::get().error)),
        };
        lines.push(Line::from(vec![
            Span::styled(format!("{glyph} "), style),
            Span::styled(label.clone(), step_style(*status)),
        ]));
    }
    prepend_blank(hang(lines, bullet(), width))
}

/// Done and skipped steps recede; only what is left to do reads at full weight.
fn step_style(status: PlanStepStatus) -> Style {
    match status {
        PlanStepStatus::Done => theme::get()
            .dimmer_style()
            .add_modifier(Modifier::CROSSED_OUT),
        PlanStepStatus::Skipped => theme::get()
            .faint_style()
            .add_modifier(Modifier::CROSSED_OUT),
        PlanStepStatus::InProgress => Style::default().fg(theme::get().text),
        PlanStepStatus::Pending => Style::default().fg(theme::get().dim),
        PlanStepStatus::Blocked => Style::default().fg(theme::get().error),
    }
}

/// A tool call with its output, elided in the middle when it is long.
pub(super) fn tool(label: &str, output: &str, failed: bool, width: usize) -> Vec<Line<'static>> {
    let head_style = if failed {
        Style::default()
            .fg(theme::get().error)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(theme::get().text)
            .add_modifier(Modifier::BOLD)
    };
    let mut lines = vec![Line::from(Span::styled(label.to_string(), head_style))];

    let body: Vec<&str> = output.lines().collect();
    let out_style = if failed {
        Style::default().fg(theme::get().error)
    } else {
        theme::get().dimmer_style()
    };
    for (i, text) in elide(&body).into_iter().enumerate() {
        let style = match text {
            Elided::Text(_) => out_style,
            Elided::Gap(_) => theme::get().faint_style(),
        };
        lines.push(Line::from(vec![
            branch(i == 0),
            Span::styled(text.into_string(), style),
        ]));
    }
    prepend_blank(hang(lines, bullet(), width))
}

/// The model's thinking. Collapsed it is one faint line naming its size, so a
/// long deliberation costs a row of scrollback; expanded it is the whole text,
/// dimmed to sit behind the answer rather than compete with it.
pub(super) fn reasoning(text: &str, open: bool, width: usize) -> Vec<Line<'static>> {
    let text = text.trim();
    if text.is_empty() {
        return Vec::new();
    }
    let header = Line::from(Span::styled(
        "Thinking".to_string(),
        Style::default()
            .fg(theme::get().text)
            .add_modifier(Modifier::BOLD),
    ));
    if !open {
        let words = text.split_whitespace().count();
        let hint = Line::from(vec![
            branch(true),
            Span::styled(
                format!("{words} words · /thinking to show"),
                theme::get().faint_style(),
            ),
        ]);
        return prepend_blank(hang(vec![header, hint], bullet(), width));
    }
    let mut lines = vec![header];
    for (i, body) in text.lines().enumerate() {
        lines.push(Line::from(vec![
            branch(i == 0),
            Span::styled(body.to_string(), theme::get().dimmer_style()),
        ]));
    }
    prepend_blank(hang(lines, bullet(), width))
}

/// An applied edit: `▸ verb path` with the counts pushed to the right edge,
/// then the tinted patch body.
pub(super) fn patch(verb: &str, path: &str, body: &str, width: usize) -> Vec<Line<'static>> {
    let added = body.lines().filter(|l| l.starts_with('+')).count();
    let removed = body.lines().filter(|l| l.starts_with('-')).count();

    let inner = body_width(width);
    let left = format!("{verb} {path}");
    let right = format!("+{added} −{removed}");
    let gap = inner.saturating_sub(left.chars().count() + right.chars().count() + 1);
    let header = Line::from(vec![
        Span::styled(
            format!("{verb} "),
            Style::default()
                .fg(theme::get().text)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(path.to_string(), Style::default().fg(theme::get().blue)),
        Span::raw(" ".repeat(gap + 1)),
        Span::styled(
            format!("+{added}"),
            Style::default().fg(theme::get().add_fg),
        ),
        Span::raw(" "),
        Span::styled(
            format!("−{removed}"),
            Style::default().fg(theme::get().del_fg),
        ),
    ]);

    let mut lines = vec![header];
    lines.extend(diff_lines(body, inner));
    prepend_blank(hang(lines, bullet(), width))
}

/// Colour a unified-ish patch body, tinting the whole row so added and removed
/// lines read as bands. The `+`/`-` mark sits a step darker than its text.
pub(super) fn diff_lines(body: &str, width: usize) -> Vec<Line<'static>> {
    body.lines()
        .map(|raw| {
            let (fg, bg, mark) = match raw.chars().next() {
                Some('+') => (
                    theme::get().add_fg,
                    theme::get().add_bg,
                    Some(theme::get().add_mark),
                ),
                Some('-') => (
                    theme::get().del_fg,
                    theme::get().del_bg,
                    Some(theme::get().del_mark),
                ),
                _ => (theme::get().faint, Color::Reset, None),
            };
            let style = Style::default().fg(fg).bg(bg);
            let text: String = wrap::lines(raw, width).first().cloned().unwrap_or_default();
            let line = match (mark, text.is_empty()) {
                (Some(mark_fg), false) => {
                    let (head, rest) = text.split_at(1);
                    Line::from(vec![
                        Span::styled(head.to_string(), Style::default().fg(mark_fg).bg(bg)),
                        Span::styled(rest.to_string(), style),
                    ])
                }
                _ => Line::from(Span::styled(text, style)),
            };
            wrap::pad_to(line, width, style)
        })
        .collect()
}

/// The session header, printed once above the first prompt: the mark, the
/// name and version, then the fields. No box; hints only for what you cannot
/// guess.
pub(super) fn welcome(fields: &[(&str, String)], width: usize) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = super::helpers::mark_lines();
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("aster", theme::get().accent_style()),
        Span::styled(
            format!("  {}", env!("CARGO_PKG_VERSION")),
            theme::get().dimmer_style(),
        ),
    ]));
    lines.push(Line::from(""));
    // Widest key plus a gap, so a key as long as the column still separates.
    let key_w = fields
        .iter()
        .map(|(k, _)| k.chars().count())
        .max()
        .unwrap_or(0)
        + 2;
    // Long values (tool or skill lists) wrap into a hanging indent under the
    // value column instead of running off the edge.
    let value_w = width.saturating_sub(key_w).max(16);
    for (key, value) in fields {
        let (names, more) = super::helpers::split_more(value);
        let mut rows: Vec<Line<'static>> = wrap::lines(names, value_w)
            .into_iter()
            .map(|row| Line::from(Span::styled(row, Style::default().fg(theme::get().text))))
            .collect();
        // The count of what was cut trails the names, dimmed, and drops to its
        // own row when the last one has no space left.
        if let Some(more) = more {
            let tail = Span::styled(more.to_string(), theme::get().dimmer_style());
            let fits = rows
                .last()
                .is_some_and(|l| l.width() + 1 + wrap::width(more) <= value_w);
            match fits {
                true => {
                    let last = rows.last_mut().expect("fits implies a last row");
                    last.spans.push(Span::raw(" "));
                    last.spans.push(tail);
                }
                false => rows.push(Line::from(tail)),
            }
        }
        for (i, row) in rows.into_iter().enumerate() {
            let head = match i {
                0 => Span::styled(format!("{key:<key_w$}"), theme::get().dimmer_style()),
                _ => Span::raw(" ".repeat(key_w)),
            };
            let mut spans = vec![head];
            spans.extend(row.spans);
            lines.push(Line::from(spans));
        }
    }
    // None of the keys are on screen anywhere else, so the header is where a
    // first-time reader finds out they exist.
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "/help for commands and keys  ·  shift+tab changes mode  ·  esc esc quits",
        theme::get().dimmer_style(),
    )));
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("✨ ", theme::get().accent_style()),
        Span::styled(format!("Tip: {}", tip()), theme::get().text_style()),
    ]));
    lines.push(Line::from(""));
    prepend_blank(lines)
}

/// One-line feature reminders; the welcome shows one per session.
const TIPS: &[&str] = &[
    "aster --resume reopens your last session; /resume picks from a list",
    "@ mentions a file from this repo without typing the whole path",
    "/compact folds earlier turns into a summary when context runs low",
    "/model switches models mid-session; /effort sets the reasoning budget",
    "/status shows session, model, context, and token usage",
    "aster mcp list shows every MCP server and the tools it advertises",
    "/diff shows the repo's uncommitted changes without leaving the chat",
    "/memory lists what Aster remembers about this project",
    "/yolo drops the guardrails; the theme turns red while it is on",
    "ctrl+j adds a newline without sending the message",
    "↑ walks back through your past messages once the cursor is at the top",
    "enter during a running turn interrupts it and sends your message",
    "aster mcp import copies MCP servers from Claude Code, Codex, or Cursor",
    "/provider switches the endpoint Aster talks to, then picks a model",
    "/clear wipes the conversation and starts fresh",
    "/skills opens a picker to use, view, or delete a skill",
    "aster sessions list prints ids to use with aster --resume <id>",
    "/mcp enables or disables MCP servers from inside the chat",
    "/mode changes how the agent acts; shift+tab steps through them",
    "/effort cycles the reasoning budget when called with no argument",
];

/// Seeded off the hasher rather than the clock: system time lands on whole
/// microseconds, so `nanos % TIPS.len()` collapses to the same tip every launch.
fn tip() -> &'static str {
    use std::hash::BuildHasher;
    let seed = std::hash::RandomState::new().hash_one(TIPS.len()) as usize;
    TIPS[seed % TIPS.len()]
}

/// A newer release on GitHub: headline, changelog, and where to get it.
pub(super) fn update(info: &crate::update::UpdateInfo, width: usize) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from(Span::styled(
        format!("update available  {} → {}", info.current, info.latest),
        theme::get().text_style(),
    ))];
    for entry in &info.changelog {
        lines.push(Line::from(Span::styled(
            entry.clone(),
            theme::get().dim_style(),
        )));
    }
    if !info.url.is_empty() {
        lines.push(Line::from(Span::styled(
            info.url.clone(),
            Style::default().fg(theme::get().link_fg),
        )));
    }
    prepend_blank(hang(
        lines,
        Span::styled("✦ ", theme::get().accent_style()),
        width,
    ))
}

/// A review pipeline phase header, e.g. `▶ Verify`.
#[allow(dead_code)]
pub(super) fn phase(name: &str, width: usize) -> Vec<Line<'static>> {
    prepend_blank(hang(
        vec![Line::from(Span::styled(
            name.to_string(),
            theme::get().accent_style().add_modifier(Modifier::BOLD),
        ))],
        Span::styled("▶ ", theme::get().accent_style()),
        width,
    ))
}

/// A confirmed review finding: severity chip, title, location, confidence.
#[allow(dead_code)]
pub(super) fn finding(f: &aster_models::Finding, width: usize) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from(vec![
        super::helpers::severity_chip(&f.severity),
        Span::styled(
            format!(" {}", f.title),
            Style::default().add_modifier(Modifier::BOLD),
        ),
    ])];
    let confidence = f
        .confidence
        .map(|c| format!("  ·  {:.0}%", c * 100.0))
        .unwrap_or_default();
    lines.push(Line::from(Span::styled(
        format!("{}:{}{confidence}", f.file_path, f.line),
        dim(),
    )));
    for l in f.description.lines() {
        lines.push(Line::from(Span::styled(l.to_string(), dim())));
    }
    prepend_blank(hang(
        lines,
        Span::styled("✓ ", Style::default().fg(theme::get().success)),
        width,
    ))
}

/// A candidate the verifier rejected; one faint line keeps the noise down.
#[allow(dead_code)]
pub(super) fn refuted(title: &str, width: usize) -> Vec<Line<'static>> {
    hang(
        vec![Line::from(Span::styled(
            format!("refuted: {title}"),
            theme::get().dimmer_style(),
        ))],
        Span::styled("✗ ", theme::get().dimmer_style()),
        width,
    )
}

fn prepend_blank(mut lines: Vec<Line<'static>>) -> Vec<Line<'static>> {
    if lines.is_empty() {
        return lines;
    }
    lines.insert(0, Line::from(""));
    lines
}

/// One entry of an output body after the middle has been dropped.
enum Elided<'a> {
    Text(&'a str),
    Gap(usize),
}

impl Elided<'_> {
    fn into_string(self) -> String {
        match self {
            Elided::Text(s) => s.to_string(),
            Elided::Gap(n) => format!("… +{n} lines"),
        }
    }
}

fn elide<'a>(body: &[&'a str]) -> Vec<Elided<'a>> {
    if body.len() <= HEAD + TAIL + 1 {
        return body.iter().copied().map(Elided::Text).collect();
    }
    let mut out: Vec<Elided<'a>> = body[..HEAD].iter().copied().map(Elided::Text).collect();
    out.push(Elided::Gap(body.len() - HEAD - TAIL));
    out.extend(body[body.len() - TAIL..].iter().copied().map(Elided::Text));
    out
}

#[cfg(test)]
#[path = "tests/history_test.rs"]
mod tests;
