use std::collections::BTreeMap;

use super::*;
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn choice_answer(choice: &str, probability: f64) -> serde_json::Value {
    serde_json::json!({
        "type": "choice",
        "choice": choice,
        "probabilities": { choice: probability },
        "confidence": probability,
    })
}

fn one_question() -> BTreeMap<String, Question> {
    let mut questions = BTreeMap::new();
    questions.insert(
        "action".to_string(),
        Question::Choice {
            instructions: "decide".to_string(),
            criteria: vec![("stop".to_string(), "done".to_string())],
        },
    );
    questions
}

#[tokio::test]
async fn parses_typed_answers() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/systemone"))
        .and(body_partial_json(serde_json::json!({
            "model": "jev-latest",
            "state": "the state",
            // Guards the wire tag: the API rejects `boolean`, it wants `noul`.
            "questions": { "done": { "type": "noul" } },
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "answers": {
                "action": choice_answer("stop", 0.95),
                "done": { "type": "noul", "noul": 0.99 },
                "quality": { "type": "score", "score": 2.97 },
            }
        })))
        .mount(&server)
        .await;

    let client = JevClient::new("key").base_url(server.uri());
    let mut questions = one_question();
    questions.insert(
        "done".to_string(),
        Question::Boolean {
            instructions: "is it done".to_string(),
        },
    );
    questions.insert(
        "quality".to_string(),
        Question::Score {
            instructions: "rate it".to_string(),
            criteria: vec!["bad".to_string(), "good".to_string()],
        },
    );

    let answers = client.evaluate("the state", &questions).await.unwrap();
    assert_eq!(
        answers.get("action"),
        Some(&Answer::Choice {
            choice: "stop".to_string(),
            probabilities: [("stop".to_string(), 0.95)].into_iter().collect(),
            confidence: Some(0.95),
        })
    );
    assert_eq!(
        answers.get("done"),
        Some(&Answer::Boolean { probability: 0.99 })
    );
    assert_eq!(
        answers.get("quality"),
        Some(&Answer::Score {
            score: 2.97,
            probabilities: BTreeMap::new(),
            confidence: None,
        })
    );
}

fn entries() -> Vec<(String, String)> {
    vec![
        (
            "everyday".to_string(),
            "small edits and questions".to_string(),
        ),
        (
            "deep".to_string(),
            "multi-step design and debugging".to_string(),
        ),
    ]
}

#[tokio::test]
async fn route_sends_entries_as_choice_criteria() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/systemone"))
        .and(body_partial_json(serde_json::json!({
            "state": "User message:\nwhy does the retry loop deadlock?",
            "questions": { "entry": {
                "type": "choice",
                "criteria": {
                    "everyday": "small edits and questions",
                    "deep": "multi-step design and debugging",
                },
            } },
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "answers": { "entry": choice_answer("deep", 0.91) }
        })))
        .mount(&server)
        .await;

    let client = JevClient::new("key").base_url(server.uri());
    let pick = client
        .route("why does the retry loop deadlock?", &entries())
        .await
        .unwrap();
    assert_eq!(
        pick,
        RoutePick {
            entry: "deep".to_string(),
            probability: 0.91,
        }
    );
}

#[tokio::test]
async fn route_rejects_an_undeclared_entry() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/systemone"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "answers": { "entry": choice_answer("scout", 0.99) }
        })))
        .mount(&server)
        .await;

    let client = JevClient::new("key").base_url(server.uri());
    assert!(client.route("rename x", &entries()).await.is_err());
}

#[tokio::test]
async fn route_reports_a_failed_call() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/systemone"))
        .respond_with(ResponseTemplate::new(500).set_body_json(serde_json::json!({})))
        .mount(&server)
        .await;

    let client = JevClient::new("key").base_url(server.uri());
    assert!(client.route("rename x", &entries()).await.is_err());
}

#[tokio::test]
async fn confident_stop_is_advised() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/systemone"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "answers": { "action": choice_answer("stop", 0.95) }
        })))
        .mount(&server)
        .await;

    let client = JevClient::new("key").base_url(server.uri());
    let action = client
        .advise_loop(&LoopState {
            round: 3,
            round_cap: 60,
            recent: "read src/lib.rs".to_string(),
        })
        .await;
    assert_eq!(action, Some(LoopAction::Stop));
}

#[tokio::test]
async fn low_confidence_is_ignored() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/systemone"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "answers": { "action": choice_answer("stop", 0.4) }
        })))
        .mount(&server)
        .await;

    let client = JevClient::new("key").base_url(server.uri());
    let action = client
        .advise_loop(&LoopState {
            round: 3,
            round_cap: 60,
            recent: String::new(),
        })
        .await;
    assert_eq!(action, None);
}

#[tokio::test]
async fn server_errors_fall_back_to_none() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/systemone"))
        .respond_with(ResponseTemplate::new(500).set_body_json(serde_json::json!({
            "error": "overloaded"
        })))
        .mount(&server)
        .await;

    let client = JevClient::new("key").base_url(server.uri());
    let action = client
        .advise_loop(&LoopState {
            round: 0,
            round_cap: 60,
            recent: String::new(),
        })
        .await;
    assert_eq!(action, None);
}

#[tokio::test]
async fn malformed_reply_falls_back_to_none() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/systemone"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "answers": { "action": { "type": "choice" } }
        })))
        .mount(&server)
        .await;

    let client = JevClient::new("key").base_url(server.uri());
    let action = client
        .advise_loop(&LoopState {
            round: 0,
            round_cap: 60,
            recent: String::new(),
        })
        .await;
    assert_eq!(action, None);
}

#[test]
fn state_snapshot_is_capped() {
    let long = "x".repeat(MAX_STATE_CHARS * 2);
    let state = LoopState {
        round: 0,
        round_cap: 60,
        recent: long,
    };
    let built = state.state_prompt();
    assert!(built.ends_with(&truncate(&state.recent, MAX_STATE_CHARS)));
    assert_eq!(
        built.chars().count(),
        4096 + "Tool round 1 of a hard cap of 60.\nRecent activity:\n".len()
    );
}
