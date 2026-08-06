use super::*;

const BLOCK: &str = "Let me look.\n\n\
<\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}tool_calls>\n\
<\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}invoke name=\"search_files\">\n\
<\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}parameter name=\"query\" string=\"true\">sh__</\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}parameter>\n\
</\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}invoke>\n\
<\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}invoke name=\"read_file\">\n\
<\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}parameter name=\"path\" string=\"true\">a/b.ts</\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}parameter>\n\
<\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}parameter name=\"end_line\" string=\"false\">20</\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}parameter>\n\
</\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}invoke>\n\
</\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}tool_calls>";

#[test]
fn inline_calls_are_recovered_and_the_markup_is_dropped() {
    let (text, calls) = split_inline_tool_calls(BLOCK);

    assert_eq!(text, "Let me look.");
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].function.name, "search_files");
    assert_eq!(calls[0].function.arguments, r#"{"query":"sh__"}"#);
    assert_eq!(calls[1].function.name, "read_file");
    assert_eq!(
        calls[1].function.arguments,
        r#"{"end_line":20,"path":"a/b.ts"}"#
    );
}

#[test]
fn plain_text_is_untouched() {
    let (text, calls) = split_inline_tool_calls("Use `a < b` when comparing.");
    assert_eq!(text, "Use `a < b` when comparing.");
    assert!(calls.is_empty());
}

#[test]
fn a_block_without_parsable_calls_is_left_alone() {
    let content = "talking about <invoke name= syntax";
    let (text, calls) = split_inline_tool_calls(content);
    assert_eq!(text, content);
    assert!(calls.is_empty());
}

fn gated(deltas: &[&str]) -> String {
    let mut out = String::new();
    let mut gate = TokenGate::default();
    let mut emit = |s: &str| out.push_str(s);
    for delta in deltas {
        gate.feed(delta, &mut emit);
    }
    gate.finish(&mut emit);
    out
}

#[test]
fn the_gate_stops_at_the_opener_however_it_is_split() {
    let out = gated(&[
        "Let me look.\n\n<\u{ff5c}",
        "\u{ff5c}DSM",
        "L\u{ff5c}\u{ff5c}tool_ca",
        "lls>\n<\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}invoke name=\"x\">",
    ]);
    assert_eq!(out, "Let me look.");
}

#[test]
fn the_gate_passes_ordinary_text_through_whole() {
    assert_eq!(gated(&["a < b ", "and c ", "> d"]), "a < b and c > d");
}

#[test]
fn the_gate_stays_closed_for_the_rest_of_the_message() {
    let out = gated(&[
        "hi <invoke name=\"read_file\">",
        "\nmore markup",
        "</invoke>",
    ]);
    assert_eq!(out, "hi");
}
