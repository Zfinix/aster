use serde_json::json;
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

use super::*;
use crate::AiClient;

fn fields_for(base_url: &str, effort: Effort) -> Value {
    Value::Object(fields(dialect(base_url).knob, Some(effort)))
}

fn sse(chunks: &[Value]) -> String {
    let mut body: String = chunks.iter().map(|c| format!("data: {c}\n\n")).collect();
    body.push_str("data: [DONE]\n\n");
    body
}

#[test]
fn each_provider_gets_its_own_off_switch() {
    let cases = [
        (
            "https://api.fireworks.ai/inference/v1",
            json!({ "reasoning_effort": "none" }),
        ),
        (
            "https://openrouter.ai/api/v1",
            json!({ "reasoning": { "enabled": false } }),
        ),
        (
            "https://api.anthropic.com/v1",
            json!({ "thinking": { "type": "disabled" } }),
        ),
        (
            "https://api.together.xyz/v1",
            json!({ "reasoning": { "enabled": false } }),
        ),
        (
            "https://dashscope-intl.aliyuncs.com/compatible-mode/v1",
            json!({ "enable_thinking": false }),
        ),
        (
            "http://localhost:11434/v1",
            json!({ "reasoning_effort": false }),
        ),
        (
            "https://integrate.api.nvidia.com/v1",
            json!({ "chat_template_kwargs": { "enable_thinking": false } }),
        ),
        (
            "https://api.minimax.io/v1",
            json!({ "reasoning_split": true, "thinking": { "type": "disabled" } }),
        ),
        (
            "https://chatgpt.com/backend-api/codex",
            json!({ "reasoning": { "effort": "none" } }),
        ),
    ];
    for (url, want) in cases {
        assert_eq!(fields_for(url, Effort::Off), want, "{url}");
    }
}

#[test]
fn a_level_is_sent_where_the_provider_reads_one() {
    assert_eq!(
        fields_for("https://api.fireworks.ai/inference/v1", Effort::High),
        json!({ "reasoning_effort": "high" })
    );
    assert_eq!(
        fields_for("https://api.deepseek.com/v1", Effort::Ultra),
        json!({ "reasoning_effort": "max" })
    );
    assert_eq!(
        fields_for("https://openrouter.ai/api/v1", Effort::Low),
        json!({ "reasoning": { "effort": "low" } })
    );
    assert_eq!(
        fields_for("https://api.anthropic.com/v1", Effort::High),
        json!({})
    );
}

#[test]
fn a_refused_setting_steps_toward_the_model_default() {
    assert_eq!(relax(Some(Effort::Off)), Some(Some(Effort::Low)));
    assert_eq!(relax(Some(Effort::Ultra)), Some(Some(Effort::High)));
    assert_eq!(relax(Some(Effort::Medium)), Some(None));
    assert_eq!(relax(None), None);
}

#[test]
fn refusals_are_told_apart_by_what_they_name() {
    let thinking_only = anyhow::anyhow!(
        "bad request (400): GLM-5.3 is a thinking-only model; disabling thinking (reasoning_effort='none') is not supported."
    );
    assert!(rejected_effort(&thinking_only));
    assert!(!rejected_history(&thinking_only));

    let history = anyhow::anyhow!(
        "bad request (400): messages.3.assistant.reasoning_content: property 'reasoning_content' is unsupported"
    );
    assert!(rejected_history(&history));

    let missing = anyhow::anyhow!(
        "bad request (400): thinking is enabled but reasoning_content is missing in assistant tool call message at index 2"
    );
    assert!(!rejected_history(&missing));

    let unrelated = anyhow::anyhow!("rate limited (429): slow down");
    assert!(!rejected_effort(&unrelated));
}

fn assistant_with_thinking() -> Value {
    json!({
        "role": "assistant",
        "content": "done",
        "reasoning_details": [{ "type": "reasoning.text", "text": "plan it" }],
    })
}

#[test]
fn earlier_thinking_goes_back_in_the_providers_field() {
    let mut content = vec![assistant_with_thinking()];
    replay(&mut content, Replay::Content);
    assert_eq!(content[0]["reasoning_content"], "plan it");
    assert!(content[0].get("reasoning_details").is_none());

    let mut reasoning = vec![assistant_with_thinking()];
    replay(&mut reasoning, Replay::Reasoning);
    assert_eq!(reasoning[0]["reasoning"], "plan it");

    let mut stripped = vec![assistant_with_thinking()];
    replay(&mut stripped, Replay::Strip);
    assert_eq!(
        stripped[0],
        json!({ "role": "assistant", "content": "done" })
    );

    let mut details = vec![assistant_with_thinking()];
    replay(&mut details, Replay::Details);
    assert_eq!(details[0], assistant_with_thinking());

    let mut chunks = vec![assistant_with_thinking()];
    replay(&mut chunks, Replay::ThinkChunks);
    assert_eq!(chunks[0]["content"][0]["thinking"][0]["text"], "plan it");
    assert_eq!(chunks[0]["content"][1]["text"], "done");
}

#[test]
fn sealed_thinking_is_never_rewritten_as_text() {
    let mut messages = vec![json!({
        "role": "assistant",
        "content": "done",
        "reasoning_details": [{ "type": "reasoning.encrypted", "data": "sealed" }],
    })];
    replay(&mut messages, Replay::Content);
    assert!(messages[0].get("reasoning_content").is_none());
}

#[test]
fn every_spelling_of_thinking_reads_as_reasoning_content() {
    let mut mistral = json!({ "choices": [{ "delta": { "content": [
        { "type": "thinking", "thinking": [{ "type": "text", "text": "hmm" }] },
        { "type": "text", "text": "answer" }
    ] } }] });
    normalize(&mut mistral, "delta");
    assert_eq!(mistral["choices"][0]["delta"]["content"], "answer");
    assert_eq!(mistral["choices"][0]["delta"]["reasoning_content"], "hmm");

    let mut groq = json!({ "choices": [{ "delta": { "reasoning": "hmm" } }] });
    normalize(&mut groq, "delta");
    assert_eq!(groq["choices"][0]["delta"]["reasoning_content"], "hmm");

    let mut bedrock = json!({ "choices": [{ "message": {
        "content": "answer",
        "reasoning_content": { "reasoningContent": { "reasoningText": "hmm" } }
    } }] });
    normalize(&mut bedrock, "message");
    assert_eq!(bedrock["choices"][0]["message"]["reasoning_content"], "hmm");

    let mut tagged =
        json!({ "choices": [{ "message": { "content": "<think>hmm</think>\nanswer" } }] });
    normalize(&mut tagged, "message");
    assert_eq!(tagged["choices"][0]["message"]["content"], "answer");
    assert_eq!(tagged["choices"][0]["message"]["reasoning_content"], "hmm");
}

#[test]
fn openrouter_reasoning_is_not_counted_twice() {
    let mut chunk = json!({ "choices": [{ "delta": {
        "reasoning": "hmm",
        "reasoning_details": [{ "type": "reasoning.text", "text": "hmm" }]
    } }] });
    normalize(&mut chunk, "delta");
    assert!(
        chunk["choices"][0]["delta"]
            .get("reasoning_content")
            .is_none()
    );
}

#[test]
fn think_tags_split_across_chunks() {
    let mut tags = ThinkTags::default();
    let mut answer = String::new();
    let mut thinking = String::new();
    for chunk in ["<th", "ink>plan", " it</th", "ink>\n\nthe ", "answer"] {
        let (a, t) = tags.feed(chunk);
        answer.push_str(&a);
        thinking.push_str(&t);
    }
    let (a, t) = tags.finish();
    answer.push_str(&a);
    thinking.push_str(&t);
    assert_eq!(thinking, "plan it");
    assert_eq!(answer, "the answer");
}

#[test]
fn a_reply_without_think_tags_passes_through() {
    let mut tags = ThinkTags::default();
    assert_eq!(tags.feed("  "), (String::new(), String::new()));
    assert_eq!(tags.feed("hello"), ("  hello".to_string(), String::new()));
    assert_eq!(
        tags.feed(" <think>"),
        (" <think>".to_string(), String::new())
    );
}

#[tokio::test]
async fn a_reply_that_only_thinks_is_not_asked_again() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_string(sse(&[
            json!({ "choices": [{ "delta": { "reasoning_content": "thinking hard" } }] }),
            json!({ "choices": [{ "delta": {}, "finish_reason": "length" }] }),
        ])))
        .expect(1)
        .mount(&server)
        .await;

    let client = AiClient::new(server.uri(), "test-key", "mock-model");
    let err = client
        .complete_tools_stream_with("mock-model", vec![], vec![], 0.0, |_| {}, |_| {})
        .await
        .unwrap_err();
    assert_eq!(err.to_string(), THINKING_EXHAUSTED);
}

#[tokio::test]
async fn streamed_thinking_is_kept_for_the_next_request() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_string(sse(&[
            json!({ "choices": [{ "delta": { "reasoning_content": "read the file" } }] }),
            json!({ "choices": [{ "delta": { "tool_calls": [{
                "index": 0, "id": "call_1",
                "function": { "name": "read_file", "arguments": "{}" },
                "extra_content": { "google": { "thought_signature": "sig" } }
            }] } }] }),
            json!({ "choices": [{ "delta": {}, "finish_reason": "tool_calls" }] }),
        ])))
        .mount(&server)
        .await;

    let client = AiClient::new(server.uri(), "test-key", "mock-model");
    let mut seen = String::new();
    let msg = client
        .complete_tools_stream_with(
            "mock-model",
            vec![],
            vec![],
            0.0,
            |_| {},
            |t| seen.push_str(t),
        )
        .await
        .unwrap();
    assert_eq!(seen, "read the file");
    assert_eq!(
        msg.reasoning_details[0].text.as_deref(),
        Some("read the file")
    );
    assert_eq!(
        msg.tool_calls[0].extra_content,
        Some(json!({ "google": { "thought_signature": "sig" } }))
    );
}

#[tokio::test]
async fn a_refused_off_switch_falls_back_once_per_session() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(body_partial_json(json!({ "reasoning_effort": "none" })))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "error": { "message": "GLM-5.3 is a thinking-only model; disabling thinking (reasoning_effort='none') is not supported." }
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(body_partial_json(json!({ "reasoning_effort": "low" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{ "message": { "role": "assistant", "content": "hi" } }]
        })))
        .expect(2)
        .mount(&server)
        .await;

    let client = AiClient::new(server.uri(), "test-key", "glm").with_effort(Effort::Off);
    for _ in 0..2 {
        let msg = client
            .complete_tools_with(
                "glm",
                vec![json!({ "role": "user", "content": "hi" })],
                vec![],
                0.0,
            )
            .await
            .unwrap();
        assert_eq!(msg.content.as_deref(), Some("hi"));
    }
}

#[tokio::test]
async fn refused_history_thinking_is_dropped_and_retried() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(|request: &Request| {
            let body: Value = serde_json::from_slice(&request.body).unwrap();
            if body.to_string().contains("reasoning_content") {
                return ResponseTemplate::new(400).set_body_json(json!({
                    "error": { "message": "messages.1.assistant.reasoning_content: property 'reasoning_content' is unsupported" }
                }));
            }
            ResponseTemplate::new(200).set_body_json(json!({
                "choices": [{ "message": { "role": "assistant", "content": "ok" } }]
            }))
        })
        .expect(2)
        .mount(&server)
        .await;

    let client = AiClient::new(server.uri(), "test-key", "mock-model");
    let history = vec![
        json!({ "role": "user", "content": "go" }),
        assistant_with_thinking(),
        json!({ "role": "user", "content": "again" }),
    ];
    let msg = client
        .complete_tools_with("mock-model", history, vec![], 0.0)
        .await
        .unwrap();
    assert_eq!(msg.content.as_deref(), Some("ok"));
}
