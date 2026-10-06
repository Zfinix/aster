use super::*;

#[test]
fn the_seed_goes_out_under_the_name_mistral_reads() {
    let body = serde_json::json!({ "model": "codestral-latest", "seed": 0, "messages": [] });
    assert_eq!(
        adapt_request(&body),
        serde_json::json!({ "model": "codestral-latest", "random_seed": 0, "messages": [] })
    );
}

#[test]
fn only_the_mistral_endpoint_is_matched() {
    assert!(is_mistral("https://api.mistral.ai/v1"));
    assert!(!is_mistral("https://openrouter.ai/api/v1"));
}
