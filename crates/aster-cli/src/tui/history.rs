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

/// A full-width divider between the agent's work and its answer.
pub(super) fn rule(width: usize) -> Vec<Line<'static>> {
    prepend_blank(vec![Line::from(Span::styled(
        "─".repeat(width.max(1)),
        theme::get().faint_style(),
    ))])
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
    let secs = elapsed.as_secs();
    let time = if secs >= 60 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else {
        format!("{secs}s")
    };
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

/// A run of read-only tool calls, collapsed into one cell so a twelve-file
/// sweep reads as a single step.
pub(super) fn explored(labels: &[String], width: usize) -> Vec<Line<'static>> {
    if labels.is_empty() {
        return Vec::new();
    }
    let mut lines = vec![Line::from(Span::styled(
        "Explored".to_string(),
        Style::default()
            .fg(theme::get().text)
            .add_modifier(Modifier::BOLD),
    ))];
    for (i, label) in labels.iter().enumerate() {
        lines.push(Line::from(vec![
            branch(i == 0),
            Span::styled(label.clone(), Style::default().fg(theme::get().blue)),
        ]));
    }
    prepend_blank(hang(lines, bullet(), width))
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
pub(super) fn welcome(fields: &[(&str, String)], _width: usize) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = super::helpers::mark_lines();
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("aster", theme::get().accent_style()),
        Span::styled(
            format!("  v{}", env!("CARGO_PKG_VERSION")),
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
    for (key, value) in fields {
        lines.push(Line::from(vec![
            Span::styled(format!("{key:<key_w$}"), theme::get().dimmer_style()),
            Span::styled(value.clone(), Style::default().fg(theme::get().text)),
        ]));
    }
    // None of the keys are on screen anywhere else, so the header is where a
    // first-time reader finds out they exist.
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "/help for commands and keys  ·  shift+tab changes mode  ·  esc esc quits",
        theme::get().dimmer_style(),
    )));
    prepend_blank(lines)
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
mod tests {
    use super::*;

    fn text_of(lines: &[Line<'static>]) -> Vec<String> {
        lines
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect()
    }

    #[test]
    fn long_output_is_elided_in_the_middle() {
        let body: Vec<String> = (0..30).map(|i| format!("line {i}")).collect();
        let out = tool("Ran cargo check", &body.join("\n"), false, 80);
        let rendered = text_of(&out);
        assert!(rendered.iter().any(|l| l.contains("… +22 lines")));
        assert!(rendered.iter().any(|l| l.contains("line 0")));
        assert!(rendered.iter().any(|l| l.contains("line 29")));
        assert!(!rendered.iter().any(|l| l.contains("line 15")));
    }

    #[test]
    fn short_output_is_kept_whole() {
        let out = text_of(&tool("Ran ls", "a\nb\nc", false, 80));
        assert!(out.iter().any(|l| l.ends_with('b')));
        assert!(!out.iter().any(|l| l.contains('…')));
    }

    #[test]
    fn a_patch_counts_its_added_and_removed_lines() {
        let out = text_of(&patch("Edited", "src/lib.rs", "- old\n- gone\n+ new\n", 80));
        assert!(out.iter().any(|l| l.contains("+1 −2")), "{out:?}");
    }

    #[test]
    fn diff_rows_are_padded_so_the_tint_spans_the_width() {
        let rows = diff_lines("+ new", 20);
        assert_eq!(rows[0].width(), 20);
        assert_eq!(
            rows[0].spans[0].style.fg,
            Some(crate::tui::theme::get().add_mark)
        );
        assert_eq!(
            rows[0].spans[1].style.fg,
            Some(crate::tui::theme::get().add_fg)
        );
    }

    #[test]
    fn a_key_as_wide_as_the_column_still_separates_from_its_value() {
        let fields = [
            ("model", "kimi-k3".to_string()),
            ("provider", "OpenRouter".to_string()),
        ];
        let out = text_of(&welcome(&fields, 80));
        assert!(
            out.iter().any(|l| l.contains("provider  OpenRouter")),
            "{out:?}"
        );
        assert!(
            out.iter().any(|l| l.contains("model     kimi-k3")),
            "{out:?}"
        );
    }

    #[test]
    fn explored_collapses_into_one_cell() {
        let labels = vec!["Read a.rs".to_string(), "Read b.rs".to_string()];
        let out = text_of(&explored(&labels, 80));
        assert!(out.iter().any(|l| l.contains("Explored")));
        assert_eq!(out.iter().filter(|l| l.contains("Read ")).count(), 2);
    }

    #[test]
    fn continuation_lines_hang_under_the_bullet() {
        let out = text_of(&user(
            "a fairly long sentence that has to wrap somewhere",
            24,
        ));
        assert!(out[1].starts_with("▌❯ "));
        assert!(out[2].starts_with("▌  "));
    }
}
