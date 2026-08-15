use super::*;
use serde_json::json;

fn tool(content: &str) -> Value {
    json!({ "role": "tool", "tool_call_id": "t", "content": content })
}

#[test]
fn history_budget_reserves_system_and_headroom() {
    assert_eq!(history_budget(100_000, 10_000), 74_000);
}

#[test]
fn history_budget_floors_at_quarter_of_total() {
    assert_eq!(history_budget(100_000, 95_000), 25_000);
}

#[test]
fn under_budget_evicts_nothing() {
    let mut wire = vec![json!({ "role": "system", "content": "s" })];
    for _ in 0..12 {
        wire.push(tool(&"x".repeat(2_000)));
    }
    let evictions = evict_tool_results(&mut wire, 1_000_000);
    assert!(evictions.is_empty());
}

#[test]
fn evicts_oldest_tool_results_first_and_keeps_the_tail() {
    let mut wire = vec![json!({ "role": "system", "content": "s" })];
    for i in 0..12 {
        wire.push(tool(&format!("{i}:{}", "x".repeat(5_000))));
    }
    let evictions = evict_tool_results(&mut wire, 30_000);
    assert!(!evictions.is_empty());
    assert_eq!(evictions[0].index, 1);
    assert!(evictions.iter().all(|e| e.index < wire.len() - 8));
    let first = wire[1]["content"].as_str().unwrap();
    assert!(first.starts_with("[evicted"));
    let last = wire.last().unwrap()["content"].as_str().unwrap();
    assert!(last.starts_with("11:"));
}

#[test]
fn never_evicts_user_or_assistant_turns() {
    let mut wire = vec![json!({ "role": "system", "content": "s" })];
    wire.push(json!({ "role": "user", "content": "u".repeat(50_000) }));
    for _ in 0..10 {
        wire.push(tool("small"));
    }
    let evictions = evict_tool_results(&mut wire, 1_000);
    assert!(evictions.is_empty());
    assert_eq!(wire[1]["content"].as_str().unwrap().len(), 50_000);
}

#[test]
fn eviction_stops_once_under_budget() {
    let mut wire = vec![json!({ "role": "system", "content": "s" })];
    for i in 0..20 {
        wire.push(tool(&format!("{i}:{}", "x".repeat(10_000))));
    }
    let evictions = evict_tool_results(&mut wire, 110_000);
    assert!(!evictions.is_empty());
    assert!(
        evictions.len() < 12,
        "evicted more than needed: {}",
        evictions.len()
    );
    assert!(used(&wire) <= 110_000);
}

#[test]
fn an_attached_image_is_charged_flat_rather_than_by_its_encoding() {
    let wire = vec![json!({
        "role": "user",
        "content": [
            { "type": "text", "text": "what is this" },
            { "type": "image_url", "image_url": { "url": format!("data:image/png;base64,{}", "A".repeat(400_000)) } },
        ],
    })];
    assert_eq!(used(&wire), "what is this".len() + IMAGE_CHARS);
}
