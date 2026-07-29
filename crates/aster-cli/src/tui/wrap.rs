//! Unicode-aware wrapping. Everything the chat TUI puts on screen goes through
//! here, so the composer's cursor math and the transcript's line count agree
//! with what the terminal actually draws (CJK and emoji are two columns wide).

use std::ops::Range;

use ratatui::text::{Line, Span};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Display columns `text` occupies.
pub(super) fn width(text: &str) -> usize {
    UnicodeWidthStr::width(text)
}

/// Split `text` into display rows of at most `max` columns, as byte ranges that
/// tile the input. Breaks after spaces; a word wider than a row is cut. The
/// ranges tile so a byte offset (a cursor) maps to exactly one row.
pub(super) fn rows(text: &str, max: usize) -> Vec<Range<usize>> {
    let max = max.max(1);
    let mut out = Vec::new();
    let mut start = 0;
    let mut col = 0;
    let mut pos = 0;
    for chunk in text.split_inclusive(' ') {
        let chunk_start = pos;
        pos += chunk.len();
        let word = chunk.trim_end_matches(' ');
        let word_w = width(word);
        if col > 0 && col + word_w > max {
            out.push(start..chunk_start);
            start = chunk_start;
            col = 0;
        }
        if word_w > max {
            let mut w = 0;
            for (off, ch) in word.char_indices() {
                let cw = UnicodeWidthChar::width(ch).unwrap_or(0);
                if w + cw > max && chunk_start + off > start {
                    out.push(start..chunk_start + off);
                    start = chunk_start + off;
                    w = 0;
                }
                w += cw;
            }
            col = w + (chunk.len() - word.len());
        } else {
            col += width(chunk);
        }
    }
    out.push(start..text.len());
    out
}

/// `text` as display rows, trailing spaces dropped.
pub(super) fn lines(text: &str, max: usize) -> Vec<String> {
    rows(text, max)
        .into_iter()
        .map(|r| text[r].trim_end().to_string())
        .collect()
}

/// Re-flow a styled line to `max` columns, carrying each span's style across
/// the break.
pub(super) fn wrap_line(line: Line<'static>, max: usize) -> Vec<Line<'static>> {
    let mut text = String::new();
    let mut runs = Vec::with_capacity(line.spans.len());
    for span in &line.spans {
        let start = text.len();
        text.push_str(&span.content);
        runs.push((start..text.len(), span.style));
    }

    let ranges = rows(&text, max);
    if ranges.len() <= 1 {
        return vec![line];
    }
    ranges
        .into_iter()
        .map(|row| {
            let spans: Vec<Span<'static>> = runs
                .iter()
                .filter_map(|(run, style)| {
                    let from = run.start.max(row.start);
                    let to = run.end.min(row.end);
                    (from < to).then(|| Span::styled(text[from..to].to_string(), *style))
                })
                .collect();
            Line::from(spans).style(line.style)
        })
        .collect()
}

/// Pad `line` with spaces so its background colour reaches the full width.
pub(super) fn pad_to(
    mut line: Line<'static>,
    max: usize,
    style: ratatui::style::Style,
) -> Line<'static> {
    let used = line.spans.iter().map(|s| width(&s.content)).sum::<usize>();
    if used < max {
        line.spans.push(Span::styled(" ".repeat(max - used), style));
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_tile_the_whole_input() {
        let text = "the quick brown fox jumps";
        let ranges = rows(text, 10);
        assert_eq!(ranges.first().unwrap().start, 0);
        assert_eq!(ranges.last().unwrap().end, text.len());
        for pair in ranges.windows(2) {
            assert_eq!(pair[0].end, pair[1].start);
        }
    }

    #[test]
    fn rows_never_exceed_the_width() {
        for row in lines("the quick brown fox jumps over the lazy dog", 12) {
            assert!(width(&row) <= 12, "{row:?}");
        }
    }

    #[test]
    fn oversized_words_are_cut() {
        let rows = lines("supercalifragilistic", 6);
        assert_eq!(rows, ["superc", "alifra", "gilist", "ic"]);
    }

    #[test]
    fn wide_glyphs_count_as_two_columns() {
        assert_eq!(width("日本語"), 6);
        for row in lines("日本語のテキスト", 6) {
            assert!(width(&row) <= 6);
        }
    }

    #[test]
    fn empty_input_is_one_empty_row() {
        assert_eq!(lines("", 10), [""]);
    }
}
