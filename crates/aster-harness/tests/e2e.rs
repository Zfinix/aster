//! End-to-end pipeline test against a mock OpenAI-compatible server. Exercises
//! the full HYPOTHESIZE -> VERIFY -> SHAPE flow (including the confidence gate
//! and the streaming-to-non-streaming fallback) without a real provider.

use std::sync::Arc;

use aster_ai::AiClient;
use aster_harness::{HarnessConfig, ReviewDeps, ReviewInput, review};
use serde_json::json;
use wiremock::matchers::{body_string_contains, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn chat_response(content: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "choices": [{ "message": { "role": "assistant", "content": content } }],
        "usage": { "prompt_tokens": 100, "completion_tokens": 20 }
    }))
}

async fn mount(server: &MockServer, marker: &str, content: &str) {
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(body_string_contains(marker))
        .respond_with(chat_response(content))
        .mount(server)
        .await;
}

fn deps(base_url: String) -> ReviewDeps {
    ReviewDeps {
        ai_client: Arc::new(AiClient::new(base_url, "test-key", "mock-model")),
        index: None,
        config: HarnessConfig::default(),
    }
}

fn input() -> ReviewInput {
    ReviewInput {
        diff: "diff --git a/src/foo.rs b/src/foo.rs\n\
               --- a/src/foo.rs\n\
               +++ b/src/foo.rs\n\
               @@ -1,3 +1,3 @@\n\
               -let x = items[0];\n\
               +let x = items[n];\n"
            .to_string(),
        base_branch: "main".into(),
        repo_name: "acme/widget".into(),
        pr_number: None,
        repo_root: None,
    }
}

#[tokio::test]
async fn confirmed_finding_survives_full_pipeline() {
    let server = MockServer::start().await;
    // "over-produce" is unique to the hypothesis system prompt; "REFUTE" to the
    // verify prompt. Both stream and non-stream requests carry the prompt, so a
    // single mock serves the streaming attempt and its non-streaming fallback.
    mount(
        &server,
        "over-produce",
        r#"{"candidates":[{"file":"src/foo.rs","line":2,"defect_class":"correctness","severity":"high","title":"index out of bounds","failure_scenario":"n >= items.len() panics","suggestion":"bounds-check n"}]}"#,
    )
    .await;
    mount(
        &server,
        "REFUTE",
        r#"{"real":true,"confidence":0.9,"reason":"n can exceed length","explanation":"the program crashes when n is too large"}"#,
    )
    .await;

    let report = review(&deps(server.uri()), input()).await.unwrap();

    assert_eq!(report.findings.len(), 1, "one finding should survive");
    let f = &report.findings[0];
    assert_eq!(f.file_path, "src/foo.rs");
    assert_eq!(f.severity, "high");
    assert_eq!(f.confidence, Some(0.9));
    assert_eq!(f.category, "correctness");
}

#[tokio::test]
async fn low_confidence_is_refuted_by_the_gate() {
    let server = MockServer::start().await;
    mount(
        &server,
        "over-produce",
        r#"{"candidates":[{"file":"src/foo.rs","line":2,"defect_class":"correctness","severity":"high","title":"maybe a bug","failure_scenario":"could crash","suggestion":"look into it"}]}"#,
    )
    .await;
    // Verifier says real, but confidence 0.2 is below the default 0.5 gate.
    mount(
        &server,
        "REFUTE",
        r#"{"real":true,"confidence":0.2,"reason":"unsure","explanation":"might crash"}"#,
    )
    .await;

    let report = review(&deps(server.uri()), input()).await.unwrap();
    assert_eq!(
        report.findings.len(),
        0,
        "low-confidence finding must be gated out"
    );
}

#[tokio::test]
async fn scenario_gate_drops_candidates_without_a_failure_scenario() {
    let server = MockServer::start().await;
    // Candidate has an empty failure_scenario; it must never reach verification.
    mount(
        &server,
        "over-produce",
        r#"{"candidates":[{"file":"src/foo.rs","line":2,"defect_class":"style","severity":"low","title":"naming","failure_scenario":"   ","suggestion":"rename"}]}"#,
    )
    .await;

    let report = review(&deps(server.uri()), input()).await.unwrap();
    assert_eq!(
        report.findings.len(),
        0,
        "candidate without a scenario is dropped pre-verify"
    );
}
