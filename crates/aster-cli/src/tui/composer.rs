//! The message editor: a multi-line text area with word motion, prompt recall and
//! paste folding. It owns no drawing; the chat loop asks it for display rows and
//! the cursor's position within them.

use std::ops::Range;

use ratatui::style::Style;
use ratatui::text::{Line, Span};

use crate::tui::theme;
use crate::tui::wrap;

/// A paste longer than this is folded into a placeholder so it cannot swallow
/// the screen; the real text is restored on send.
const FOLD_PASTE_OVER: usize = 240;
/// A path token this long is folded into a `[@name]` reference so the composer
/// and transcript stay readable. Short paths pass through untouched.
const FOLD_PATH_MIN_LEN: usize = 28;
/// Longest name shown inside a `[@name]` token; longer basenames are truncated
/// with an ellipsis. The full path always survives in the reference block.
const MAX_TOKEN_CHARS: usize = 40;

#[derive(Default)]
pub(super) struct Composer {
    text: String,
    /// Byte offset into `text`, always on a char boundary.
    cursor: usize,
    /// Previously sent messages, oldest first.
    sent: Vec<String>,
    /// Position in `sent` while recalling; `None` means editing a fresh draft.
    recall: Option<usize>,
    /// The draft set aside while recalling, restored on the way back down.
    stash: String,
    /// Placeholder text mapped to the full paste it stands for.
    folded: Vec<(String, String)>,
    /// `[@name]` token mapped to the full cleaned path it stands for. The
    /// tokens stay in the sent text; the paths ride along as a reference block.
    refs: Vec<(String, String)>,
}

impl Composer {
    pub(super) fn text(&self) -> &str {
        &self.text
    }

    pub(super) fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    pub(super) fn insert(&mut self, c: char) {
        self.text.insert(self.cursor, c);
        self.cursor += c.len_utf8();
        self.recall = None;
    }

    pub(super) fn insert_str(&mut self, s: &str) {
        self.text.insert_str(self.cursor, s);
        self.cursor += s.len();
        self.recall = None;
    }

    /// Fold a bracketed paste that is too big to read in the composer.
    pub(super) fn paste(&mut self, text: &str) {
        if text.len() <= FOLD_PASTE_OVER {
            self.insert_str(&text.replace("\r\n", "\n").replace('\r', "\n"));
            return;
        }
        let placeholder = format!("[pasted {} lines]", text.lines().count().max(1));
        self.folded.push((placeholder.clone(), text.to_string()));
        self.insert_str(&placeholder);
    }

    pub(super) fn backspace(&mut self) {
        if let Some(prev) = self.prev_boundary(self.cursor) {
            self.text.replace_range(prev..self.cursor, "");
            self.cursor = prev;
        }
    }

    pub(super) fn delete(&mut self) {
        if let Some(next) = self.next_boundary(self.cursor) {
            self.text.replace_range(self.cursor..next, "");
        }
    }

    pub(super) fn delete_word_back(&mut self) {
        let start = self.word_start();
        self.text.replace_range(start..self.cursor, "");
        self.cursor = start;
    }

    pub(super) fn kill_to_start(&mut self) {
        let start = self.row_bounds().start;
        self.text.replace_range(start..self.cursor, "");
        self.cursor = start;
    }

    pub(super) fn kill_to_end(&mut self) {
        let end = self.row_bounds().end;
        self.text.replace_range(self.cursor..end, "");
    }

    pub(super) fn left(&mut self) {
        if let Some(prev) = self.prev_boundary(self.cursor) {
            self.cursor = prev;
        }
    }

    pub(super) fn right(&mut self) {
        if let Some(next) = self.next_boundary(self.cursor) {
            self.cursor = next;
        }
    }

    pub(super) fn word_left(&mut self) {
        self.cursor = self.word_start();
    }

    pub(super) fn word_right(&mut self) {
        let rest = &self.text[self.cursor..];
        let skipped: usize = rest
            .char_indices()
            .skip_while(|(_, c)| c.is_whitespace())
            .find(|(_, c)| c.is_whitespace())
            .map(|(i, _)| i)
            .unwrap_or(rest.len());
        self.cursor += skipped;
    }

    pub(super) fn home(&mut self) {
        self.cursor = self.row_bounds().start;
    }

    pub(super) fn end(&mut self) {
        self.cursor = self.row_bounds().end;
    }

    /// Move up a display row. `false` means the cursor was already on the first
    /// row, which the caller turns into a prompt-history step.
    pub(super) fn up(&mut self, width: u16) -> bool {
        self.step_row(width, -1)
    }

    pub(super) fn down(&mut self, width: u16) -> bool {
        self.step_row(width, 1)
    }

    fn step_row(&mut self, width: u16, delta: isize) -> bool {
        let rows = self.rows(width);
        let (row, col) = self.position(&rows);
        let Some(target) = row.checked_add_signed(delta).filter(|r| *r < rows.len()) else {
            return false;
        };
        let range = rows[target].clone();
        let text = &self.text[range.clone()];
        let mut offset = text.len();
        let mut seen = 0;
        for (i, ch) in text.char_indices() {
            if seen >= col {
                offset = i;
                break;
            }
            seen += wrap::width(ch.encode_utf8(&mut [0u8; 4]));
        }
        self.cursor = range.start + offset;
        true
    }

    pub(super) fn recall_prev(&mut self) {
        if self.sent.is_empty() {
            return;
        }
        let next = match self.recall {
            None => {
                self.stash = std::mem::take(&mut self.text);
                self.sent.len() - 1
            }
            Some(0) => 0,
            Some(i) => i - 1,
        };
        self.recall = Some(next);
        self.text = self.sent[next].clone();
        self.cursor = self.text.len();
    }

    pub(super) fn recall_next(&mut self) {
        let Some(i) = self.recall else { return };
        if i + 1 < self.sent.len() {
            self.recall = Some(i + 1);
            self.text = self.sent[i + 1].clone();
        } else {
            self.recall = None;
            self.text = std::mem::take(&mut self.stash);
        }
        self.cursor = self.text.len();
    }

    /// Hand the draft over for sending: folded pastes are expanded and the raw draft
    /// remembered for recall. Long paths stay folded into `[@name]` tokens; see
    /// [`Self::take_refs`] for the paths they stand for.
    pub(super) fn take(&mut self) -> String {
        let draft = std::mem::take(&mut self.text);
        self.cursor = 0;
        self.recall = None;
        self.stash.clear();
        if self.sent.last() != Some(&draft) {
            self.sent.push(draft.clone());
        }
        let expanded = self
            .folded
            .drain(..)
            .fold(draft, |acc, (mark, full)| acc.replace(&mark, &full));
        self.fold_paths(&expanded)
    }

    /// Drain the `[@name]` → full-path references collected by folding, to be
    /// attached to the message as a resolvable block.
    pub(super) fn take_refs(&mut self) -> Vec<(String, String)> {
        std::mem::take(&mut self.refs)
    }

    pub(super) fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
        self.recall = None;
        self.folded.clear();
        self.refs.clear();
    }

    /// Replace path-like tokens with `[@name]` placeholders, recording each token's
    /// full path in [`Self::refs`]. Tokens may contain shell-escaped spaces, so a
    /// space ends one only when no backslash precedes it.
    fn fold_paths(&mut self, text: &str) -> String {
        let mut out = String::with_capacity(text.len());
        let bytes = text.as_bytes();
        let mut start = 0;
        let mut i = 0;
        while i < text.len() {
            let ch = text[i..].chars().next().expect("i on a char boundary");
            if ch.is_whitespace() {
                // A backslash-escaped space is part of the same path token.
                if i > 0 && bytes[i - 1] == b'\\' {
                    i += ch.len_utf8();
                    continue;
                }
                self.fold_token(&text[start..i], &mut out);
                out.push(ch);
                i += ch.len_utf8();
                start = i;
            } else {
                i += ch.len_utf8();
            }
        }
        self.fold_token(&text[start..], &mut out);
        out
    }

    fn fold_token(&mut self, token: &str, out: &mut String) {
        let Some(cleaned) = clean_path(token) else {
            out.push_str(token);
            return;
        };
        if cleaned.len() < FOLD_PATH_MIN_LEN && !token.contains('\\') {
            out.push_str(token);
            return;
        }
        let mark = format!("[@{}]", short_name(&cleaned));
        if !self.refs.iter().any(|(t, _)| t == &mark) {
            self.refs.push((mark.clone(), cleaned));
        }
        out.push_str(&mark);
    }

    /// Columns available to the text, after the `❯ ` prompt.
    pub(super) fn text_width(width: u16) -> usize {
        (width as usize).saturating_sub(2).max(8)
    }

    fn rows(&self, width: u16) -> Vec<Range<usize>> {
        let inner = Self::text_width(width);
        let mut out = Vec::new();
        let mut base = 0;
        for segment in self.text.split('\n') {
            for row in wrap::rows(segment, inner) {
                out.push(base + row.start..base + row.end);
            }
            // Step past the newline; it belongs to the row it terminates.
            base += segment.len() + 1;
        }
        out
    }

    /// The cursor's `(row, column)` among `rows`.
    fn position(&self, rows: &[Range<usize>]) -> (usize, usize) {
        let idx = rows
            .iter()
            .position(|r| self.cursor >= r.start && self.cursor <= r.end)
            .unwrap_or(rows.len().saturating_sub(1));
        let row = &rows[idx.min(rows.len().saturating_sub(1))];
        let col = wrap::width(&self.text[row.start..self.cursor.max(row.start).min(row.end)]);
        (idx, col)
    }

    /// The line the cursor sits on, as byte offsets into `text`.
    fn row_bounds(&self) -> Range<usize> {
        let start = self.text[..self.cursor]
            .rfind('\n')
            .map(|i| i + 1)
            .unwrap_or(0);
        let end = self.text[self.cursor..]
            .find('\n')
            .map(|i| self.cursor + i)
            .unwrap_or(self.text.len());
        start..end
    }

    fn word_start(&self) -> usize {
        let head = &self.text[..self.cursor];
        let trimmed = head.trim_end();
        match trimmed.rfind(char::is_whitespace) {
            Some(i) => i + 1,
            None => 0,
        }
    }

    fn prev_boundary(&self, at: usize) -> Option<usize> {
        (at > 0).then(|| {
            self.text[..at]
                .char_indices()
                .next_back()
                .map(|(i, _)| i)
                .unwrap_or(0)
        })
    }

    fn next_boundary(&self, at: usize) -> Option<usize> {
        self.text[at..].chars().next().map(|c| at + c.len_utf8())
    }

    /// Display rows the draft occupies, capped so it never eats the screen.
    pub(super) fn height(&self, width: u16) -> u16 {
        (self.rows(width).len() as u16).clamp(1, 8)
    }

    /// If the cursor sits right after an `@` with no intervening whitespace,
    /// return the byte position of the `@` and the query text after it (may be
    /// empty).  Returns `None` when the cursor is not inside a mention context.
    pub(super) fn mention_context(&self) -> Option<(usize, &str)> {
        let head = &self.text[..self.cursor];
        let at = head.rfind('@')?;
        // Must be at the start or preceded by a space.
        if at > 0 && !head.as_bytes()[at - 1].is_ascii_whitespace() {
            return None;
        }
        let query = &self.text[at + 1..self.cursor];
        if query.contains(char::is_whitespace) {
            return None;
        }
        Some((at, query))
    }

    /// Replace the mention at `start` (the `@` byte offset) through the cursor
    /// with `@path ` and park the cursor after the trailing space.
    pub(super) fn complete_mention(&mut self, start: usize, path: &str) {
        self.text
            .replace_range(start..self.cursor, &format!("@{path} "));
        self.cursor = start + path.len() + 2;
        self.recall = None;
    }

    /// Delete from the `@` at `start` through the cursor, canceling the mention.
    pub(super) fn cancel_mention(&mut self, start: usize) {
        self.text.replace_range(start..self.cursor, "");
        self.cursor = start;
    }

    /// The draft as styled rows, plus the cursor's `(row, column)` inside them.
    pub(super) fn render(&self, width: u16, placeholder: &str) -> (Vec<Line<'static>>, (u16, u16)) {
        if self.text.is_empty() {
            let line = Line::from(vec![
                prompt_span(true),
                Span::styled(
                    placeholder.to_string(),
                    Style::default().fg(theme::get().placeholder),
                ),
            ]);
            return (vec![line], (0, 2));
        }

        let rows = self.rows(width);
        let (cursor_row, cursor_col) = self.position(&rows);
        let visible = self.height(width) as usize;
        // Keep the caret on screen when the draft outgrows the box.
        let top = cursor_row.saturating_sub(visible.saturating_sub(1));

        let lines = rows
            .iter()
            .enumerate()
            .skip(top)
            .take(visible)
            .map(|(i, row)| {
                Line::from(vec![
                    prompt_span(i == 0),
                    Span::raw(self.text[row.clone()].trim_end_matches('\n').to_string()),
                ])
            })
            .collect();

        (lines, ((cursor_row - top) as u16, (cursor_col + 2) as u16))
    }
}

/// `token` with shell escapes removed when it looks like a path, else `None`. It
/// must contain a separator and be absolute, relative (`./`, `../`, `~/`), a
/// Windows drive, or end in a short file extension.
fn clean_path(token: &str) -> Option<String> {
    let has_sep = token.contains('/') || token.contains('\\');
    if !has_sep {
        return None;
    }
    let b = token.as_bytes();
    let looks_like_path = token.starts_with('/')
        || token.starts_with("./")
        || token.starts_with("../")
        || token.starts_with('~')
        || (b.len() >= 3 && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/'))
        || file_extension(token);
    looks_like_path.then(|| unescape(token))
}

/// Drop a backslash that escapes a following space (`\ ` → space). Windows
/// separator backslashes (`C:\Users`) are left intact.
fn unescape(token: &str) -> String {
    let mut out = String::with_capacity(token.len());
    let mut i = 0;
    while i < token.len() {
        let ch = token[i..].chars().next().expect("i on a char boundary");
        if ch == '\\' {
            let next = token[i + ch.len_utf8()..].chars().next();
            if matches!(next, Some(c) if c.is_whitespace()) {
                i += ch.len_utf8();
                continue;
            }
        }
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// The basename of a path, truncated with an ellipsis (extension preserved)
/// when it would overflow a `[@name]` token.
fn short_name(cleaned: &str) -> String {
    let base = cleaned.rsplit(['/', '\\']).next().unwrap_or(cleaned);
    let chars: Vec<char> = base.chars().collect();
    if chars.len() <= MAX_TOKEN_CHARS {
        return base.to_string();
    }
    let ext: Vec<char> = match base.rsplit_once('.') {
        Some((_, e)) if !e.is_empty() && e.chars().count() <= 8 => {
            std::iter::once('.').chain(e.chars()).collect()
        }
        _ => Vec::new(),
    };
    let head = MAX_TOKEN_CHARS.saturating_sub(ext.len() + 1);
    let mut out: String = chars[..head].iter().collect();
    out.push('…');
    out.extend(ext);
    out
}

/// True when the final path segment ends in a short extension like `.mov` or
/// `.rs`, which is a strong path signal even without a leading separator hint.
fn file_extension(token: &str) -> bool {
    let base = token.rsplit(['/', '\\']).next().unwrap_or(token);
    matches!(
        base.rsplit_once('.'),
        Some((name, ext)) if !name.is_empty() && !ext.is_empty() && ext.chars().count() <= 8
    )
}

fn prompt_span(first: bool) -> Span<'static> {
    if first {
        Span::styled("❯ ", theme::get().accent_bold())
    } else {
        Span::raw("  ")
    }
}

#[cfg(test)]
#[path = "tests/composer_test.rs"]
mod tests;
