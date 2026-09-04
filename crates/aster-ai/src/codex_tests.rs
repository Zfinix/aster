use super::*;
use sha2::{Digest, Sha256};
use std::fs;

fn temp_home() -> tempfile::TempDir {
    tempfile::tempdir().expect("tempdir")
}

/// A minimal unsigned JWT: header.payload with the given claims, no signature.
fn fake_jwt(claims: &serde_json::Value) -> String {
    let enc = |v: &serde_json::Value| {
        base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(serde_json::to_vec(v).expect("serialize"))
    };
    format!(
        "{}.{}.sig",
        enc(&serde_json::json!({"alg": "none"})),
        enc(claims)
    )
}

#[test]
fn codex_cli_auth_json_round_trips() {
    let json = r#"{
        "OPENAI_API_KEY": null,
        "tokens": {
            "id_token": "id",
            "access_token": "at",
            "refresh_token": "rt",
            "account_id": "acc"
        },
        "last_refresh": "2026-08-01T00:00:00Z"
    }"#;
    let auth: CodexAuth = serde_json::from_str(json).expect("parse");
    assert_eq!(
        auth.tokens.as_ref().expect("tokens").account_id.as_deref(),
        Some("acc")
    );
}

#[test]
fn load_prefers_aster_store_over_codex_cli() {
    let home = temp_home();
    let cli = CodexAuth {
        tokens: Some(TokenSet {
            id_token: "cli-id".into(),
            access_token: "cli-at".into(),
            refresh_token: "cli-rt".into(),
            account_id: None,
        }),
        ..Default::default()
    };
    fs::create_dir_all(codex_cli_path(home.path()).parent().unwrap()).unwrap();
    fs::write(
        codex_cli_path(home.path()),
        serde_json::to_vec(&cli).unwrap(),
    )
    .unwrap();

    let imported = load(home.path()).expect("imported");
    assert_eq!(imported.tokens.unwrap().access_token, "cli-at");

    let own = CodexAuth {
        tokens: Some(TokenSet {
            id_token: "own-id".into(),
            access_token: "own-at".into(),
            refresh_token: "own-rt".into(),
            account_id: None,
        }),
        ..Default::default()
    };
    save(home.path(), &own).unwrap();
    let loaded = load(home.path()).expect("loaded");
    assert_eq!(loaded.tokens.unwrap().access_token, "own-at");
}

#[test]
fn load_returns_none_when_neither_store_exists() {
    let home = temp_home();
    assert!(load(home.path()).is_none());
}

#[test]
fn clear_removes_only_the_aster_store() {
    let home = temp_home();
    let auth = CodexAuth::default();
    save(home.path(), &auth).unwrap();
    assert!(clear(home.path()));
    assert!(!clear(home.path()));
    assert!(!store_path(home.path()).exists());
}

#[test]
fn expired_reads_jwt_exp_and_defaults_to_expired() {
    let later = now_unix() + 3600;
    assert!(!expired(&fake_jwt(&serde_json::json!({ "exp": later }))));
    assert!(expired(&fake_jwt(
        &serde_json::json!({ "exp": now_unix() })
    )));
    assert!(expired("not-a-jwt"));
}

#[test]
fn authorize_url_encodes_every_parameter() {
    let url = authorize_url("chall", "state-with-entropy").unwrap();
    assert!(!url.as_str().contains(' '));
    let param = |name: &str| {
        url.query_pairs()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.into_owned())
            .unwrap()
    };
    assert_eq!(param("scope"), SCOPES);
    assert_eq!(param("redirect_uri"), REDIRECT_URI);
    assert_eq!(param("state"), "state-with-entropy");
    assert_eq!(param("code_challenge"), "chall");
}

#[test]
fn callback_accepts_the_matching_redirect_and_decodes_the_code() {
    let req =
        "GET /auth/callback?code=abc%2F123&state=expected HTTP/1.1\r\nHost: localhost\r\n\r\n";
    match parse_callback(req, "expected") {
        Callback::Code(code) => assert_eq!(code, "abc/123"),
        _ => panic!("expected a code"),
    }
}

#[test]
fn callback_refuses_a_state_mismatch() {
    let req = "GET /auth/callback?code=abc&state=evil HTTP/1.1\r\n\r\n";
    assert!(matches!(
        parse_callback(req, "expected"),
        Callback::Refused(_)
    ));
}

#[test]
fn callback_surfaces_the_provider_error() {
    let req =
        "GET /auth/callback?error=invalid_state&error_description=state+too+short HTTP/1.1\r\n\r\n";
    match parse_callback(req, "expected") {
        Callback::Refused(reason) => {
            assert!(reason.contains("invalid_state"));
            assert!(reason.contains("state too short"));
        }
        _ => panic!("expected a refusal"),
    }
}

#[test]
fn callback_ignores_probes_and_empty_reads() {
    assert!(matches!(
        parse_callback("GET /favicon.ico HTTP/1.1\r\n\r\n", "expected"),
        Callback::NotTheRedirect
    ));
    assert!(matches!(
        parse_callback("", "expected"),
        Callback::NotTheRedirect
    ));
}

#[test]
fn pkce_challenge_is_verifier_sha256() {
    let pair = crate::pkce::pkce();
    assert!((43..=128).contains(&pair.verifier.len()));
    let expected = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(Sha256::digest(pair.verifier.as_bytes()));
    assert_eq!(pair.challenge, expected);
}

fn tokens(access: &str, refresh: &str) -> CodexAuth {
    CodexAuth {
        tokens: Some(TokenSet {
            id_token: "id".into(),
            access_token: access.into(),
            refresh_token: refresh.into(),
            account_id: Some("acc".into()),
        }),
        ..Default::default()
    }
}

#[test]
fn store_lives_in_the_data_dir() {
    let home = temp_home();
    assert_eq!(
        store_path(home.path()),
        home.path().join(".local/share/aster/codex.json")
    );
}

#[test]
fn load_moves_a_legacy_store_to_the_current_path() {
    let home = temp_home();
    let misplaced = home.path().join(".local/share/aster/.aster/codex.json");
    fs::create_dir_all(misplaced.parent().unwrap()).unwrap();
    fs::write(
        &misplaced,
        serde_json::to_vec(&tokens("old-at", "old-rt")).unwrap(),
    )
    .unwrap();

    let loaded = load(home.path()).expect("loaded");
    assert_eq!(loaded.tokens.unwrap().access_token, "old-at");
    assert!(store_path(home.path()).exists());
    assert!(!misplaced.exists());
}

#[tokio::test]
async fn refresh_stores_the_new_tokens_and_keeps_the_old_refresh_token() {
    use wiremock::matchers::{body_string_contains, method};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(body_string_contains("grant_type=refresh_token"))
        .and(body_string_contains("refresh_token=old-rt"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "new-at",
            "id_token": "new-id",
        })))
        .mount(&server)
        .await;

    let home = temp_home();
    let mut auth = tokens("old-at", "old-rt");
    refresh_with(&server.uri(), home.path(), &mut auth)
        .await
        .expect("refresh");

    let stored = load(home.path()).expect("stored").tokens.unwrap();
    assert_eq!(stored.access_token, "new-at");
    assert_eq!(stored.id_token, "new-id");
    assert_eq!(stored.refresh_token, "old-rt");
    assert_eq!(stored.account_id.as_deref(), Some("acc"));
}

#[tokio::test]
async fn refresh_rejection_asks_for_a_new_login() {
    use wiremock::matchers::method;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(400).set_body_string("invalid_grant"))
        .mount(&server)
        .await;

    let home = temp_home();
    let mut auth = tokens("old-at", "old-rt");
    let err = refresh_with(&server.uri(), home.path(), &mut auth)
        .await
        .expect_err("rejected");
    assert!(err.to_string().contains("aster login codex"), "{err}");
    assert!(load(home.path()).is_none());
}
