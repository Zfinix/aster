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
  mode: auto
";
    let out = with_review_key(yaml, "model", "new-model");
    assert!(out.contains("  model: new-model"), "{out}");
    assert!(!out.contains("old-model"), "{out}");
    assert!(out.contains("# reviewed by the team"), "{out}");
    assert!(out.contains("mode: auto"), "{out}");
}

#[test]
fn review_key_is_added_inside_an_existing_block() {
    let yaml = "review:\n  base_url: https://x.test/v1\npermissions:\n  mode: auto\n";
    let out = with_review_key(yaml, "model", "m1");
    let review = out.find("review:").unwrap();
    let perms = out.find("permissions:").unwrap();
    let model = out.find("  model: m1").unwrap();
    assert!(review < model && model < perms, "{out}");
}

#[test]
fn review_block_is_created_when_the_file_lacks_one() {
    assert_eq!(with_review_key("", "model", "m1"), "review:\n  model: m1\n");
    let out = with_review_key("permissions:\n  mode: auto\n", "model", "m1");
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
        Decision::Deny { .. }
    ));
}

#[test]
fn permissions_block_parses_and_compiles() {
    let yaml = "\
permissions:
  mode: ask
  deny: [\"**/*.pem\"]
  allow: [\"src/**\"]
";
    let s: Settings = serde_yaml::from_str(yaml).expect("parse permissions block");
    let p = Policy::compile(&s.permissions).expect("compile");
    assert!(matches!(
        p.evaluate(&Action::Edit {
            path: "certs/key.pem"
        }),
        Decision::Deny { .. }
    ));
    // The legacy `ask` name is `manual`, which prompts for unmatched paths.
    assert!(matches!(
        p.evaluate(&Action::Edit {
            path: "docs/readme.md"
        }),
        Decision::Prompt { .. }
    ));
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
