use super::*;

#[cfg(feature = "jev")]
#[test]
fn snapshot_keeps_the_last_entries_bounded() {
    let wire: Vec<serde_json::Value> = (0..10)
        .map(|i| serde_json::json!({ "role": "assistant", "content": format!("round {i}") }))
        .collect();
    let snap = snapshot(&wire);
    assert!(snap.contains("round 9"));
    assert!(!snap.contains("round 3"));
    let long = "x".repeat(2000);
    let wire = vec![serde_json::json!({ "role": "user", "content": long })];
    assert!(snapshot(&wire).chars().count() < 600);
}

#[cfg(feature = "jev")]
async fn advisor_answering(
    status: u16,
    answers: serde_json::Value,
) -> (Advisor, wiremock::MockServer) {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/systemone"))
        .respond_with(ResponseTemplate::new(status).set_body_json(answers))
        .mount(&server)
        .await;
    let advisor = Advisor {
        client: Some(aster_jev::JevClient::new("key").base_url(server.uri())),
    };
    (advisor, server)
}

#[cfg(feature = "jev")]
fn route_answer(entry: &str, probability: f64) -> serde_json::Value {
    serde_json::json!({ "answers": { "entry": {
        "type": "choice",
        "choice": entry,
        "probabilities": { entry: probability },
        "confidence": probability,
    } } })
}

#[cfg(feature = "jev")]
fn entries() -> Vec<(String, String)> {
    vec![
        ("everyday".to_string(), "small edits".to_string()),
        ("deep".to_string(), "hard debugging".to_string()),
    ]
}

#[cfg(feature = "jev")]
#[tokio::test]
async fn route_takes_a_confident_pick() {
    let (advisor, _server) = advisor_answering(200, route_answer("deep", 0.9)).await;
    assert_eq!(
        advisor.route("why does this deadlock", &entries()).await,
        RouteAdvice::Picked {
            entry: "deep".to_string(),
            probability: 0.9,
        }
    );
}

#[cfg(feature = "jev")]
#[tokio::test]
async fn route_below_the_bar_is_unsure() {
    let (advisor, _server) = advisor_answering(200, route_answer("deep", 0.4)).await;
    assert_eq!(
        advisor.route("rename x", &entries()).await,
        RouteAdvice::Unsure {
            entry: "deep".to_string(),
            probability: 0.4,
        }
    );
}

#[cfg(feature = "jev")]
#[tokio::test]
async fn route_failure_is_reported() {
    let (advisor, _server) = advisor_answering(500, serde_json::json!({})).await;
    assert!(matches!(
        advisor.route("rename x", &entries()).await,
        RouteAdvice::Failed(_)
    ));
}

#[tokio::test]
async fn route_without_a_client_is_unavailable() {
    let advisor = Advisor::resolve(&Experimental::default());
    assert_eq!(
        advisor.route("rename x", &[]).await,
        RouteAdvice::Unavailable
    );
}
