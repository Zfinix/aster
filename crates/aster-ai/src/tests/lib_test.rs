use super::*;
use reqwest::StatusCode;

#[test]
fn format_api_error_prefers_openrouter_upstream_message() {
    let body = r#"{"error":{"message":"Provider returned error","code":429,"metadata":{"raw":"kimi is temporarily rate-limited upstream. Please retry shortly.","provider_name":"Moonshot AI"}}}"#;
    let msg = format_api_error(StatusCode::TOO_MANY_REQUESTS, body);
    assert_eq!(
        msg,
        "rate limited (429): kimi is temporarily rate-limited upstream. Please retry shortly."
    );
}

#[test]
fn format_api_error_falls_back_to_error_message() {
    let body = r#"{"error":{"message":"invalid model id"}}"#;
    assert_eq!(
        format_api_error(StatusCode::BAD_REQUEST, body),
        "bad request (400): invalid model id"
    );
}

#[test]
fn format_api_error_labels_auth_failures() {
    let msg = format_api_error(StatusCode::UNAUTHORIZED, "");
    assert_eq!(msg, "authentication failed (check your API key) (401)");
}

#[test]
fn format_api_error_uses_raw_body_when_not_json() {
    let msg = format_api_error(StatusCode::INTERNAL_SERVER_ERROR, "upstream down");
    assert_eq!(msg, "provider error (500): upstream down");
}
