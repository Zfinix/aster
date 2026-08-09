use super::*;

#[test]
fn render_code_fence_becomes_pre() {
    let chunks = to_html_chunks("before\n```rust\nlet x = 1;\n```\nafter", 4000);
    let html = chunks.join("\n");
    assert!(html.contains("<pre>let x = 1;</pre>"));
    assert!(html.contains("before"));
    assert!(html.contains("after"));
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
