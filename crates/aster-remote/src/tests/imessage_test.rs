use super::markdown::to_plain_chunks;

#[test]
fn chunks_respect_limit_and_keep_all_lines() {
    let text = "one\ntwo\nthree\nfour";
    let chunks = to_plain_chunks(text, 9);
    assert_eq!(chunks.join("\n"), text);
    for c in &chunks {
        assert!(c.len() <= 9, "chunk too long: {c:?}");
    }
}

#[test]
fn long_line_becomes_its_own_chunk() {
    let chunks = to_plain_chunks(
        "short\nthis-is-one-very-long-line-that-exceeds-the-limit\nshort2",
        20,
    );
    assert!(chunks.iter().any(|c| c.contains("very-long-line")));
    assert_eq!(chunks.join("\n").lines().count(), 3);
}

#[test]
fn empty_text_yields_no_chunks() {
    assert!(to_plain_chunks("", 100).is_empty());
}
