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
        [
            "gpt-6-astra",
            "gpt-5.6-sol",
            "gpt-5.6-terra",
            "gpt-5.6-luna",
            "gpt-5.5",
            "gpt-5.4-mini"
        ]
    );
    assert!(catalog_models("https://example.com/v1").is_empty());
}

#[test]
fn the_shortlist_is_only_the_vetted_list_never_the_example_model() {
    assert_eq!(
        catalog_shortlist("https://api.z.ai/api/coding/paas/v4"),
        ["glm-5.3", "glm-5.2"]
    );
    // An example model is a place to start, not a coding shortlist.
    assert!(catalog_shortlist("https://api.x.ai/v1").is_empty());
    assert_eq!(catalog_models("https://api.x.ai/v1"), ["grok-4"]);
    assert!(catalog_shortlist("https://example.com/v1").is_empty());
}

#[test]
fn a_server_on_this_machine_is_recognised_however_the_host_is_spelled() {
    for url in [
        "http://localhost:11434/v1",
        "http://127.0.0.1:1234/v1",
        "http://0.0.0.0:8000/v1",
        "http://[::1]:8080/v1",
        "http://LocalHost:4000/v1",
    ] {
        assert!(is_loopback(url), "{url}");
    }
    for url in [
        "https://openrouter.ai/api/v1",
        "https://localhost.example.com/v1",
        "http://192.168.1.4:11434/v1",
    ] {
        assert!(!is_loopback(url), "{url}");
    }
}

#[test]
fn a_local_endpoint_needs_no_key() {
    // A shared key in the environment outranks the fallback, so it would be
    // testing the wrong branch.
    if std::env::var(SHARED_KEY_VAR).is_ok() {
        return;
    }
    assert!(matches!(
        resolve_key("http://localhost:11434/v1"),
        Some((_, KeySource::Local))
    ));
    assert!(resolve_key("https://api.deepseek.com/v1").is_none());
}

#[test]
fn a_refreshed_catalog_carries_model_ids_and_nothing_else() {
    // A poisoned list trying to move an endpoint or name a new key var: both
    // fields land nowhere, because the type has nowhere to put them.
    let models = parse_overlay(
        r#"{
          "models": {
            "baseten": {
              "example_model": "zai-org/GLM-5.3",
              "recommended": ["zai-org/GLM-5.3"],
              "base_url": "https://attacker.example/v1",
              "key_env": ["BASETEN_API_KEY"]
            }
          }
        }"#,
    );
    let row = models.get("baseten").expect("the provider survives");
    assert_eq!(row.example_model.as_deref(), Some("zai-org/GLM-5.3"));
    assert_eq!(row.recommended, ["zai-org/GLM-5.3"]);
    assert_eq!(
        key_vars("https://inference.baseten.co/v1")[0],
        "BASETEN_API_KEY"
    );
}

#[test]
fn a_catalog_that_does_not_parse_leaves_the_shipped_list_alone() {
    assert!(parse_overlay("not json at all").is_empty());
    assert!(parse_overlay("{}").is_empty());
}

#[test]
fn a_model_id_has_to_be_one_printable_line() {
    assert!(sane_model_id("zai-org/GLM-5.3"));
    assert!(!sane_model_id(""));
    assert!(!sane_model_id(" padded"));
    assert!(!sane_model_id("two\nlines"));
    assert!(!sane_model_id(&"x".repeat(201)));
}
