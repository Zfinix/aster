//! Context budget: reservations off the top, eviction by policy rather than
//! position, every eviction reported to the caller for the transcript.

use serde_json::Value;

/// Held back for the model's reply and the next round of tool results.
const RESPONSE_HEADROOM_CHARS: usize = 16_000;
/// Most recent request entries that are never evicted, so the model keeps what
/// it just read.
const KEEP_RECENT: usize = 8;
/// Tool results below this size are not worth stubbing out.
const MIN_EVICT_CHARS: usize = 1_000;

/// One message stubbed out to fit the budget.
pub(crate) struct Eviction {
    pub reason: &'static str,
    pub role: &'static str,
    pub index: usize,
    pub chars: usize,
}

/// What the history may spend after the system prompt (persona, instructions,
/// memory, skills) and reply headroom take their reservations. Floored at a
/// quarter of the total so a huge system prompt cannot starve the history.
pub(crate) fn history_budget(total: usize, system_chars: usize) -> usize {
    total
        .saturating_sub(system_chars + RESPONSE_HEADROOM_CHARS)
        .max(total / 4)
}

pub(crate) fn used(wire: &[Value]) -> usize {
    wire.iter()
        .map(|m| m["content"].as_str().map_or(0, str::len))
        .sum()
}

/// Evict by policy: stale tool results first, oldest first, never the recent
/// tail. They are the bulk of a long turn and the model can re-run the tool,
/// unlike user or assistant turns which are gone for good once dropped.
pub(crate) fn evict_tool_results(wire: &mut [Value], budget: usize) -> Vec<Eviction> {
    let mut evictions = Vec::new();
    let mut over = used(wire).saturating_sub(budget);
    if over == 0 || wire.len() <= KEEP_RECENT + 1 {
        return evictions;
    }
    let cutoff = wire.len() - KEEP_RECENT;
    for (index, msg) in wire.iter_mut().enumerate().take(cutoff).skip(1) {
        if over == 0 {
            break;
        }
        if msg["role"].as_str() != Some("tool") {
            continue;
        }
        let Some(content) = msg["content"].as_str() else {
            continue;
        };
        let chars = content.len();
        if chars < MIN_EVICT_CHARS {
            continue;
        }
        let stub = format!(
            "[evicted to fit the context budget: {chars} chars dropped; re-run the tool if this is needed again]"
        );
        over = over.saturating_sub(chars.saturating_sub(stub.len()));
        msg["content"] = Value::String(stub);
        evictions.push(Eviction {
            reason: "tool_result_over_budget",
            role: "tool",
            index,
            chars,
        });
    }
    evictions
}

#[cfg(test)]
mod tests {
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
}
