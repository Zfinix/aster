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
        // Naming the narrower re-read keeps the model from pulling the whole
        // file back in and evicting itself again on the next round.
        let stub = format!(
            "[evicted to fit the context budget: {chars} chars dropped. Only re-run this tool if you still need it, and ask for the specific range or filter you are missing rather than the whole thing]"
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
#[path = "tests/budget_test.rs"]
mod tests;
