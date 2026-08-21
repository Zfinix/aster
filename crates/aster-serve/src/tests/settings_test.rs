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
    assert_eq!(settings.model, None);
    assert!(settings.provider.is_none());
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
    assert_eq!(settings.model.as_deref(), Some("a"));
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
fn an_endpoints_own_key_var_wins_over_nothing() {
    let provider = ProviderOverride {
        base_url: "https://x.test/v1".into(),
        key_env: vec!["ASTER_SERVE_TEST_KEY_UNSET".into()],
    };
    assert_eq!(
        provider.key(),
        None,
        "unset vars leave the CLI to fall back"
    );
}

#[test]
fn settings_survive_a_json_round_trip() {
    let mut settings = Settings::default();
    settings.remember_model("z/model", &VETTED);
    settings.provider = Some(ProviderOverride {
        base_url: "https://x.test/v1".into(),
        key_env: vec!["X_API_KEY".into()],
    });
    let json = serde_json::to_string(&settings).expect("serialize");
    let back: Settings = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.model.as_deref(), Some("z/model"));
    assert_eq!(back.provider.expect("provider").key_env, ["X_API_KEY"]);
}
