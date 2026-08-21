use super::*;

use serde_json::json;

fn session(events: Value) -> Value {
    json!({ "id": "s1", "events": events })
}

#[test]
fn a_user_turn_and_its_reply_are_rebuilt() {
    let turns = turns(&session(json!([
        { "type": "message", "role": "user", "content": "hello" },
        { "type": "message", "role": "assistant", "content": "hi" },
    ])));
    let turns = turns.as_array().expect("turns");
    assert_eq!(turns.len(), 2);
    assert_eq!(turns[0]["role"], "user");
    assert_eq!(turns[1]["content"], "hi");
}

#[test]
fn a_tool_result_lands_on_the_call_that_made_it() {
    let turns = turns(&session(json!([
        {
            "type": "message",
            "role": "assistant",
            "content": "",
            "tool_calls": [
                { "id": "c1", "function": { "name": "read_file", "arguments": "{\"path\":\"a\"}" } }
            ],
        },
        { "type": "message", "role": "tool", "tool_call_id": "c1", "content": "file body" },
    ])));
    let call = &turns[0]["toolCalls"][0];
    assert_eq!(call["name"], "read_file");
    assert_eq!(call["result"], "file body");
    assert_eq!(call["error"], Value::Null, "a plain result is not an error");
}

#[test]
fn a_failed_tool_result_is_marked() {
    let turns = turns(&session(json!([
        {
            "type": "message",
            "role": "assistant",
            "tool_calls": [{ "id": "c1", "function": { "name": "run", "arguments": "{}" } }],
        },
        { "type": "message", "role": "tool", "tool_call_id": "c1", "content": "error: no such file" },
    ])));
    assert_eq!(turns[0]["toolCalls"][0]["error"], true);
}

#[test]
fn a_round_of_thinking_alone_is_still_a_turn() {
    let turns = turns(&session(json!([
        { "type": "message", "role": "assistant", "content": "  ",
          "reasoning": { "text": "weighing it up", "tokens": 12, "duration_ms": 900 } },
    ])));
    let turns = turns.as_array().expect("turns");
    assert_eq!(turns.len(), 1);
    assert_eq!(turns[0]["reasoning"]["text"], "weighing it up");
    assert_eq!(turns[0]["reasoning"]["durationMs"], 900);
}

#[test]
fn turns_with_nothing_to_draw_are_dropped() {
    let turns = turns(&session(json!([
        { "type": "message", "role": "user", "content": "   " },
        { "type": "message", "role": "assistant", "content": "" },
        { "type": "message", "role": "system", "content": "steering" },
        { "type": "usage", "role": "assistant", "content": "not a message" },
    ])));
    assert_eq!(turns.as_array().expect("turns").len(), 0);
}

#[test]
fn a_call_without_an_id_still_renders() {
    let turns = turns(&session(json!([
        { "type": "message", "role": "assistant",
          "tool_calls": [{ "function": { "name": "run", "arguments": "{}" } }] },
    ])));
    assert_eq!(turns[0]["toolCalls"][0]["id"], "restored-0");
}
