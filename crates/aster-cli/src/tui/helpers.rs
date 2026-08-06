use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Padding, Paragraph, Wrap};

use super::{SPINNER, theme};

pub(super) fn short_path(path: &std::path::Path) -> String {
    let full = path.display().to_string();
    if let Some(home) = dirs::home_dir()
        && let Ok(rel) = path.strip_prefix(&home)
    {
        return format!("~/{}", rel.display());
    }
    full
}

/// The Aster mark: an asterisk in half-block glyphs, tinted with the gradient.
pub(super) fn mark_lines() -> Vec<Line<'static>> {
    let row = theme::get().mark;
    const ON: [&[usize]; 10] = [
        &[4],
        &[1, 4, 7],
        &[2, 4, 6],
        &[3, 4, 5],
        &[0, 1, 2, 3, 4, 5, 6, 7, 8],
        &[3, 4, 5],
        &[2, 4, 6],
        &[1, 4, 7],
        &[4],
        &[],
    ];
    let lit = |row: usize, col: usize| row < 10 && ON[row].contains(&col);
    (0..10)
        .step_by(2)
        .map(|top| {
            let bottom = top + 1;
            let spans = (0..9)
                .map(|col| {
                    let t = lit(top, col);
                    let b = lit(bottom, col);
                    match (t, b) {
                        (true, true) => {
                            Span::styled("▀", Style::default().fg(row[top]).bg(row[bottom]))
                        }
                        (true, false) => Span::styled("▀", Style::default().fg(row[top])),
                        (false, true) => Span::styled("▄", Style::default().fg(row[bottom])),
                        (false, false) => Span::raw(" "),
                    }
                })
                .collect::<Vec<_>>();
            Line::from(spans)
        })
        .collect()
}

/// The mark as plain ANSI escape codes, for non-ratatui output (e.g. `aster init`).
pub(crate) fn mark_ansi() -> String {
    let mut out = String::new();
    for line in mark_lines() {
        for span in &line.spans {
            let style = span.style;
            if let Some(fg) = style.fg
                && let Color::Rgb(r, g, b) = fg
            {
                out.push_str(&format!("\x1b[38;2;{r};{g};{b}m"));
            }
            if let Some(bg) = style.bg
                && let Color::Rgb(r, g, b) = bg
            {
                out.push_str(&format!("\x1b[48;2;{r};{g};{b}m"));
            }
            out.push_str(&span.content);
            out.push_str("\x1b[0m");
        }
        out.push('\n');
    }
    out
}

pub(super) fn draw_banner(frame: &mut Frame, area: Rect) {
    frame.render_widget(
        Paragraph::new(mark_lines()).alignment(Alignment::Center),
        area,
    );
}

/// Rounded input field: placeholder, typed text with caret, or thinking spinner.
pub(super) fn draw_input_box(
    frame: &mut Frame,
    area: Rect,
    input: &str,
    thinking: bool,
    spinner: usize,
    placeholder: &str,
) {
    let prompt = Span::styled("❯ ", theme::get().accent_bold());
    let (line, border) = if thinking {
        (
            Line::from(vec![
                Span::styled(
                    format!("{} ", SPINNER[spinner]),
                    theme::get().accent_style(),
                ),
                Span::styled("Aster is thinking…", theme::get().accent_style()),
            ]),
            theme::get().accent,
        )
    } else if input.is_empty() {
        (
            Line::from(vec![
                prompt,
                Span::styled(
                    placeholder.to_string(),
                    Style::default()
                        .fg(theme::get().faint)
                        .add_modifier(Modifier::ITALIC),
                ),
            ]),
            theme::get().accent,
        )
    } else {
        (
            Line::from(vec![
                prompt,
                Span::raw(input.to_string()),
                Span::styled("▏", theme::get().accent_style()),
            ]),
            theme::get().accent,
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

/// Compact number formatter (e.g. 1234 -> "1.2k"). Delegates to the shared util.
pub(super) fn human_count(n: usize) -> String {
    crate::util::human(n as u64)
}

/// Join names with commas, keeping the first `max` and counting the rest.
pub(super) fn listed<'a>(names: impl Iterator<Item = &'a str>, max: usize) -> String {
    let all: Vec<&str> = names.collect();
    if all.len() <= max {
        return all.join(", ");
    }
    format!("{} … +{} more", all[..max].join(", "), all.len() - max)
}

/// Clip to `max` display columns with an ellipsis. Measured in columns, not
/// chars, so wide CJK text still lands on one row.
pub(super) fn clip_row(text: &str, max: usize) -> String {
    let flat = text.replace('\n', " ");
    if super::wrap::width(&flat) <= max {
        return flat;
    }
    let head = super::wrap::rows(&flat, max.saturating_sub(1).max(1))
        .first()
        .map(|r| flat[r.clone()].to_string())
        .unwrap_or_default();
    format!("{}…", head.trim_end())
}

/// Clip a one-line label, on a char boundary so multi-byte text survives.
pub(super) fn truncate_label(text: &str, max: usize) -> String {
    let flat = text.replace('\n', " ");
    if flat.chars().count() <= max {
        return flat;
    }
    let kept: String = flat.chars().take(max.saturating_sub(1)).collect();
    format!("{kept}…")
}

pub(super) fn dim(text: impl Into<String>) -> Line<'static> {
    Line::from(Span::styled(text.into(), theme::get().faint_style()))
}

pub(super) fn severity_chip(severity: &str) -> Span<'static> {
    let t = theme::get();
    let (bg, label) = match severity {
        "critical" => (t.severity_critical, "CRIT"),
        "high" => (t.severity_high, "HIGH"),
        "medium" => (t.severity_medium, "MED"),
        "low" => (t.severity_low, "LOW"),
        _ => (t.severity_info, "INFO"),
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
    fn clip_row_keeps_short_text_and_cuts_long_text_at_the_column_budget() {
        assert_eq!(clip_row("short", 20), "short");
        let long = clip_row("a".repeat(50).as_str(), 20);
        assert!(long.ends_with('…'), "{long}");
        assert!(crate::tui::wrap::width(&long) <= 20, "{long}");
    }

    #[test]
    fn clip_row_measures_wide_glyphs_in_columns_not_chars() {
        let cjk = "寫論文學術論文引導我寫論文審查意見評估回覆論文審查";
        let clipped = clip_row(cjk, 20);
        assert!(clipped.ends_with('…'), "{clipped}");
        assert!(crate::tui::wrap::width(&clipped) <= 20, "{clipped}");
    }

    #[test]
    fn clip_row_flattens_newlines_into_one_row() {
        assert_eq!(clip_row("one\ntwo", 20), "one two");
    }
}
