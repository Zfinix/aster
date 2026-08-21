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
        recommended: Vec::new(),
        auth: "Bearer".into(),
    };
    let local = Provider {
        auth: "none".into(),
        ..Provider {
            id: "o".into(),
            name: "O".into(),
            base_url: "http://localhost/v1".into(),
            example_model: "m".into(),
            recommended: Vec::new(),
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
    let openrouter = provider_recommended("https://openrouter.ai/api/v1");
    assert!(openrouter.len() > 1);
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
