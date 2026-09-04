use super::*;

#[test]
fn catalog_parses_and_has_openrouter() {
    let providers = load_providers().expect("embedded providers.json parses");
    assert!(providers.len() > 10);
    assert_eq!(default_provider(&providers).id, "openrouter");
}

#[test]
fn needs_key_follows_auth() {
    let cloud = Provider {
        id: "x".into(),
        name: "X".into(),
        base_url: "https://x/v1".into(),
        example_model: "m".into(),
        auth: "Bearer".into(),
    };
    let local = Provider {
        auth: "none".into(),
        ..Provider {
            id: "o".into(),
            name: "O".into(),
            base_url: "http://localhost/v1".into(),
            example_model: "m".into(),
            auth: String::new(),
        }
    };
    assert!(cloud.needs_key());
    assert!(!local.needs_key());
}

#[test]
fn templated_detects_placeholder() {
    let providers = load_providers().unwrap();
    let azure = providers.iter().find(|p| p.id == "azure_openai").unwrap();
    let groq = providers.iter().find(|p| p.id == "groq").unwrap();
    assert!(azure.templated());
    assert!(!groq.templated());
}

#[test]
fn yaml_contains_selected_provider() {
    let y = yaml_contents("http://localhost:11434/v1", "qwen2.5-coder");
    assert!(y.contains("base_url: http://localhost:11434/v1"));
    assert!(y.contains("model: qwen2.5-coder"));
}

#[test]
fn the_scaffolded_config_parses_with_the_browser_off_and_its_keyed_tools_denied() {
    let y = yaml_contents("http://localhost:11434/v1", "qwen2.5-coder");
    let settings: crate::settings::Settings =
        serde_yaml::from_str(&y).expect("the config Aster writes must be one it can read");

    let browser = settings
        .mcp
        .servers
        .get("browser")
        .expect("browser is scaffolded");
    assert!(browser.disabled, "the browser must be opt-in");
    assert_eq!(browser.command, "uvx");
    assert_eq!(browser.env["ANONYMIZED_TELEMETRY"], "False");

    assert!(
        settings
            .mcp
            .tools
            .deny
            .contains(&"browser/retry_with_browser_use_agent".to_string()),
        "{:?}",
        settings.mcp.tools.deny
    );
}

#[test]
fn env_has_key_matches_only_exact_key() {
    let dir = tempfile::tempdir().unwrap();
    let env = dir.path().join(".env");
    fs::write(&env, "ASTER_API_KEY=sk-123\nOTHER=1\n").unwrap();
    assert!(env_has_key(&env, "ASTER_API_KEY"));
    assert!(!env_has_key(&env, "ASTER_BASE_URL"));
}

#[test]
fn find_provider_accepts_an_id_a_name_or_a_url() {
    let by_id = find_provider("openai").unwrap();
    assert_eq!(by_id.1, "https://api.openai.com/v1");
    assert_eq!(find_provider("OpenAI").unwrap().1, by_id.1);
    assert_eq!(
        find_provider("https://api.openai.com/v1/").unwrap().1,
        by_id.1
    );
    // The example model travels with it, so a switch never leaves the endpoint
    // paired with a model the last provider served.
    assert!(!by_id.2.is_empty());
}

#[test]
fn find_provider_takes_an_unknown_url_at_face_value() {
    let (_, base_url, model) = find_provider("http://127.0.0.1:9999/v1").unwrap();
    assert_eq!(base_url, "http://127.0.0.1:9999/v1");
    // Nothing to adopt, which is why `use` demands --model for these.
    assert!(model.is_empty());
    assert!(find_provider("not-a-provider").is_err());
}

#[test]
fn find_provider_refuses_endpoints_with_an_unfilled_placeholder() {
    assert!(find_provider("azure_openai").is_err());
}

#[test]
fn recommended_falls_back_to_the_example_model() {
    // OpenRouter's shortlist is live now (the benchmark router), so the
    // catalog only carries its example model.
    let openrouter = provider_recommended("https://openrouter.ai/api/v1");
    assert_eq!(openrouter, ["deepseek/deepseek-v4-pro"]);
    // No shortlist of its own: the example model is the whole answer.
    assert_eq!(provider_recommended("https://api.x.ai/v1"), ["grok-4"]);
    assert!(provider_recommended("https://nobody.example/v1").is_empty());
}

#[test]
fn lookup_prefers_an_exact_url_over_a_shared_host() {
    // Both are openrouter.ai; the exact entry must win regardless of order.
    assert_eq!(provider_label("https://openrouter.ai/api/v1"), "OpenRouter");
    assert_eq!(provider_label("https://openrouter.ai/other"), "OpenRouter");
}

#[test]
fn append_line_adds_trailing_newline_when_missing() {
    let dir = tempfile::tempdir().unwrap();
    let f = dir.path().join(".env");
    fs::write(&f, "A=1").unwrap();
    append_line(&f, "B=2").unwrap();
    assert_eq!(fs::read_to_string(&f).unwrap(), "A=1\nB=2\n");
}

#[test]
fn remove_env_key_drops_only_its_line_and_reports_presence() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join(".env");
    set_env_key(&path, "OPEN_ROUTER_API_KEY", "sk-or-abc").unwrap();
    set_env_key(&path, "OTHER_KEY", "keep-me").unwrap();

    assert!(remove_env_key(&path, "OPEN_ROUTER_API_KEY").unwrap());
    let text = fs::read_to_string(&path).unwrap();
    assert!(!text.contains("OPEN_ROUTER_API_KEY"));
    assert!(text.contains("OTHER_KEY=keep-me"));

    assert!(!remove_env_key(&path, "OPEN_ROUTER_API_KEY").unwrap());
    assert!(!remove_env_key(&tmp.path().join("missing.env"), "X").unwrap());
}

#[test]
fn a_web_key_prompt_names_the_var_only_when_the_provider_needs_two() {
    assert_eq!(
        web_prompt_label("Firecrawl", "FIRECRAWL_API_KEY", 1),
        "Firecrawl API key"
    );
    // Asked twice for Cloudflare, the var name is the only thing telling the
    // account id from the token.
    assert_eq!(
        web_prompt_label("Cloudflare Browser Rendering", "CLOUDFLARE_BR_API_TOKEN", 2),
        "Cloudflare Browser Rendering · CLOUDFLARE_BR_API_TOKEN"
    );
}

#[test]
fn every_web_provider_can_be_prompted_for() {
    // The wizard builds a prompt for each var the catalog names, so a provider
    // added to aster-web is askable without touching init.
    for (name, vars) in crate::config::key::web_providers() {
        for (var, _) in &vars {
            let label = web_prompt_label(name, var, vars.len());
            assert!(label.contains(name), "{label}");
            assert!(!label.is_empty());
        }
    }
}

#[test]
fn a_custom_endpoint_takes_a_key_and_stores_it_in_the_shared_var() {
    // The synthesized row: no auth note means the key prompt is offered, and an
    // off-catalog host resolves to the one var that crosses endpoints.
    let custom = Provider {
        id: "custom".to_string(),
        name: "Custom endpoint".to_string(),
        base_url: "http://localhost:8080/v1".to_string(),
        example_model: String::new(),
        auth: String::new(),
    };
    assert!(custom.needs_key());
    assert!(!custom.templated());
    assert_eq!(key_var_for(&custom.base_url), keys::SHARED_KEY_VAR);

    // A hosted endpoint the catalog does not know is custom too, and must not
    // borrow a var from a catalog vendor that merely shares a suffix.
    let aliyun = "https://ws-26w9qup24v8mief7.ap-southeast-1.maas.aliyuncs.com/compatible-mode/v1";
    assert_eq!(key_var_for(aliyun), keys::SHARED_KEY_VAR);
    let providers = load_providers().expect("catalog parses");
    assert!(!providers.iter().any(|p| p.base_url == aliyun));
}
