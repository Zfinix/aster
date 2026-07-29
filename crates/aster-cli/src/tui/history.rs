//! Transcript cells.
//!
//! Every cell renders to a finished block of lines, already wrapped to the
//! terminal width, that the chat loop pushes into the terminal's own scrollback
//! and never touches again. That is what gives the transcript native scrolling,
//! selection and copy, and it is why nothing here needs mutable state.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use crate::tui::wrap;
use crate::tui::{ACCENT, theme};

/// Columns reserved for the `• ` / `  ` gutter every cell hangs from.
const GUTTER: usize = 2;
/// Output lines shown before the middle is elided.
const HEAD: usize = 4;
/// Output lines shown after an elision.
const TAIL: usize = 4;

#[allow(dead_code)]
fn dim() -> Style {
    theme::dim()
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
    Span::styled("• ", theme::dimmer())
}

/// The branch a cell's children hang from: `└ ` on the first row, aligned
/// blanks after it.
fn branch(first: bool) -> Span<'static> {
    if first {
        Span::styled("└ ", theme::faint())
    } else {
        Span::raw("  ")
    }
}

/// A full-width divider between the agent's work and its answer.
pub(super) fn rule(width: usize) -> Vec<Line<'static>> {
    prepend_blank(vec![Line::from(Span::styled(
        "─".repeat(width.max(1)),
        theme::faint(),
    ))])
}

/// A message the user sent: a coral rail on a filled band, the only chapter
/// mark in the transcript.
pub(super) fn user(text: &str, width: usize) -> Vec<Line<'static>> {
    let fill = Style::default().fg(theme::TEXT).bg(theme::RAIL_BG);
    let body = body_width(width);
    let mut out = Vec::new();
    let mut first = true;
    for raw in text.lines() {
        for chunk in wrap::lines(raw, body) {
            let lead = if first { "❯ " } else { "  " };
            let line = Line::from(vec![
                Span::styled("▌", Style::default().fg(ACCENT).bg(theme::RAIL_BG)),
                Span::styled(lead, Style::default().fg(ACCENT).bg(theme::RAIL_BG)),
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
        vec![Line::from(Span::styled(text.to_string(), theme::dimmer()))],
        Span::styled("· ", theme::dimmer()),
        width,
    )
}

pub(super) fn error(text: &str, width: usize) -> Vec<Line<'static>> {
    let lines = text
        .lines()
        .map(|l| {
            Line::from(Span::styled(
                l.to_string(),
                Style::default().fg(theme::ROSE),
            ))
        })
        .collect();
    prepend_blank(hang(
        lines,
        Span::styled("✗ ", Style::default().fg(theme::ROSE)),
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
            .fg(theme::TEXT)
            .add_modifier(Modifier::BOLD),
    ))];
    for (i, label) in labels.iter().enumerate() {
        lines.push(Line::from(vec![
            branch(i == 0),
            Span::styled(label.clone(), Style::default().fg(theme::BLUE)),
        ]));
    }
    prepend_blank(hang(lines, bullet(), width))
}

/// A tool call with its output, elided in the middle when it is long.
pub(super) fn tool(label: &str, output: &str, failed: bool, width: usize) -> Vec<Line<'static>> {
    let head_style = if failed {
        Style::default()
            .fg(theme::ROSE)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(theme::TEXT)
            .add_modifier(Modifier::BOLD)
    };
    let mut lines = vec![Line::from(Span::styled(label.to_string(), head_style))];

    let body: Vec<&str> = output.lines().collect();
    let out_style = if failed {
        Style::default().fg(theme::ROSE)
    } else {
        theme::dimmer()
    };
    for (i, text) in elide(&body).into_iter().enumerate() {
        let style = match text {
            Elided::Text(_) => out_style,
            Elided::Gap(_) => theme::faint(),
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
                .fg(theme::TEXT)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(path.to_string(), Style::default().fg(theme::BLUE)),
        Span::raw(" ".repeat(gap + 1)),
        Span::styled(format!("+{added}"), Style::default().fg(theme::ADD_FG)),
        Span::raw(" "),
        Span::styled(format!("−{removed}"), Style::default().fg(theme::DEL_FG)),
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
                Some('+') => (theme::ADD_FG, theme::ADD_BG, Some(theme::ADD_MARK)),
                Some('-') => (theme::DEL_FG, theme::DEL_BG, Some(theme::DEL_MARK)),
                _ => (theme::FAINT, Color::Reset, None),
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
        Span::styled("aster", Style::default().fg(ACCENT)),
        Span::styled(format!("  v{}", env!("CARGO_PKG_VERSION")), theme::dimmer()),
    ]));
    lines.push(Line::from(""));
    for (key, value) in fields {
        lines.push(Line::from(vec![
            Span::styled(format!("{key:<8}"), theme::dimmer()),
            Span::styled(value.clone(), Style::default().fg(theme::TEXT)),
        ]));
    }
    prepend_blank(lines)
}

/// A review pipeline phase header, e.g. `▶ Verify`.
#[allow(dead_code)]
pub(super) fn phase(name: &str, width: usize) -> Vec<Line<'static>> {
    prepend_blank(hang(
        vec![Line::from(Span::styled(
            name.to_string(),
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ))],
        Span::styled("▶ ", Style::default().fg(ACCENT)),
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
        Span::styled("✓ ", Style::default().fg(Color::Green)),
        width,
    ))
}

/// A candidate the verifier rejected; one faint line keeps the noise down.
#[allow(dead_code)]
pub(super) fn refuted(title: &str, width: usize) -> Vec<Line<'static>> {
    hang(
        vec![Line::from(Span::styled(
            format!("refuted: {title}"),
            theme::dimmer(),
        ))],
        Span::styled("✗ ", theme::dimmer()),
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
        assert_eq!(rows[0].spans[0].style.fg, Some(crate::tui::theme::ADD_MARK));
        assert_eq!(rows[0].spans[1].style.fg, Some(crate::tui::theme::ADD_FG));
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
