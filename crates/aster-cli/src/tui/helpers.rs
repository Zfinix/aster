use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Padding, Paragraph, Wrap};

use super::{ACCENT, SPINNER};

/// A short, home-relative path for display, e.g. `~/projects/aster`.
pub(super) fn short_path(path: &std::path::Path) -> String {
    let full = path.display().to_string();
    if let Some(home) = dirs::home_dir()
        && let Ok(rel) = path.strip_prefix(&home)
    {
        return format!("~/{}", rel.display());
    }
    full
}

/// The Aster mark: an 8×8 pixel asterisk (a thick vertical and horizontal bar
/// through the center plus diagonal rays), rendered with half-block glyphs so
/// two pixel rows share one terminal cell. The grid is 8×8 and symmetric about
/// both axes, so the four output lines phase cleanly onto cell boundaries and
/// the mark reads as a square. Sunset gradient, pink (top) to gold (bottom),
/// matching the desktop logo. Reused by the review banner and the chat welcome
/// panel.
pub(super) fn mark_lines() -> Vec<Line<'static>> {
    // Per-row color, top to bottom, sampled from the SVG's sunset gradient.
    const ROW: [Color; 8] = [
        Color::Rgb(239, 90, 111),
        Color::Rgb(241, 110, 79),
        Color::Rgb(243, 132, 78),
        Color::Rgb(246, 152, 77),
        Color::Rgb(246, 170, 85),
        Color::Rgb(247, 186, 90),
        Color::Rgb(248, 196, 97),
        Color::Rgb(248, 203, 102),
    ];
    // Lit columns per row: the two center columns (vertical bar), the two center
    // rows (horizontal bar), and both diagonals through the center.
    const ON: [&[usize]; 8] = [
        &[0, 3, 4, 7],
        &[1, 3, 4, 6],
        &[2, 3, 4, 5],
        &[0, 1, 2, 3, 4, 5, 6, 7],
        &[0, 1, 2, 3, 4, 5, 6, 7],
        &[2, 3, 4, 5],
        &[1, 3, 4, 6],
        &[0, 3, 4, 7],
    ];
    let lit = |row: usize, col: usize| ON[row].contains(&col);
    // Two pixel rows per output line: the top pixel colors the upper half of the
    // cell, the bottom pixel the lower half.
    (0..8)
        .step_by(2)
        .map(|top| {
            let bottom = top + 1;
            let spans = (0..8)
                .map(|col| {
                    let t = lit(top, col);
                    let b = bottom < 8 && lit(bottom, col);
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

/// A rounded input field showing a placeholder, the typed text with a caret,
/// or a "thinking" spinner while a reply is in flight.
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

/// Compact number formatter (e.g. 1234 -> "1.2k"). Unitless; callers add the
/// unit so the same helper serves token and count displays.
pub(super) fn human_count(n: usize) -> String {
    if n >= 1000 {
        format!("{:.1}k", n as f64 / 1000.0)
    } else {
        n.to_string()
    }
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
