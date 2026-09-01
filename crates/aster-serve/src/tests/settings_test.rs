use super::*;

const VETTED: [&str; 2] = ["anthropic/claude-opus-5", "anthropic/claude-sonnet-5"];

#[test]
fn defaults_leave_the_repo_in_charge() {
    let settings = Settings::default();
    assert_eq!(settings.permission_mode, "edit");
    assert_eq!(
        settings.effort, None,
        "unset effort keeps aster.yaml deciding"
    );
}

#[test]
fn a_hand_typed_model_is_kept_alongside_the_vetted_ones() {
    let mut settings = Settings::default();
    settings.remember_model("local/my-model", &VETTED);
    assert_eq!(settings.custom_models, ["local/my-model"]);

    settings.remember_model("anthropic/claude-opus-5", &VETTED);
    assert_eq!(
        settings.custom_models,
        ["local/my-model"],
        "a catalog model is not a custom one"
    );
}

#[test]
fn recents_are_most_recent_first_without_repeats() {
    let mut settings = Settings::default();
    for model in ["a", "b", "c", "a"] {
        settings.remember_model(model, &VETTED);
    }
    assert_eq!(settings.recent_models, ["a", "c", "b"]);
    assert_eq!(settings.custom_models, ["a", "b", "c"], "each is kept once");
}

#[test]
fn recents_stay_short_enough_for_a_picker() {
    let mut settings = Settings::default();
    for model in ["a", "b", "c", "d", "e", "f", "g"] {
        settings.remember_model(model, &VETTED);
    }
    assert_eq!(settings.recent_models, ["g", "f", "e", "d", "c"]);
}

#[test]
fn settings_survive_a_json_round_trip() {
    let mut settings = Settings::default();
    settings.remember_model("z/model", &VETTED);
    settings.effort = Some("low".into());
    let json = serde_json::to_string(&settings).expect("serialize");
    let back: Settings = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.recent_models, ["z/model"]);
    assert_eq!(back.effort.as_deref(), Some("low"));
}

#[test]
fn a_file_with_a_retired_override_still_loads() {
    // serve.json used to carry the model and a provider override; the config
    // owns both now, and an old file must not fail to parse over them.
    let legacy = r#"{
        "permissionMode": "auto",
        "model": "deepseek-v4-pro",
        "customModels": ["stealth/ox-alpha"],
        "recentModels": ["deepseek-v4-pro"],
        "effort": "low",
        "provider": { "baseUrl": "https://api.deepseek.com/v1", "keyEnv": ["DEEPSEEK_API_KEY"] }
    }"#;
    let back: Settings = serde_json::from_str(legacy).expect("legacy file parses");
    assert_eq!(back.permission_mode, "auto");
    assert_eq!(back.custom_models, ["stealth/ox-alpha"]);
    assert_eq!(back.effort.as_deref(), Some("low"));
}
