//! Streaming markdown: block structure is tracked per line so committed
//! output never changes, while inline markup and tables go through
//! pulldown-cmark. `push` emits lines as they complete; `flush` drains.

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::theme;

fn dim() -> Style {
    theme::get().dim_style()
}

fn code_chip() -> Style {
    theme::get().code_style()
}

#[derive(Default)]
pub(super) struct MarkdownStream {
    buf: String,
    in_fence: bool,
    /// Rows of an in-flight table, held until the table ends.
    table: Vec<String>,
    /// Terminal width from the last draw; tables are capped to it.
    width: usize,
}

impl MarkdownStream {
    pub(super) fn set_width(&mut self, width: usize) {
        self.width = width;
    }
    /// Append streamed text, returning the display lines now final.
    pub(super) fn push(&mut self, delta: &str) -> Vec<Line<'static>> {
        self.buf.push_str(delta);
        let mut out = Vec::new();
        while let Some(end) = self.buf.find('\n') {
            let line: String = self.buf.drain(..=end).collect();
            self.render_line(line.trim_end_matches('\n'), &mut out);
        }
        out
    }

    /// Emit everything still held: an open table and the partial last line.
    pub(super) fn flush(&mut self) -> Vec<Line<'static>> {
        let mut out = Vec::new();
        let rest = std::mem::take(&mut self.buf);
        if !rest.trim().is_empty() {
            self.render_line(rest.trim_end(), &mut out);
        }
        self.end_table(&mut out);
        self.in_fence = false;
        out
    }

    pub(super) fn is_empty(&self) -> bool {
        self.buf.is_empty() && self.table.is_empty()
    }

    fn render_line(&mut self, src: &str, out: &mut Vec<Line<'static>>) {
        let trimmed = src.trim_start();

        if trimmed.starts_with("```") && !self.table_open() {
            self.in_fence = !self.in_fence;
            return;
        }
        if self.in_fence {
            self.end_table(out);
            let mut spans = vec![Span::styled("│ ", theme::get().faint_style())];
            spans.extend(super::syntax::highlight(src));
            out.push(Line::from(spans));
            return;
        }

        // Table rows buffer until the first non-`|` line closes the table.
        if trimmed.starts_with('|') {
            self.table.push(src.to_string());
            return;
        }
        self.end_table(out);

        if trimmed.is_empty() {
            out.push(Line::from(""));
            return;
        }
        if let Some(rest) = trimmed
            .strip_prefix("> ")
            .or_else(|| (trimmed == ">").then_some(""))
        {
            let mut spans = vec![Span::styled("│ ", dim())];
            spans.extend(inline(rest, dim()));
            out.push(Line::from(spans));
            return;
        }
        if trimmed.len() >= 3
            && (trimmed.chars().all(|c| c == '-')
                || trimmed.chars().all(|c| c == '*')
                || trimmed.chars().all(|c| c == '_'))
        {
            out.push(Line::from(Span::styled("─".repeat(24), dim())));
            return;
        }

        let hashes = trimmed.chars().take_while(|c| *c == '#').count();
        if (1..=6).contains(&hashes) && trimmed[hashes..].starts_with(' ') {
            let style = Style::default().add_modifier(Modifier::BOLD);
            out.push(Line::from(inline(trimmed[hashes + 1..].trim(), style)));
            return;
        }

        let indent = " ".repeat(src.len() - trimmed.len());
        if let Some(rest) = ["- ", "* ", "+ "]
            .iter()
            .find_map(|m| trimmed.strip_prefix(m))
        {
            let (marker, rest) = checkbox(rest);
            let mut spans = vec![
                Span::raw(indent),
                Span::styled(marker, theme::get().dimmer_style()),
            ];
            spans.extend(inline(rest, Style::default()));
            out.push(Line::from(spans));
            return;
        }
        if let Some((marker, rest)) = ordered(trimmed) {
            let mut spans = vec![
                Span::raw(indent),
                Span::styled(marker, theme::get().dimmer_style()),
            ];
            spans.extend(inline(rest, Style::default()));
            out.push(Line::from(spans));
            return;
        }

        let mut spans = vec![Span::raw(indent)];
        spans.extend(inline(trimmed, Style::default()));
        out.push(Line::from(spans));
    }

    fn table_open(&self) -> bool {
        !self.table.is_empty()
    }

    fn end_table(&mut self, out: &mut Vec<Line<'static>>) {
        if !self.table.is_empty() {
            let rows = std::mem::take(&mut self.table);
            out.extend(render_table(&rows, self.width));
        }
    }
}

/// Render a whole markdown string at once, for text that never streamed.
pub(super) fn render(text: &str) -> Vec<Line<'static>> {
    let mut stream = MarkdownStream::default();
    let mut out = stream.push(text);
    if !text.ends_with('\n') || !stream.is_empty() {
        out.extend(stream.flush());
    }
    out
}

fn checkbox(rest: &str) -> (String, &str) {
    if let Some(r) = rest.strip_prefix("[ ] ") {
        ("☐ ".into(), r)
    } else if let Some(r) = rest
        .strip_prefix("[x] ")
        .or_else(|| rest.strip_prefix("[X] "))
    {
        ("☑ ".into(), r)
    } else {
        ("• ".into(), rest)
    }
}

fn ordered(line: &str) -> Option<(String, &str)> {
    let digits = line.chars().take_while(char::is_ascii_digit).count();
    if digits == 0 || digits > 3 {
        return None;
    }
    let rest = line[digits..].strip_prefix(". ")?;
    Some((format!("{}. ", &line[..digits]), rest))
}

/// Inline markup on one line via pulldown-cmark: emphasis, code, links.
fn inline(text: &str, base: Style) -> Vec<Span<'static>> {
    let mut out = Vec::new();
    let mut style = base;
    let mut bold = 0u32;
    let mut italic = 0u32;
    let mut strike = 0u32;
    let mut link: Option<String> = None;

    let apply = |bold: u32, italic: u32, strike: u32, base: Style| {
        let mut s = base;
        if bold > 0 {
            s = s.add_modifier(Modifier::BOLD);
        }
        if italic > 0 {
            s = s.add_modifier(Modifier::ITALIC);
        }
        if strike > 0 {
            s = s.add_modifier(Modifier::CROSSED_OUT);
        }
        s
    };

    for event in Parser::new_ext(text, Options::ENABLE_STRIKETHROUGH) {
        match event {
            Event::Text(t) => out.push(Span::styled(t.into_string(), style)),
            Event::Code(t) => out.push(Span::styled(format!(" {t} "), code_chip())),
            Event::Start(Tag::Strong) => {
                bold += 1;
                style = apply(bold, italic, strike, base);
            }
            Event::End(TagEnd::Strong) => {
                bold = bold.saturating_sub(1);
                style = apply(bold, italic, strike, base);
            }
            Event::Start(Tag::Emphasis) => {
                italic += 1;
                style = apply(bold, italic, strike, base);
            }
            Event::End(TagEnd::Emphasis) => {
                italic = italic.saturating_sub(1);
                style = apply(bold, italic, strike, base);
            }
            Event::Start(Tag::Strikethrough) => {
                strike += 1;
                style = apply(bold, italic, strike, base);
            }
            Event::End(TagEnd::Strikethrough) => {
                strike = strike.saturating_sub(1);
                style = apply(bold, italic, strike, base);
            }
            Event::Start(Tag::Link { dest_url, .. }) => link = Some(dest_url.into_string()),
            Event::End(TagEnd::Link) => {
                if let Some(url) = link.take() {
                    out.push(Span::styled(format!(" ({url})"), dim()));
                }
            }
            Event::SoftBreak | Event::HardBreak => out.push(Span::raw(" ")),
            _ => {}
        }
    }
    if out.is_empty() {
        out.push(Span::styled(text.to_string(), base));
    }
    out
}

/// Render buffered `|`-rows as an aligned table; pulldown-cmark validates the
/// header, and a malformed table falls back to plain rows. Cells go through
/// `inline` so markup renders, and the widest columns shrink with a `…` until
/// the whole table fits `max` columns instead of wrapping mid-rule.
fn render_table(rows: &[String], max: usize) -> Vec<Line<'static>> {
    let cells: Vec<Vec<String>> = rows
        .iter()
        .map(|r| {
            r.trim()
                .trim_matches('|')
                .split('|')
                .map(|c| c.trim().to_string())
                .collect()
        })
        .collect();
    let is_delim = |row: &[String]| {
        !row.is_empty()
            && row
                .iter()
                .all(|c| !c.is_empty() && c.chars().all(|ch| matches!(ch, '-' | ':')))
    };
    if cells.len() < 2 || !is_delim(&cells[1]) {
        return rows
            .iter()
            .map(|r| Line::from(Span::raw(r.clone())))
            .collect();
    }

    let columns = cells.iter().map(Vec::len).max().unwrap_or(0);
    let header_style = Style::default().add_modifier(Modifier::BOLD);
    let rendered: Vec<Vec<(Vec<Span<'static>>, usize)>> = cells
        .iter()
        .map(|row| {
            row.iter()
                .enumerate()
                .map(|(i, cell)| {
                    let style = if i == 0 {
                        header_style
                    } else {
                        Style::default()
                    };
                    let spans = inline(cell, style);
                    let w = spans
                        .iter()
                        .map(|s| UnicodeWidthStr::width(s.content.as_ref()))
                        .sum();
                    (spans, w)
                })
                .collect()
        })
        .collect();

    let mut widths = vec![0usize; columns];
    for (i, row) in rendered.iter().enumerate() {
        if is_delim(&cells[i]) {
            continue;
        }
        for (c, (_, w)) in row.iter().enumerate() {
            widths[c] = widths[c].max(*w);
        }
    }

    // Shrink the widest columns until the table fits the terminal, so the
    // rule line never wraps into stray fragments.
    let total =
        |widths: &[usize]| widths.iter().sum::<usize>() + 3 * widths.len().saturating_sub(1);
    if max > 0 {
        while total(&widths) > max {
            let (i, _) = widths
                .iter()
                .enumerate()
                .max_by_key(|(_, w)| **w)
                .unwrap_or((0, &0));
            if widths[i] <= 4 {
                break;
            }
            widths[i] -= 1;
        }
    }

    let mut out = Vec::new();
    for (i, row) in rendered.iter().enumerate() {
        if is_delim(&cells[i]) {
            let rule: Vec<String> = widths.iter().map(|w| "─".repeat(*w)).collect();
            out.push(Line::from(Span::styled(rule.join("─┼─"), dim())));
            continue;
        }
        let mut spans = Vec::new();
        for (c, width) in widths.iter().enumerate() {
            if c > 0 {
                spans.push(Span::styled(" │ ", dim()));
            }
            let (cell_spans, cell_w) = row.get(c).cloned().unwrap_or_default();
            let (cell_spans, cell_w) = if cell_w > *width {
                (truncate_spans(cell_spans, *width), *width)
            } else {
                (cell_spans, cell_w)
            };
            spans.extend(cell_spans);
            spans.push(Span::raw(" ".repeat(width.saturating_sub(cell_w))));
        }
        out.push(Line::from(spans));
    }
    out
}

/// Cut styled spans down to at most `max` columns, ending with `…` when cut.
fn truncate_spans(spans: Vec<Span<'static>>, max: usize) -> Vec<Span<'static>> {
    let mut out = Vec::new();
    let mut used = 0usize;
    let mut cut = false;
    let budget = max.saturating_sub(1);
    for span in spans {
        if used >= budget {
            cut = true;
            break;
        }
        let w = UnicodeWidthStr::width(span.content.as_ref());
        if used + w <= budget {
            used += w;
            out.push(span);
        } else {
            let keep = budget - used;
            let mut taken = 0usize;
            let mut end = 0usize;
            for (off, ch) in span.content.char_indices() {
                let cw = UnicodeWidthChar::width(ch).unwrap_or(0);
                if taken + cw > keep {
                    break;
                }
                taken += cw;
                end = off + ch.len_utf8();
            }
            if end > 0 {
                out.push(Span::styled(span.content[..end].to_string(), span.style));
            }
            cut = true;
            break;
        }
    }
    if cut && !out.is_empty() {
        out.push(Span::styled("…", out.last().unwrap().style));
    }
    out
}

#[cfg(test)]
#[path = "tests/markdown_test.rs"]
mod tests;
