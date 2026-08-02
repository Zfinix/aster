//! The compiled decision engine.

use anyhow::{Context, Result};
use globset::{Glob, GlobSet, GlobSetBuilder};

use crate::action::Action;
use crate::config::PermissionsConfig;
use crate::decision::{Decision, Mode};
use crate::defaults::{PROTECTED, SECRET_READ};

/// A compiled, immutable set of rules. [`Policy::evaluate`] is pure and never
/// touches disk.
#[derive(Clone)]
pub struct Policy {
    mode: Mode,
    protected: GlobSet,
    allow: GlobSet,
    deny: GlobSet,
    secret_read: GlobSet,
    allow_exec: Vec<String>,
    deny_exec: Vec<String>,
}

impl Policy {
    /// Unions the built-in protected and secret lists unless
    /// `use_default_protected` is false.
    pub fn compile(cfg: &PermissionsConfig) -> Result<Policy> {
        let protected = if cfg.use_default_protected {
            union(PROTECTED, &cfg.protected)
        } else {
            cfg.protected.clone()
        };
        let secret_read = if cfg.use_default_protected {
            union(SECRET_READ, &cfg.secret_read)
        } else {
            cfg.secret_read.clone()
        };
        Ok(Policy {
            mode: cfg.mode,
            protected: build(&protected)?,
            allow: build(&cfg.allow)?,
            deny: build(&cfg.deny)?,
            secret_read: build(&secret_read)?,
            allow_exec: cfg.allow_exec.clone(),
            deny_exec: cfg.deny_exec.clone(),
        })
    }

    /// A no-op policy: unconditional edits, nothing protected, no secret reads blocked.
    pub fn permissive() -> Policy {
        Policy {
            mode: Mode::Edit,
            protected: GlobSet::empty(),
            allow: GlobSet::empty(),
            deny: GlobSet::empty(),
            secret_read: GlobSet::empty(),
            allow_exec: Vec::new(),
            deny_exec: Vec::new(),
        }
    }

    /// The compiled mode, for callers that need to check it directly
    /// (e.g. yolo triple-confirm in the sandbox layer).
    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// `path` must be repo-relative and already validated against escape by the caller.
    pub fn evaluate(&self, action: &Action) -> Decision {
        match action {
            Action::Edit { path } => self.evaluate_edit(path),
            Action::Read { path } => self.evaluate_read(path),
            Action::Exec { binary, .. } => self.evaluate_exec(binary),
        }
    }

    fn evaluate_exec(&self, binary: &str) -> Decision {
        if self.mode == Mode::Yolo {
            return Decision::Allow;
        }
        if self.deny_exec.iter().any(|b| b == binary) {
            return Decision::Deny {
                reason: format!("command `{binary}` is denied by permissions"),
            };
        }
        if self.allow_exec.iter().any(|b| b == binary) {
            return Decision::Allow;
        }
        match self.mode {
            Mode::Plan => Decision::Deny {
                reason: "permissions mode is `plan`, so command execution is off".to_string(),
            },
            Mode::Edit | Mode::Yolo => Decision::Allow,
            Mode::Auto | Mode::Manual => Decision::Prompt {
                preview: format!("run `{binary}`"),
            },
        }
    }

    fn evaluate_edit(&self, path: &str) -> Decision {
        if self.mode == Mode::Yolo {
            return Decision::Allow;
        }
        // Explicit deny beats everything.
        if self.deny.is_match(path) {
            return Decision::Deny {
                reason: format!("`{path}` matches a permissions `deny` rule"),
            };
        }
        // Plan mode never writes, allow-listed or not.
        if self.mode == Mode::Plan {
            return Decision::Deny {
                reason: "permissions mode is `plan`, so edits are off".to_string(),
            };
        }
        // Protected beats mode, unless the user explicitly allow-listed it.
        // `auto` is the exception: risky is what it pauses on rather than refuses.
        if self.protected.is_match(path) && !self.allow.is_match(path) {
            return match self.mode {
                Mode::Auto => Decision::Prompt {
                    preview: format!("edit {path} (protected path)"),
                },
                _ => Decision::Deny {
                    reason: format!(
                        "`{path}` is a protected path; add it to permissions `allow` to override"
                    ),
                },
            };
        }
        if self.allow.is_match(path) {
            return Decision::Allow;
        }
        match self.mode {
            Mode::Auto | Mode::Edit | Mode::Yolo => Decision::Allow,
            Mode::Manual => Decision::Prompt {
                preview: format!("edit {path}"),
            },
            Mode::Plan => unreachable!("plan returns above"),
        }
    }

    fn evaluate_read(&self, path: &str) -> Decision {
        if self.mode == Mode::Yolo {
            return Decision::Allow;
        }
        if self.secret_read.is_match(path) {
            return Decision::Deny {
                reason: format!("`{path}` is a secret file; reading it is blocked by policy"),
            };
        }
        Decision::Allow
    }
}

fn union(defaults: &[&str], extra: &[String]) -> Vec<String> {
    defaults
        .iter()
        .map(|s| s.to_string())
        .chain(extra.iter().cloned())
        .collect()
}

fn build(patterns: &[String]) -> Result<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    for p in patterns {
        builder.add(Glob::new(p).with_context(|| format!("invalid glob: {p}"))?);
    }
    builder.build().context("building glob set")
}

#[cfg(test)]
mod tests {
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
}
