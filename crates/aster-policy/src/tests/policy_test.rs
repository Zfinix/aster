use super::*;

fn policy(cfg: PermissionsConfig) -> Policy {
    Policy::compile(&cfg).expect("compile")
}

fn cfg() -> PermissionsConfig {
    PermissionsConfig::default()
}

fn is_deny(d: &Decision) -> bool {
    matches!(d, Decision::Deny { .. })
}

#[test]
fn evaluate_protected_denies_git() {
    let p = policy(cfg());
    assert!(is_deny(&p.evaluate(&Action::Edit {
        path: ".git/hooks/pre-commit"
    })));
}

#[test]
fn evaluate_protected_denies_workflows() {
    let p = policy(cfg());
    assert!(is_deny(&p.evaluate(&Action::Edit {
        path: ".github/workflows/ci.yml"
    })));
}

#[test]
fn evaluate_protected_overridden_by_allow() {
    let mut c = cfg();
    c.allow = vec![".github/workflows/**".to_string()];
    let p = policy(c);
    assert_eq!(
        p.evaluate(&Action::Edit {
            path: ".github/workflows/ci.yml"
        }),
        Decision::Allow
    );
}

#[test]
fn evaluate_allow_glob_allows() {
    let mut c = cfg();
    c.mode = Mode::Manual;
    c.allow = vec!["src/**".to_string()];
    let p = policy(c);
    assert_eq!(
        p.evaluate(&Action::Edit {
            path: "src/main.rs"
        }),
        Decision::Allow
    );
}

#[test]
fn evaluate_deny_glob_denies() {
    let mut c = cfg();
    c.deny = vec!["**/*.pem".to_string()];
    let p = policy(c);
    assert!(is_deny(&p.evaluate(&Action::Edit {
        path: "certs/server.pem"
    })));
}

#[test]
fn evaluate_deny_beats_protected_and_allow() {
    let mut c = cfg();
    c.allow = vec!["src/**".to_string()];
    c.deny = vec!["src/secret.rs".to_string()];
    let p = policy(c);
    assert!(is_deny(&p.evaluate(&Action::Edit {
        path: "src/secret.rs"
    })));
}

#[test]
fn evaluate_mode_edit_allows_plain() {
    let p = policy(cfg());
    assert_eq!(
        p.evaluate(&Action::Edit {
            path: "src/main.rs"
        }),
        Decision::Allow
    );
}

#[test]
fn evaluate_mode_manual_prompts_plain() {
    let mut c = cfg();
    c.mode = Mode::Manual;
    let p = policy(c);
    assert!(matches!(
        p.evaluate(&Action::Edit {
            path: "src/main.rs"
        }),
        Decision::Prompt { .. }
    ));
}

#[test]
fn evaluate_mode_plan_denies_plain() {
    let mut c = cfg();
    c.mode = Mode::Plan;
    let p = policy(c);
    assert!(is_deny(&p.evaluate(&Action::Edit {
        path: "src/main.rs"
    })));
}

#[test]
fn evaluate_mode_plan_denies_even_allow_listed() {
    let mut c = cfg();
    c.mode = Mode::Plan;
    c.allow = vec!["src/**".to_string()];
    let p = policy(c);
    assert!(is_deny(&p.evaluate(&Action::Edit {
        path: "src/main.rs"
    })));
}

#[test]
fn evaluate_mode_auto_prompts_for_protected() {
    let mut c = cfg();
    c.mode = Mode::Auto;
    let p = policy(c);
    assert!(matches!(
        p.evaluate(&Action::Edit {
            path: ".github/workflows/ci.yml"
        }),
        Decision::Prompt { .. }
    ));
}

#[test]
fn evaluate_mode_auto_allows_plain() {
    let mut c = cfg();
    c.mode = Mode::Auto;
    let p = policy(c);
    assert_eq!(
        p.evaluate(&Action::Edit {
            path: "src/main.rs"
        }),
        Decision::Allow
    );
}

#[test]
fn evaluate_read_denies_env() {
    let p = policy(cfg());
    assert!(is_deny(&p.evaluate(&Action::Read { path: ".env" })));
    assert!(is_deny(&p.evaluate(&Action::Read {
        path: "config/.env.production"
    })));
}

#[test]
fn evaluate_read_allows_source() {
    let p = policy(cfg());
    assert_eq!(
        p.evaluate(&Action::Read {
            path: "src/main.rs"
        }),
        Decision::Allow
    );
}

#[test]
fn compile_invalid_glob_errors() {
    let mut c = cfg();
    c.deny = vec!["[".to_string()];
    assert!(Policy::compile(&c).is_err());
}

#[test]
fn permissive_matches_todays_behavior() {
    let p = Policy::permissive();
    assert_eq!(
        p.evaluate(&Action::Edit {
            path: ".git/hooks/pre-commit"
        }),
        Decision::Allow
    );
    assert_eq!(p.evaluate(&Action::Read { path: ".env" }), Decision::Allow);
}

#[test]
fn use_default_protected_false_unblocks_git() {
    let mut c = cfg();
    c.use_default_protected = false;
    let p = policy(c);
    assert_eq!(
        p.evaluate(&Action::Edit {
            path: ".git/hooks/pre-commit"
        }),
        Decision::Allow
    );
}

#[test]
fn yolo_mode_allows_protected_edits() {
    let mut c = cfg();
    c.mode = Mode::Yolo;
    let p = policy(c);
    assert_eq!(
        p.evaluate(&Action::Edit {
            path: ".git/hooks/pre-commit"
        }),
        Decision::Allow
    );
}

#[test]
fn yolo_mode_allows_secret_reads() {
    let mut c = cfg();
    c.mode = Mode::Yolo;
    let p = policy(c);
    assert_eq!(p.evaluate(&Action::Read { path: ".env" }), Decision::Allow);
}

#[test]
fn auto_mode_allows_plain_exec() {
    let mut c = cfg();
    c.mode = Mode::Auto;
    let p = policy(c);
    assert_eq!(
        p.evaluate(&Action::Exec {
            binary: "npx",
            args: &["vsce", "package"]
        }),
        Decision::Allow
    );
}

#[test]
fn exec_prompt_preview_shows_the_full_command() {
    let mut c = cfg();
    c.mode = Mode::Manual;
    let p = policy(c);
    let decision = p.evaluate(&Action::Exec {
        binary: "git",
        args: &["commit", "-m", "fix things"],
    });
    let Decision::Prompt { preview } = decision else {
        panic!("expected a prompt, got {decision:?}");
    };
    assert_eq!(preview, "run `git commit -m \"fix things\"`");
}

#[test]
fn exec_prompt_preview_caps_huge_argument_lists() {
    let mut c = cfg();
    c.mode = Mode::Manual;
    let p = policy(c);
    let long = "x".repeat(500);
    let decision = p.evaluate(&Action::Exec {
        binary: "git",
        args: &[&long],
    });
    let Decision::Prompt { preview } = decision else {
        panic!("expected a prompt, got {decision:?}");
    };
    assert!(preview.len() < 240);
    assert!(preview.contains('…'));
}

#[test]
fn auto_mode_prompts_for_risky_exec() {
    let mut c = cfg();
    c.mode = Mode::Auto;
    let p = policy(c);
    assert!(matches!(
        p.evaluate(&Action::Exec {
            binary: "rm",
            args: &["-rf", "dist"]
        }),
        Decision::Prompt { .. }
    ));
}

#[test]
fn auto_mode_allow_exec_overrides_risky() {
    let mut c = cfg();
    c.mode = Mode::Auto;
    c.allow_exec = vec!["curl".to_string()];
    let p = policy(c);
    assert_eq!(
        p.evaluate(&Action::Exec {
            binary: "curl",
            args: &["https://example.com"]
        }),
        Decision::Allow
    );
}

#[test]
fn manual_mode_prompts_for_plain_exec() {
    let mut c = cfg();
    c.mode = Mode::Manual;
    let p = policy(c);
    assert!(matches!(
        p.evaluate(&Action::Exec {
            binary: "ls",
            args: &[]
        }),
        Decision::Prompt { .. }
    ));
}

#[test]
fn yolo_mode_allows_exec_by_default() {
    let mut c = cfg();
    c.mode = Mode::Yolo;
    let p = policy(c);
    assert_eq!(
        p.evaluate(&Action::Exec {
            binary: "rm",
            args: &["-rf", "/"]
        }),
        Decision::Allow
    );
}
