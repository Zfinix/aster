use super::*;

#[test]
fn a_zero_seed_is_dropped() {
    let body = serde_json::json!({ "model": "venice-uncensored", "seed": 0, "messages": [] });
    assert_eq!(
        adapt_request(&body),
        serde_json::json!({ "model": "venice-uncensored", "messages": [] })
    );
}

#[test]
fn a_seed_venice_accepts_is_left_alone() {
    let body = serde_json::json!({ "seed": 7, "messages": [] });
    assert_eq!(adapt_request(&body), body);
}

#[test]
fn only_the_venice_endpoint_is_matched() {
    assert!(is_venice("https://api.venice.ai/api/v1"));
    assert!(!is_venice("https://openrouter.ai/api/v1"));
}
