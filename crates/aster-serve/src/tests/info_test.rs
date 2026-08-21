use super::*;

use serde_json::json;

#[test]
fn thousands_read_as_k() {
    assert_eq!(human(999), "999");
    assert_eq!(human(1000), "1k");
    assert_eq!(human(192_000), "192k");
}

#[test]
fn a_count_the_cli_could_not_read_says_so() {
    assert_eq!(count(&json!(null), "blocks"), "unavailable");
    assert_eq!(count(&json!(3), "blocks"), "3 blocks");
    assert_eq!(count(&json!(3), ""), "3");
}

#[test]
fn a_skill_description_is_cut_to_its_first_sentence() {
    let description = "Reviews a diff. Triggers on: review, audit, check.";
    assert_eq!(first_sentence(description), "Reviews a diff");
    assert_eq!(first_sentence("No full stop here"), "No full stop here");
}

#[test]
fn text_reads_a_string_without_its_quotes() {
    assert_eq!(text(&json!("gpt-5")), "gpt-5");
    assert_eq!(text(&json!(7)), "7");
}
