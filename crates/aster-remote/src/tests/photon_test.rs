use super::markdown::to_plain_chunks;

#[test]
fn chunks_respect_limit_and_keep_all_lines() {
    let text = "one\ntwo\nthree";
    let chunks = to_plain_chunks(text, 8);
    assert_eq!(chunks.join("\n"), text);
    assert!(chunks.iter().all(|c| c.len() <= 8));
}

#[test]
fn long_line_becomes_its_own_chunk() {
    let chunks = to_plain_chunks("abcdefghij", 4);
    assert_eq!(chunks, vec!["abcdefghij".to_string()]);
}
