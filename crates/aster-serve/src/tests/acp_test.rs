use std::time::Instant;

use super::*;

fn thinking_turn(chars: usize) -> Turn {
    Turn {
        event_id: "e1".into(),
        reply: String::new(),
        edits: Vec::new(),
        tool_names: HashMap::new(),
        permission: None,
        reasoning_chars: chars,
        reasoning_started: Some(Instant::now()),
        pending: Arc::default(),
    }
}

#[test]
fn each_thinking_block_counts_its_own_tokens() {
    let mut turn = thinking_turn(401);
    let done = turn.end_thinking().unwrap();
    assert_eq!(done["type"], "reasoning_done");
    assert_eq!(done["tokens"], 101);
    assert_eq!(turn.reasoning_chars, 0);
    assert!(turn.end_thinking().is_none());
}
