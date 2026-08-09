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
    assert_eq!(out[0], ("let".into(), Some(theme::get().purple)));
    assert!(
        out.iter()
            .any(|(t, c)| t == "12" && *c == Some(theme::get().amber))
    );
}

#[test]
fn highlight_treats_a_trailing_comment_as_one_span() {
    let out = styles("run # do the thing");
    assert_eq!(out.last().unwrap().1, Some(theme::get().faint));
    assert!(out.last().unwrap().0.starts_with('#'));
}

#[test]
fn highlight_marks_the_first_shell_word_as_the_command() {
    let out = styles("cargo test");
    assert_eq!(out[0], ("cargo".into(), Some(theme::get().blue)));
}

#[test]
fn highlight_leaves_an_unterminated_string_on_its_line() {
    let out = styles("echo \"open");
    assert_eq!(out.last().unwrap().1, Some(theme::get().add_fg));
}
