use super::*;

#[test]
fn codex_fast_mode_env_overrides_yaml_and_defaults_off() {
    let mut review = Review::default();
    assert_eq!(codex_fast_mode_from(&review, None), CodexFastMode::Disabled);
    review.codex_fast_mode = Some(true);
    assert_eq!(codex_fast_mode_from(&review, None), CodexFastMode::Enabled);
    for value in ["false", "0", "off", "no"] {
        assert_eq!(
            codex_fast_mode_from(&review, Some(value.into())),
            CodexFastMode::Disabled
        );
    }
    review.codex_fast_mode = Some(false);
    assert_eq!(codex_fast_mode_from(&review, None), CodexFastMode::Disabled);
    for value in ["true", "1", "yes", "on"] {
        assert_eq!(
            codex_fast_mode_from(&review, Some(value.into())),
            CodexFastMode::Enabled
        );
    }
}
