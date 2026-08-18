use super::Redactor;

#[test]
fn redact_replaces_known_secret_values() {
    let r = Redactor::new(vec!["sk-or-v1-secretvalue".into()]);
    assert_eq!(
        r.redact("key is sk-or-v1-secretvalue ok"),
        "key is [redacted] ok"
    );
}

#[test]
fn redact_replaces_longest_secret_first() {
    // Unsorted input: "abcdefgh" is a prefix of "abcdefghijkl", so replacing the
    // shorter one first would leave a visible tail behind.
    let r = Redactor::new(vec!["abcdefgh".into(), "abcdefghijkl".into()]);
    assert_eq!(r.redact("xabcdefghijkly"), "x[redacted]y");
}

#[test]
fn redact_leaves_unrelated_text_alone() {
    let r = Redactor::new(vec!["sk-or-v1-secretvalue".into()]);
    assert_eq!(r.redact("no secrets here"), "no secrets here");
}

#[test]
fn short_values_are_not_redacted() {
    let r = Redactor::new(vec!["short".into()]);
    assert_eq!(r.redact("a short word"), "a short word");
}

#[test]
fn secret_names_are_recognized() {
    for name in [
        "ASTER_API_KEY",
        "OPEN_ROUTER_API_KEY",
        "ANTHROPIC_API_KEY",
        "GITHUB_TOKEN",
        "CLIENT_SECRET",
        "DB_PASSWORD",
        "PRIVATE_KEY",
    ] {
        assert!(super::is_secret_name(name), "{name}");
    }
}

#[test]
fn ordinary_names_are_not_secrets() {
    for name in ["PWD", "MONKEY", "HOME", "USER", "EDITOR"] {
        assert!(!super::is_secret_name(name), "{name}");
    }
}

#[test]
fn env_lines_parse_names_and_values() {
    assert_eq!(
        super::parse_env_line("ASTER_API_KEY=sk-123"),
        Some(("ASTER_API_KEY".into(), "sk-123".into()))
    );
    assert_eq!(
        super::parse_env_line("QUOTED=\"abc\""),
        Some(("QUOTED".into(), "abc".into()))
    );
    assert_eq!(
        super::parse_env_line("SINGLE='abc'"),
        Some(("SINGLE".into(), "abc".into()))
    );
    assert_eq!(super::parse_env_line("# comment"), None);
    assert_eq!(super::parse_env_line(""), None);
    assert_eq!(super::parse_env_line("NO_EQ"), None);
}
