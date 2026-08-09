use super::*;

#[test]
fn defaults_are_sensible() {
    let config = WebConfig::default();
    assert_eq!(config.defaults.timeout_ms, 120_000);
}

fn env_of(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> + use<> {
    let pairs: Vec<(String, String)> = pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    move |name| {
        pairs
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.clone())
    }
}

#[test]
fn resolve_key_prefers_first_var() {
    let lookup = env_of(&[
        ("CONTEXT_DEV_API_KEY", "ctxt_secret_dev"),
        ("CONTEXT_API_KEY", "ctxt_secret_fallback"),
    ]);
    assert_eq!(
        resolve_key(lookup, &["CONTEXT_DEV_API_KEY", "CONTEXT_API_KEY"]).as_deref(),
        Some("ctxt_secret_dev")
    );
}

#[test]
fn resolve_key_falls_back_to_second_var() {
    let lookup = env_of(&[("CONTEXT_API_KEY", "ctxt_secret_fallback")]);
    assert_eq!(
        resolve_key(lookup, &["CONTEXT_DEV_API_KEY", "CONTEXT_API_KEY"]).as_deref(),
        Some("ctxt_secret_fallback")
    );
}

#[test]
fn resolve_key_skips_blank() {
    let lookup = env_of(&[
        ("CONTEXT_DEV_API_KEY", "   "),
        ("CONTEXT_API_KEY", "ctxt_secret_fallback"),
    ]);
    assert_eq!(
        resolve_key(lookup, &["CONTEXT_DEV_API_KEY", "CONTEXT_API_KEY"]).as_deref(),
        Some("ctxt_secret_fallback")
    );
}

#[test]
fn resolve_key_returns_none_when_none_set() {
    assert!(resolve_key(env_of(&[]), &["FIRECRAWL_API_KEY"]).is_none());
}
