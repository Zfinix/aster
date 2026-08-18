use crate::AiClient;
use serde_json::json;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn chat_response() -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "choices": [{ "message": { "role": "assistant", "content": "hello" } }],
        "usage": { "prompt_tokens": 10, "completion_tokens": 10 }
    }))
}

#[tokio::test]
async fn attribution_headers_are_sent_on_chat_requests() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(header("HTTP-Referer", "https://github.com/zfinix/aster"))
        .and(header("X-OpenRouter-Title", "Aster"))
        .respond_with(chat_body())
        .mount(&server)
        .await;

    let client = AiClient::new(server.uri(), "test-key", "mock-model").with_attribution_headers([
        (
            "HTTP-Referer".to_string(),
            "https://github.com/zfinix/aster".to_string(),
        ),
        ("X-OpenRouter-Title".to_string(), "Aster".to_string()),
    ]);

    let out = client
        .complete_with("test-model", "sys", "user", 0.0)
        .await
        .unwrap();
    assert_eq!(out, "hello");
}

#[tokio::test]
async fn requests_without_attribution_do_not_carry_the_headers() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(wiremock::matchers::header::header_not_existing("HTTP-Referer"))
        .respond_with(chat_body())
        .mount(&server)
        .await;

    let client = AiClient::new(server.uri(), "test-key", "test-model");
    client
        .complete_with("test-model", "sys", "user", 0.0)
        .await
        .unwrap();
}