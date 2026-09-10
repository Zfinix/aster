use super::*;

#[test]
fn render_code_fence_becomes_pre() {
    let chunks = to_html_chunks("before\n```rust\nlet x = 1;\n```\nafter", 4000);
    let html = chunks.join("\n");
    assert!(html.contains("<pre><code class=\"language-rust\">let x = 1;</code></pre>"));
    assert!(html.contains("before"));
    assert!(html.contains("after"));
}

#[test]
fn render_unknown_code_language_stays_plain() {
    let chunks = to_html_chunks("```not a language\nx\n```", 4000);
    assert_eq!(chunks[0], "<pre>x</pre>");
}

#[test]
fn render_quotes_become_blockquotes() {
    let chunks = to_html_chunks("> first\n> second\n\ntail", 4000);
    let html = chunks.join("\n");
    assert!(
        html.contains("<blockquote>first\nsecond</blockquote>"),
        "{html}"
    );
    assert!(html.contains("tail"));
}

#[test]
fn render_links_become_anchors() {
    let chunks = to_html_chunks("see [the docs](https://example.com/x)", 4000);
    assert_eq!(
        chunks[0],
        "see <a href=\"https://example.com/x\">the docs</a>"
    );
}

#[test]
fn render_rejects_unsafe_links() {
    let chunks = to_html_chunks("[x](javascript:alert(1))", 4000);
    assert!(!chunks.join("\n").contains("<a href"));
}

#[test]
fn render_collapses_repeated_blank_lines() {
    let chunks = to_html_chunks("one\n\n\n\ntwo", 4000);
    assert_eq!(chunks[0], "one\n\ntwo");
}

#[test]
fn render_escapes_html_in_code() {
    let chunks = to_html_chunks("```\nVec<String>\n```", 4000);
    assert!(chunks[0].contains("Vec&lt;String&gt;"));
}

#[test]
fn render_inline_code_and_bold() {
    let chunks = to_html_chunks("run `cargo test` for **all** crates", 4000);
    assert_eq!(
        chunks[0],
        "run <code>cargo test</code> for <b>all</b> crates"
    );
}

#[test]
fn render_header_and_bullets() {
    let chunks = to_html_chunks("## Plan\n- first\n- second", 4000);
    assert_eq!(chunks[0], "<b>Plan</b>\n•  first\n•  second");
}

#[test]
fn render_unbalanced_backticks_stay_plain() {
    let chunks = to_html_chunks("odd ` tick", 4000);
    assert_eq!(chunks[0], "odd ` tick");
}

#[test]
fn chunking_splits_long_code_into_multiple_pre() {
    let code = format!("```\n{}\n```", "x".repeat(9000));
    let chunks = to_html_chunks(&code, 4000);
    assert!(chunks.len() >= 3);
    for chunk in &chunks {
        assert!(chunk.len() <= 4000);
        assert_eq!(
            chunk.matches("<pre>").count(),
            chunk.matches("</pre>").count()
        );
    }
}

#[test]
fn chunking_never_exceeds_limit() {
    let text = format!("{}\n```\n{}\n```", "word ".repeat(2000), "y".repeat(5000));
    for chunk in to_html_chunks(&text, 4000) {
        assert!(chunk.len() <= 4000, "chunk was {}", chunk.len());
    }
}
