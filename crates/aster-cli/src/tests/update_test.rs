use super::*;

#[test]
fn version_triples_are_read_through_any_tag_prefix() {
    assert_eq!(version_triple("0.3.0"), Some((0, 3, 0)));
    assert_eq!(version_triple("v0.2.0"), Some((0, 2, 0)));
    assert_eq!(version_triple("cli-v0.3.1"), Some((0, 3, 1)));
    assert_eq!(version_triple("v1.0.0-rc1"), Some((1, 0, 0)));
    assert_eq!(version_triple("nightly"), None);
}

#[test]
fn newer_means_strictly_greater() {
    assert!(is_newer("cli-v0.4.0", "0.3.0"));
    assert!(is_newer("v0.3.1", "0.3.0"));
    assert!(!is_newer("v0.3.0", "0.3.0"));
    assert!(!is_newer("v0.2.9", "0.3.0"));
    assert!(!is_newer("nightly", "0.3.0"));
}

#[test]
fn the_shown_version_drops_the_tag_prefix() {
    assert_eq!(trimmed_version("cli-v0.4.0"), "0.4.0");
    assert_eq!(trimmed_version("v0.4.0"), "0.4.0");
    assert_eq!(trimmed_version("0.4.0"), "0.4.0");
}

#[test]
fn the_compare_range_starts_at_the_running_version_tag() {
    let releases = vec![
        serde_json::json!({ "tag_name": "cli-v0.4.0" }),
        serde_json::json!({ "tag_name": "cli-v0.3.0" }),
        serde_json::json!({ "tag_name": "v0.2.0" }),
    ];
    assert_eq!(tag_for(&releases, (0, 3, 0)), Some("cli-v0.3.0".into()));
    assert_eq!(tag_for(&releases, (0, 2, 0)), Some("v0.2.0".into()));
    assert_eq!(tag_for(&releases, (0, 1, 0)), None);
}
