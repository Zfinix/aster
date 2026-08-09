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
