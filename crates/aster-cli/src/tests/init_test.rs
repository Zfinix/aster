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
fn env_has_key_matches_only_exact_key() {
    let dir = tempfile::tempdir().unwrap();
    let env = dir.path().join(".env");
    fs::write(&env, "ASTER_API_KEY=sk-123\nOTHER=1\n").unwrap();
    assert!(env_has_key(&env, "ASTER_API_KEY"));
    assert!(!env_has_key(&env, "ASTER_BASE_URL"));
}

#[test]
fn append_line_adds_trailing_newline_when_missing() {
    let dir = tempfile::tempdir().unwrap();
    let f = dir.path().join(".env");
    fs::write(&f, "A=1").unwrap();
    append_line(&f, "B=2").unwrap();
    assert_eq!(fs::read_to_string(&f).unwrap(), "A=1\nB=2\n");
}
