use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Padding, Paragraph, Wrap};

use super::{ACCENT, SPINNER};

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
    const ROW: [Color; 10] = [
        Color::Rgb(239, 90, 111),
        Color::Rgb(239, 90, 111),
        Color::Rgb(241, 110, 79),
        Color::Rgb(243, 130, 79),
        Color::Rgb(245, 152, 78),
        Color::Rgb(246, 168, 84),
        Color::Rgb(247, 182, 89),
        Color::Rgb(247, 193, 95),
        Color::Rgb(248, 203, 102),
        Color::Rgb(248, 203, 102),
    ];
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

/// Compact number formatter (e.g. 1234 -> "1.2k"). Unitless; callers add the unit.
pub(super) fn human_count(n: usize) -> String {
    if n >= 1000 {
        format!("{:.1}k", n as f64 / 1000.0)
    } else {
        n.to_string()
    }
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
    Line::from(Span::styled(
        text.into(),
        Style::default().fg(Color::DarkGray),
    ))
}

pub(super) fn severity_chip(severity: &str) -> Span<'static> {
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
