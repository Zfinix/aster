//! Streaming markdown: block structure is tracked per line so committed
//! output never changes, while inline markup and tables go through
//! pulldown-cmark. `push` emits lines as they complete; `flush` drains.

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use super::theme;

fn dim() -> Style {
    theme::dim()
}

/// Inline code: a shade brighter than body text on a quiet raised band. Colour
/// is reserved for accents; identifiers appear too often to carry one.
fn code_chip() -> Style {
    Style::default()
        .fg(Color::Rgb(0xdd, 0xdd, 0xd8))
        .bg(Color::Rgb(0x20, 0x20, 0x20))
}

#[derive(Default)]
pub(super) struct MarkdownStream {
    buf: String,
    in_fence: bool,
    /// Rows of an in-flight table, held until the table ends.
    table: Vec<String>,
}

impl MarkdownStream {
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
            let mut spans = vec![Span::styled("│ ", theme::faint())];
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
            let mut spans = vec![Span::raw(indent), Span::styled(marker, theme::dimmer())];
            spans.extend(inline(rest, Style::default()));
            out.push(Line::from(spans));
            return;
        }
        if let Some((marker, rest)) = ordered(trimmed) {
            let mut spans = vec![Span::raw(indent), Span::styled(marker, theme::dimmer())];
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
            out.extend(render_table(&rows));
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
/// header, and a malformed table falls back to plain rows.
fn render_table(rows: &[String]) -> Vec<Line<'static>> {
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
    let mut widths = vec![0usize; columns];
    for row in cells.iter().filter(|r| !is_delim(r)) {
        for (i, cell) in row.iter().enumerate() {
            widths[i] = widths[i].max(UnicodeWidthStr::width(cell.as_str()));
        }
    }

    let mut out = Vec::new();
    for (i, row) in cells.iter().enumerate() {
        if is_delim(row) {
            let rule: Vec<String> = widths.iter().map(|w| "─".repeat(*w)).collect();
            out.push(Line::from(Span::styled(rule.join("─┼─"), dim())));
            continue;
        }
        let style = if i == 0 {
            Style::default().add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        let mut spans = Vec::new();
        for (c, width) in widths.iter().enumerate() {
            if c > 0 {
                spans.push(Span::styled(" │ ", dim()));
            }
            let text = row.get(c).cloned().unwrap_or_default();
            let pad = width.saturating_sub(UnicodeWidthStr::width(text.as_str()));
            spans.push(Span::styled(format!("{text}{}", " ".repeat(pad)), style));
        }
        out.push(Line::from(spans));
    }
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
    fn lines_are_emitted_only_once_complete() {
        let mut md = MarkdownStream::default();
        assert!(md.push("# Hea").is_empty());
        assert!(md.push("ding").is_empty());
        assert_eq!(text_of(&md.push("\n")), ["Heading"]);
    }

    #[test]
    fn fences_render_as_code_and_hide_the_markers() {
        let out = render("```rust\nlet a = 1;\n```\n");
        assert_eq!(text_of(&out), ["│ let a = 1;"]);
    }

    #[test]
    fn bullets_and_inline_marks_are_styled() {
        let out = render("- run `cargo check` **now**\n");
        assert_eq!(text_of(&out), ["• run  cargo check  now"]);
        assert!(
            out[0]
                .spans
                .iter()
                .any(|s| s.style.add_modifier.contains(Modifier::BOLD))
        );
    }

    #[test]
    fn a_lone_asterisk_stays_literal() {
        assert_eq!(text_of(&render("2 * 3 = 6\n")), ["2 * 3 = 6"]);
    }

    #[test]
    fn flush_emits_a_trailing_partial_line() {
        let mut md = MarkdownStream::default();
        md.push("no newline here");
        assert_eq!(text_of(&md.flush()), ["no newline here"]);
    }

    #[test]
    fn links_show_text_then_dim_url() {
        let out = render("see [the docs](https://a.dev)\n");
        assert_eq!(text_of(&out), ["see the docs (https://a.dev)"]);
    }

    #[test]
    fn tables_align_and_emit_only_when_closed() {
        let mut md = MarkdownStream::default();
        assert!(md.push("| a | long |\n|---|---|\n| x | y |\n").is_empty());
        let out = md.push("done\n");
        let rows = text_of(&out);
        assert_eq!(rows[0], "a │ long");
        assert!(rows[1].contains("┼"));
        assert_eq!(rows[2], "x │ y");
        assert_eq!(rows[3], "done");
    }

    #[test]
    fn an_unclosed_table_flushes_at_end_of_turn() {
        let mut md = MarkdownStream::default();
        md.push("| a | b |\n|---|---|\n| 1 | 2 |\n");
        assert!(!md.is_empty());
        assert_eq!(text_of(&md.flush()).len(), 3);
        assert!(md.is_empty());
    }

    #[test]
    fn checkboxes_render_as_glyphs() {
        let out = render("- [x] done\n- [ ] todo\n");
        assert_eq!(text_of(&out), ["☑ done", "☐ todo"]);
    }

    #[test]
    fn strikethrough_is_crossed_out() {
        let out = render("~~gone~~\n");
        assert!(
            out[0]
                .spans
                .iter()
                .any(|s| s.style.add_modifier.contains(Modifier::CROSSED_OUT))
        );
    }
}
