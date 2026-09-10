use super::*;

fn agent(max_output_tokens: Option<u32>) -> Agent {
    Agent {
        max_output_tokens,
        ..Agent::default()
    }
}

#[test]
fn the_output_cap_falls_back_to_the_default_and_yaml_moves_it() {
    assert_eq!(
        max_tokens_from(&agent(None), None),
        Some(aster_ai::DEFAULT_MAX_TOKENS)
    );
    assert_eq!(max_tokens_from(&agent(Some(2000)), None), Some(2000));
}

#[test]
fn the_environment_beats_yaml_and_zero_lifts_the_cap() {
    let configured = agent(Some(2000));
    assert_eq!(
        max_tokens_from(&configured, Some("1500".into())),
        Some(1500)
    );
    for lifted in ["0", "none", "off"] {
        assert_eq!(max_tokens_from(&configured, Some(lifted.into())), None);
    }
    assert_eq!(max_tokens_from(&agent(Some(0)), None), None);
}

#[test]
fn an_unparseable_environment_cap_falls_back_to_yaml() {
    assert_eq!(
        max_tokens_from(&agent(Some(2000)), Some("plenty".into())),
        Some(2000)
    );
    assert_eq!(
        max_tokens_from(&agent(None), Some("plenty".into())),
        Some(aster_ai::DEFAULT_MAX_TOKENS)
    );
}
