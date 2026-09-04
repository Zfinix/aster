//! A small, language-agnostic highlighter for code shown in the transcript. One
//! tokenizer splits on strings, comments, numbers, and words, then colours words by
//! a shared keyword set: cheaper than a grammar, right for text seen once.

use ratatui::style::Style;
use ratatui::text::Span;

use super::theme;

const KEYWORDS: &[&str] = &[
    "as",
    "async",
    "await",
    "break",
    "case",
    "class",
    "const",
    "continue",
    "def",
    "default",
    "defer",
    "do",
    "elif",
    "else",
    "end",
    "enum",
    "esac",
    "export",
    "extern",
    "fi",
    "final",
    "finally",
    "fn",
    "for",
    "from",
    "func",
    "go",
    "if",
    "impl",
    "import",
    "in",
    "interface",
    "let",
    "loop",
    "match",
    "mod",
    "move",
    "mut",
    "namespace",
    "new",
    "package",
    "pub",
    "public",
    "ref",
    "return",
    "select",
    "self",
    "static",
    "struct",
    "super",
    "switch",
    "then",
    "trait",
    "try",
    "type",
    "typedef",
    "union",
    "unsafe",
    "use",
    "var",
    "where",
    "while",
    "with",
    "yield",
];

const LITERALS: &[&str] = &[
    "true",
    "false",
    "null",
    "nil",
    "None",
    "True",
    "False",
    "undefined",
    "Some",
    "Ok",
    "Err",
];

const LINE_COMMENTS: &[&str] = &["//", "#", "--"];

/// Split `src` into styled spans. `src` is one display line; multi-line
/// constructs are not tracked, so a block comment only tints its own row.
pub(super) fn highlight(src: &str) -> Vec<Span<'static>> {
    let mut out = Vec::new();
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        let rest: String = chars[i..].iter().collect();

        if let Some(open) = LINE_COMMENTS.iter().find(|c| rest.starts_with(**c)) {
            // `#` opens a comment only outside a word, so `a#b` and CSS hex
            // colours are not swallowed.
            let standalone = *open != "#" || i == 0 || !chars[i - 1].is_alphanumeric();
            if standalone {
                out.push(Span::styled(rest, theme::get().faint_style()));
                return out;
            }
        }

        let c = chars[i];
        if c.is_whitespace() {
            let start = i;
            while i < chars.len() && chars[i].is_whitespace() {
                i += 1;
            }
            out.push(Span::raw(chars[start..i].iter().collect::<String>()));
            continue;
        }
        if c == '"' || c == '\'' || c == '`' {
            let (span, next) = string_from(&chars, i, c);
            out.push(span);
            i = next;
            continue;
        }
        if c.is_ascii_digit() {
            let start = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '.') {
                i += 1;
            }
            out.push(Span::styled(
                chars[start..i].iter().collect::<String>(),
                Style::default().fg(theme::get().amber),
            ));
            continue;
        }
        if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            let called = chars.get(i) == Some(&'(');
            let style = word_style(&word, called, start == 0 || is_leading(&chars[..start]));
            out.push(Span::styled(word, style));
            continue;
        }

        let start = i;
        while i < chars.len() && is_punct(chars[i]) {
            i += 1;
        }
        if i == start {
            i += 1;
        }
        out.push(Span::styled(
            chars[start..i].iter().collect::<String>(),
            theme::get().dimmer_style(),
        ));
    }
    out
}

fn string_from(chars: &[char], start: usize, quote: char) -> (Span<'static>, usize) {
    let mut i = start + 1;
    while i < chars.len() {
        if chars[i] == '\\' {
            i += 2;
            continue;
        }
        if chars[i] == quote {
            i += 1;
            break;
        }
        i += 1;
    }
    let end = i.min(chars.len());
    (
        Span::styled(
            chars[start..end].iter().collect::<String>(),
            Style::default().fg(theme::get().add_fg),
        ),
        end,
    )
}

fn word_style(word: &str, called: bool, leading: bool) -> Style {
    if KEYWORDS.contains(&word) {
        return Style::default().fg(theme::get().purple);
    }
    if LITERALS.contains(&word) {
        return Style::default().fg(theme::get().amber);
    }
    // A word in call position, or the first word of a shell line, is the
    // thing being invoked.
    if called || leading {
        return Style::default().fg(theme::get().blue);
    }
    if starts_upper(word) {
        return Style::default().fg(theme::get().blue);
    }
    Style::default().fg(theme::get().text)
}

fn starts_upper(word: &str) -> bool {
    word.chars().next().is_some_and(char::is_uppercase)
}

fn is_leading(before: &[char]) -> bool {
    before.iter().all(|c| c.is_whitespace())
}

fn is_punct(c: char) -> bool {
    !c.is_alphanumeric() && c != '_' && !c.is_whitespace() && !matches!(c, '"' | '\'' | '`')
}

#[cfg(test)]
#[path = "tests/syntax_test.rs"]
mod tests;
