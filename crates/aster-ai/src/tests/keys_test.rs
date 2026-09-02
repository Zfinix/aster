use super::*;

#[test]
fn provider_key_vars_are_keyed_on_the_host_not_the_whole_url() {
    assert_eq!(
        provider_key_vars("https://api.anthropic.com/v1"),
        ["ANTHROPIC_API_KEY"]
    );
    assert_eq!(
        provider_key_vars("https://openrouter.ai/api/v1/"),
        ["OPEN_ROUTER_API_KEY", "OPENROUTER_API_KEY"]
    );
    // A path mentioning a vendor must not be read as that vendor's endpoint.
    assert!(provider_key_vars("https://example.com/anthropic/v1").is_empty());
}

#[test]
fn an_endpoint_without_a_var_of_its_own_has_only_the_shared_one() {
    assert!(provider_key_vars("http://localhost:8080/v1").is_empty());
    assert_eq!(key_vars("http://localhost:8080/v1"), [SHARED_KEY_VAR]);
}

#[test]
fn baseten_has_a_var_of_its_own() {
    assert_eq!(
        key_vars("https://inference.baseten.co/v1"),
        ["BASETEN_API_KEY", SHARED_KEY_VAR]
    );
}

#[test]
fn key_vars_puts_the_endpoints_own_before_the_shared_one() {
    assert_eq!(
        key_vars("https://api.groq.com/openai/v1"),
        ["GROQ_API_KEY", SHARED_KEY_VAR]
    );
}

// A vendor-named var must never be offered to another vendor: that is the
// silent 401 this table exists to prevent.
#[test]
fn no_vendor_var_is_reachable_from_another_vendors_endpoint() {
    for (base_url, expected) in [
        ("https://api.anthropic.com/v1", "ANTHROPIC_API_KEY"),
        ("https://api.openai.com/v1", "OPENAI_API_KEY"),
        ("https://api.deepseek.com/v1", "DEEPSEEK_API_KEY"),
    ] {
        let vars = key_vars(base_url);
        assert_eq!(vars, [expected, SHARED_KEY_VAR], "{base_url}");
        assert!(!vars.contains(&"OPEN_ROUTER_API_KEY"), "{base_url}");
    }
}

#[test]
fn every_catalog_endpoint_that_takes_a_key_names_its_var() {
    let catalog: Catalog = serde_json::from_str(PROVIDERS_JSON).unwrap();
    let raw: serde_json::Value = serde_json::from_str(PROVIDERS_JSON).unwrap();
    for entry in &catalog.providers {
        if host_only(entry.base_url.trim_end_matches('/')).starts_with("localhost") {
            continue;
        }
        // Subscription endpoints authenticate by login, not a key.
        let takes_no_key = raw["providers"].as_array().unwrap().iter().any(|p| {
            p["base_url"] == entry.base_url
                && p["auth"].as_str().is_some_and(|a| a.contains("OAuth"))
        });
        if takes_no_key {
            continue;
        }
        assert!(
            !entry.key_env.is_empty(),
            "{} has no key_env",
            entry.base_url
        );
    }
}

#[test]
fn a_templated_host_matches_a_filled_in_one() {
    assert_eq!(
        provider_key_vars("https://my-resource.openai.azure.com/openai/v1"),
        ["AZURE_OPENAI_API_KEY"]
    );
    assert_eq!(
        provider_key_vars("https://bedrock-runtime.us-east-1.amazonaws.com/openai/v1"),
        ["AWS_BEARER_TOKEN_BEDROCK"]
    );
}

#[test]
fn azure_and_openai_do_not_collide() {
    assert_eq!(
        provider_key_vars("https://api.openai.com/v1"),
        ["OPENAI_API_KEY"]
    );
    assert_eq!(
        provider_key_vars("https://acme.openai.azure.com/openai/v1"),
        ["AZURE_OPENAI_API_KEY"]
    );
}

#[test]
fn the_catalogs_own_endpoints_all_resolve() {
    let catalog: Catalog = serde_json::from_str(PROVIDERS_JSON).unwrap();
    for entry in catalog.providers.iter().filter(|e| !e.key_env.is_empty()) {
        let url = entry
            .base_url
            .replace("{resource}", "acme")
            .replace("{region}", "us-east-1")
            .replace("{account_id}", "acct");
        assert_eq!(
            provider_key_vars(&url),
            entry.key_env.iter().map(String::as_str).collect::<Vec<_>>(),
            "{}",
            entry.base_url
        );
    }
}

#[test]
fn catalog_models_reads_the_codex_shortlist_and_skips_unknown_hosts() {
    assert_eq!(
        catalog_models("https://chatgpt.com/backend-api/codex"),
        ["gpt-5.6-terra"]
    );
    assert!(catalog_models("https://example.com/v1").is_empty());
}
