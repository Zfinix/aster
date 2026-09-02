//! Tests for the chat-completions to Responses translation in `codex_api`.

use super::*;
use serde_json::json;

#[test]
fn is_codex_matches_only_the_backend_host() {
    assert!(is_codex("https://chatgpt.com/backend-api/codex"));
    assert!(!is_codex("https://openrouter.ai/api/v1"));
    assert!(!is_codex("https://api.openai.com/v1"));
}

#[test]
fn translate_request_system_becomes_instructions() {
    let chat = json!({
        "model": "gpt-5.1",
        "messages": [
            {"role": "system", "content": "be brief"},
            {"role": "user", "content": "hi"},
        ],
    });
    let out = translate_request(&chat);
    assert_eq!(out["instructions"], "be brief");
    assert_eq!(out["input"][0]["role"], "user");
    assert_eq!(out["input"][0]["content"][0]["type"], "input_text");
    assert_eq!(out["input"][0]["content"][0]["text"], "hi");
}

#[test]
fn translate_request_tool_round_trip() {
    let chat = json!({
        "model": "gpt-5.1",
        "messages": [
            {"role": "user", "content": "run ls"},
            {"role": "assistant", "content": "", "tool_calls": [{
                "id": "call_1", "kind": "function",
                "function": {"name": "shell", "arguments": "{\"cmd\":\"ls\"}"},
            }]},
            {"role": "tool", "tool_call_id": "call_1", "content": "a.txt"},
        ],
    });
    let out = translate_request(&chat);
    assert_eq!(out["input"][1]["type"], "function_call");
    assert_eq!(out["input"][1]["call_id"], "call_1");
    assert_eq!(out["input"][1]["name"], "shell");
    assert_eq!(out["input"][2]["type"], "function_call_output");
    assert_eq!(out["input"][2]["output"], "a.txt");
}

#[test]
fn translate_request_flattens_tools() {
    let chat = json!({
        "model": "gpt-5.1",
        "messages": [{"role": "user", "content": "x"}],
        "tools": [{
            "type": "function",
            "function": {"name": "shell", "description": "run it", "parameters": {"type": "object"}},
        }],
    });
    let out = translate_request(&chat);
    assert_eq!(out["tools"][0]["type"], "function");
    assert_eq!(out["tools"][0]["name"], "shell");
    assert_eq!(out["tools"][0]["parameters"]["type"], "object");
}

#[test]
fn translate_request_carries_effort_and_drops_max_tokens() {
    let chat = json!({
        "model": "gpt-5.1",
        "messages": [{"role": "user", "content": "x"}],
        "max_tokens": 512,
        "reasoning": {"effort": "high"},
    });
    let out = translate_request(&chat);
    assert_eq!(out["reasoning"]["effort"], "high");
    // The ChatGPT backend rejects max_output_tokens outright.
    assert!(out.get("max_output_tokens").is_none());
    assert_eq!(out["store"], false);
}

#[test]
fn translate_response_text_and_tool_calls() {
    let responses = json!({
        "output": [
            {"type": "reasoning"},
            {"type": "message", "content": [{"type": "output_text", "text": "doing it"}]},
            {"type": "function_call", "call_id": "call_9", "name": "shell", "arguments": "{}"},
        ],
        "usage": {"input_tokens": 10, "output_tokens": 4},
    });
    let out = translate_response(&responses);
    let message = &out["choices"][0]["message"];
    assert_eq!(message["content"], "doing it");
    assert_eq!(message["tool_calls"][0]["id"], "call_9");
    assert_eq!(message["tool_calls"][0]["function"]["name"], "shell");
    assert_eq!(out["choices"][0]["finish_reason"], "tool_calls");
    assert_eq!(out["usage"]["prompt_tokens"], 10);
    assert_eq!(out["usage"]["completion_tokens"], 4);
    assert_eq!(out["usage"]["total_tokens"], 14);
}

#[test]
fn stream_translator_maps_text_and_usage() {
    let mut translator = StreamTranslator::default();
    let delta = translator
        .event(r#"{"type":"response.output_text.delta","delta":"he"}"#)
        .expect("text delta maps");
    assert!(delta.contains(r#""content":"he""#));

    let done = translator
        .event(
            r#"{"type":"response.completed","response":{"usage":{"input_tokens":7,"output_tokens":2}}}"#,
        )
        .expect("completed maps");
    assert!(done.contains(r#""prompt_tokens":7"#));

    // Events with no chat-completions equivalent are dropped, not fatal.
    assert!(
        translator
            .event(r#"{"type":"response.created","response":{}}"#)
            .is_none()
    );
}

#[test]
fn stream_translator_tool_fragments_share_an_index() {
    let mut translator = StreamTranslator::default();
    let args = translator
        .event(r#"{"type":"response.function_call_arguments.delta","item_id":"fc_1","delta":"{}"}"#)
        .expect("argument delta maps");
    let done = translator
        .event(
            r#"{"type":"response.output_item.done","item":{"id":"fc_1","type":"function_call","call_id":"call_1","name":"shell","arguments":"{}"}}"#,
        )
        .expect("item-done maps");

    let args = serde_json::from_str::<Value>(&args).unwrap();
    let done = serde_json::from_str::<Value>(&done).unwrap();
    let args_index = args["choices"][0]["delta"]["tool_calls"][0]["index"]
        .as_u64()
        .expect("index present") as usize;
    let done_index = done["choices"][0]["delta"]["tool_calls"][0]["index"]
        .as_u64()
        .expect("index present") as usize;
    assert_eq!(args_index, done_index);

    let second = translator
        .event(
            r#"{"type":"response.output_item.done","item":{"id":"fc_2","type":"function_call","call_id":"call_2","name":"read","arguments":""}}"#,
        )
        .expect("second call maps");
    let second = serde_json::from_str::<Value>(&second).unwrap();
    let second_index = second["choices"][0]["delta"]["tool_calls"][0]["index"]
        .as_u64()
        .expect("index present") as usize;
    assert_ne!(second_index, args_index);
}

#[test]
fn assemble_stream_builds_one_body_from_the_completed_event() {
    let sse = concat!(
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"hel\"}\n",
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"lo\"}\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"output\":[{\"type\":\"message\",\"content\":[{\"type\":\"output_text\",\"text\":\"hello\"}]}],\"usage\":{\"input_tokens\":7,\"output_tokens\":2}}}\n",
        "data: [DONE]\n",
    );
    let body = assemble_stream(sse).expect("completed event assembles");
    assert_eq!(body["choices"][0]["message"]["content"], "hello");
    assert_eq!(body["usage"]["prompt_tokens"], 7);
    assert_eq!(body["choices"][0]["finish_reason"], "stop");
}

#[test]
fn assemble_stream_rebuilds_output_the_backend_stripped() {
    // The live backend sends the completed event with an empty output array;
    // the item-done events carry the only whole copy of the reply.
    let sse = concat!(
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"ok\",\"item_id\":\"msg_1\"}\n",
        "data: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"ok\"}]}}\n",
        "data: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"fc_1\",\"type\":\"function_call\",\"call_id\":\"call_1\",\"name\":\"shell\",\"arguments\":\"{}\"}}\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"output\":[],\"usage\":{\"input_tokens\":7,\"output_tokens\":2}}}\n",
        "data: [DONE]\n",
    );
    let body = assemble_stream(sse).expect("stripped output assembles");
    assert_eq!(body["choices"][0]["message"]["content"], "ok");
    assert_eq!(
        body["choices"][0]["message"]["tool_calls"][0]["function"]["name"],
        "shell"
    );
    assert_eq!(body["choices"][0]["finish_reason"], "tool_calls");
    assert_eq!(body["usage"]["prompt_tokens"], 7);
}

#[test]
fn assemble_stream_is_none_without_a_completed_event() {
    let sse = "data: {\"type\":\"response.output_text.delta\",\"delta\":\"hi\"}\n";
    assert!(assemble_stream(sse).is_none());
    assert!(assemble_stream("").is_none());
}
