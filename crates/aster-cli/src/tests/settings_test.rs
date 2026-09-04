use super::*;
use aster_policy::{Action, Decision, Policy};

#[test]
fn review_key_rewrite_keeps_comments() {
    let yaml = "\
# reviewed by the team
review:
  base_url: https://openrouter.ai/api/v1
  model: old-model  # picked long ago
permissions:
  mode: manual
";
    let out = with_key(yaml, "review", "model", "new-model");
    assert!(out.contains("  model: new-model"), "{out}");
    assert!(!out.contains("old-model"), "{out}");
    assert!(out.contains("# reviewed by the team"), "{out}");
    assert!(out.contains("mode: manual"), "{out}");
}

#[test]
fn review_key_is_added_inside_an_existing_block() {
    let yaml = "review:\n  base_url: https://x.test/v1\npermissions:\n  mode: manual\n";
    let out = with_key(yaml, "review", "model", "m1");
    let review = out.find("review:").unwrap();
    let perms = out.find("permissions:").unwrap();
    let model = out.find("  model: m1").unwrap();
    assert!(review < model && model < perms, "{out}");
}

#[test]
fn review_block_is_created_when_the_file_lacks_one() {
    assert_eq!(
        with_key("", "review", "model", "m1"),
        "review:\n  model: m1\n"
    );
    let out = with_key("permissions:\n  mode: edit\n", "review", "model", "m1");
    assert!(out.ends_with("review:\n  model: m1\n"), "{out}");
    assert!(out.starts_with("permissions:"), "{out}");
}

#[test]
fn permissions_absent_defaults_to_permissive_edits() {
    let s: Settings = serde_yaml::from_str("review: {}").expect("parse");
    let p = Policy::compile(&s.permissions).expect("compile");
    assert_eq!(
        p.evaluate(&Action::Edit {
            path: "src/main.rs"
        }),
        Decision::Allow
    );
    assert!(matches!(
        p.evaluate(&Action::Edit {
            path: ".git/hooks/pre-commit"
        }),
        Decision::Prompt { .. }
    ));
}

#[test]
fn permissions_block_parses_and_compiles() {
    let yaml = "\
permissions:
  mode: manual
  deny: [\"Edit(**/*.pem)\"]
  allow: [\"Edit(src/**)\"]
";
    let s: Settings = serde_yaml::from_str(yaml).expect("parse permissions block");
    let p = Policy::compile(&s.permissions).expect("compile");
    assert!(matches!(
        p.evaluate(&Action::Edit {
            path: "certs/key.pem"
        }),
        Decision::Deny { .. }
    ));
    assert_eq!(
        p.evaluate(&Action::Edit {
            path: "src/main.rs"
        }),
        Decision::Allow
    );
    // `manual` prompts for anything no rule matched.
    assert!(matches!(
        p.evaluate(&Action::Edit {
            path: "docs/readme.md"
        }),
        Decision::Prompt { .. }
    ));
}

#[test]
fn a_retired_permissions_key_is_an_error() {
    for key in [
        "protected: []",
        "secret_read: []",
        "allow_exec: []",
        "deny_exec: []",
        "use_default_protected: true",
    ] {
        let yaml = format!("permissions:\n  {key}\n");
        let parsed: Result<Settings, _> = serde_yaml::from_str(&yaml);
        assert!(parsed.is_err(), "{key} should no longer parse");
    }
}

#[test]
fn effort_parses_and_overlays_from_the_project_file() {
    let global: Settings = serde_yaml::from_str("review:\n  effort: high\n").expect("parse");
    assert_eq!(global.review.effort, Some(aster_ai::Effort::High));

    let project: Settings = serde_yaml::from_str("review:\n  effort: off\n").expect("parse");
    let merged = global.overlaid_with(project);
    assert_eq!(merged.review.effort, Some(aster_ai::Effort::Off));
}

#[test]
fn repo_mcp_json_servers_are_read_natively_and_yaml_wins_collisions() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(".mcp.json"),
        r#"{"mcpServers": {
                "shared-xyz": {"command": "npx", "args": ["-y", "pkg"], "env": {"K": "v"}},
                "yaml-owned-xyz": {"command": "wrong"},
                "remote-xyz": {"type": "http", "url": "http://x/mcp"}
            }}"#,
    )
    .unwrap();
    std::fs::write(
        dir.path().join("aster.yaml"),
        "mcp:\n  servers:\n    yaml-owned-xyz:\n      command: right\n",
    )
    .unwrap();

    let settings = Settings::load(Some(dir.path())).unwrap();
    let shared = &settings.mcp.servers["shared-xyz"];
    assert_eq!(shared.command, "npx");
    assert_eq!(shared.args, ["-y", "pkg"]);
    assert_eq!(shared.env["K"], "v");
    assert_eq!(settings.mcp.servers["yaml-owned-xyz"].command, "right");
    let remote = &settings.mcp.servers["remote-xyz"];
    assert_eq!(remote.url, "http://x/mcp");
    assert_eq!(
        remote.transport(),
        Some(crate::mcp::Transport::StreamableHttp)
    );
}

#[test]
fn effort_absent_leaves_the_client_default() {
    let s: Settings = serde_yaml::from_str("review: {}").expect("parse");
    assert_eq!(s.review.effort, None);
}

#[test]
fn web_search_parses_and_overlays_from_the_project_file() {
    let global: Settings = serde_yaml::from_str("review:\n  web_search: true\n").expect("parse");
    assert_eq!(global.review.web_search, Some(true));

    let project: Settings = serde_yaml::from_str("review:\n  web_search: false\n").expect("parse");
    let merged = global.overlaid_with(project);
    assert_eq!(merged.review.web_search, Some(false));
}

#[test]
fn web_search_absent_leaves_none() {
    let s: Settings = serde_yaml::from_str("review: {}").expect("parse");
    assert_eq!(s.review.web_search, None);
}

#[test]
fn pins_sees_only_a_key_the_review_block_sets_itself() {
    let sets = "review:\n  model: pinned\n  base_url: https://x/v1\n";
    assert!(pins(sets, "review", "model"));
    assert!(pins(sets, "review", "base_url"));
    assert!(!pins(sets, "review", "effort"));

    let elsewhere = "review:\n  effort: high\npermissions:\n  model: no\n";
    assert!(!pins(elsewhere, "review", "model"));
    assert!(!pins("", "review", "model"));
}

#[test]
fn a_repo_that_pins_the_model_is_moved_along_with_the_global_choice() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("aster.yaml");
    std::fs::write(&project, "review:\n  model: old\n  min_confidence: 0.9\n").unwrap();

    write_review(&project, &[("model", "new")]).unwrap();
    let out = std::fs::read_to_string(&project).unwrap();
    assert!(out.contains("model: new"), "{out}");
    // Everything the repo set for itself survives the switch.
    assert!(out.contains("min_confidence: 0.9"), "{out}");
}
