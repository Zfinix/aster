//! Tests for the Z.ai callback parsing, the one step that takes user input.

use super::*;

#[test]
fn state_round_trips_the_nonce() {
    let state = encode_state("abc123");
    assert_eq!(state_nonce(&state).as_deref(), Some("abc123"));
}

#[test]
fn authorize_url_carries_the_client_and_redirect() {
    let url = authorize_url(&encode_state("n")).unwrap();
    let pairs: Vec<(String, String)> = url
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    assert!(pairs.contains(&("client_id".into(), CLIENT_ID.into())));
    assert!(pairs.contains(&("redirect_uri".into(), REDIRECT_URI.into())));
    assert!(pairs.contains(&("response_type".into(), "code".into())));
}

#[test]
fn callback_code_reads_a_pasted_address() {
    let state = encode_state("n");
    let pasted = format!("  https://zcode.z.ai/login?code=xyz&state={state}\n");
    assert_eq!(callback_code(&pasted, "n").unwrap(), "xyz");
}

#[test]
fn callback_code_accepts_the_code_alone() {
    assert_eq!(callback_code("xyz", "n").unwrap(), "xyz");
}

#[test]
fn callback_code_refuses_another_attempts_state() {
    let state = encode_state("other");
    let pasted = format!("https://zcode.z.ai/login?code=xyz&state={state}");
    let err = callback_code(&pasted, "n").unwrap_err().to_string();
    assert!(err.contains("different sign-in"), "{err}");
}

#[test]
fn callback_code_surfaces_the_error_param() {
    let pasted = "https://zcode.z.ai/login?error=access_denied";
    let err = callback_code(pasted, "n").unwrap_err().to_string();
    assert!(err.contains("access_denied"), "{err}");
}

#[test]
fn sign_in_tries_the_zcode_jwt_before_the_access_token() {
    let tokens = sign_in_tokens(Some("zcode".into()), Some("zai".into()));
    assert_eq!(tokens, vec!["zcode".to_string(), "zai".to_string()]);
}

#[test]
fn sign_in_falls_back_to_the_access_token_alone() {
    let tokens = sign_in_tokens(None, clean(Some(" zai ".into())));
    assert_eq!(tokens, vec!["zai".to_string()]);
}

#[test]
fn sign_in_drops_blank_tokens() {
    let tokens = sign_in_tokens(clean(Some("  ".into())), clean(Some(String::new())));
    assert!(tokens.is_empty());
}

#[test]
fn expiry_is_a_deadline_not_a_lifetime() {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    assert!(expires_at(Some(3600)).unwrap() >= now + 3600);
    assert!(expires_at(None).is_none());
}

#[test]
fn a_sign_in_is_spent_a_skew_before_its_deadline() {
    let now = 1_000_000;
    assert!(is_spent(now, now));
    assert!(is_spent(now + REFRESH_SKEW, now));
    assert!(!is_spent(now + REFRESH_SKEW + 1, now));
}

#[test]
fn token_fields_names_the_envelope_without_its_values() {
    let raw = serde_json::json!({
        "code": 0,
        "data": { "token": "secret", "zai": { "access_token": "secret" }, "expires_in": 3600 },
    });
    let named = token_fields(&raw);
    assert!(named.contains("data: expires_in, token, zai"), "{named}");
    assert!(named.contains("data.zai: access_token"), "{named}");
    assert!(!named.contains("secret"), "{named}");
}

#[test]
fn token_fields_says_so_when_nothing_came_back() {
    assert_eq!(token_fields(&serde_json::json!("nope")), "nothing");
}
