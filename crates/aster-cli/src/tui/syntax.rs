//! A small, language-agnostic highlighter for code shown in the transcript.
//!
//! One tokenizer covers every language the agent is likely to print: it splits
//! on strings, comments, numbers and words, then colours words by a shared
//! keyword set. That is deliberately less precise than a real grammar and far
//! cheaper, which is the right trade for text that scrolls past once.

use ratatui::style::Style;
use ratatui::text::Span;

use super::theme;

/// Words that read as control flow or declarations across C-likes, Rust,
/// Python, Go and shell.
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

/// Words that read as values rather than names.
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

/// Comment openers, by the language families that use them.
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
                out.push(Span::styled(rest, theme::faint()));
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
                Style::default().fg(theme::AMBER),
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
            theme::dimmer(),
        ));
    }
    out
}

/// Consume a quoted run, tolerating backslash escapes and an unterminated
/// quote at end of line.
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
            Style::default().fg(theme::ADD_FG),
        ),
        end,
    )
}

fn word_style(word: &str, called: bool, leading: bool) -> Style {
    if KEYWORDS.contains(&word) {
        return Style::default().fg(theme::PURPLE);
    }
    if LITERALS.contains(&word) {
        return Style::default().fg(theme::AMBER);
    }
    // A word in call position, or the first word of a shell line, is the
    // thing being invoked.
    if called || leading {
        return Style::default().fg(theme::BLUE);
    }
    if starts_upper(word) {
        return Style::default().fg(theme::BLUE);
    }
    Style::default().fg(theme::TEXT)
}

fn starts_upper(word: &str) -> bool {
    word.chars().next().is_some_and(char::is_uppercase)
}

/// True when only whitespace precedes the word, i.e. it opens the line.
fn is_leading(before: &[char]) -> bool {
    before.iter().all(|c| c.is_whitespace())
}

fn is_punct(c: char) -> bool {
    !c.is_alphanumeric() && c != '_' && !c.is_whitespace() && !matches!(c, '"' | '\'' | '`')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn styles(src: &str) -> Vec<(String, Option<ratatui::style::Color>)> {
        highlight(src)
            .into_iter()
            .map(|s| (s.content.into_owned(), s.style.fg))
            .collect()
    }

    #[test]
    fn highlight_keeps_the_source_intact() {
        let src = "let x = foo(\"a\", 12); // note";
        let joined: String = highlight(src).iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(joined, src);
    }

    #[test]
    fn highlight_colours_keywords_strings_and_numbers() {
        let out = styles("let x = 12");
        assert_eq!(out[0], ("let".into(), Some(theme::PURPLE)));
        assert!(
            out.iter()
                .any(|(t, c)| t == "12" && *c == Some(theme::AMBER))
        );
    }

    #[test]
    fn highlight_treats_a_trailing_comment_as_one_span() {
        let out = styles("run # do the thing");
        assert_eq!(out.last().unwrap().1, Some(theme::FAINT));
        assert!(out.last().unwrap().0.starts_with('#'));
    }

    #[test]
    fn highlight_marks_the_first_shell_word_as_the_command() {
        let out = styles("cargo test");
        assert_eq!(out[0], ("cargo".into(), Some(theme::BLUE)));
    }

    #[test]
    fn highlight_leaves_an_unterminated_string_on_its_line() {
        let out = styles("echo \"open");
        assert_eq!(out.last().unwrap().1, Some(theme::ADD_FG));
    }
}
