//! The compiled decision engine.

use anyhow::{Context, Result};
use globset::{Glob, GlobSet, GlobSetBuilder};

use crate::action::Action;
use crate::config::PermissionsConfig;
use crate::decision::{Decision, Mode};
use crate::defaults::{PROTECTED, SECRET_READ};

/// A compiled, immutable set of rules. Cheap to share (`Arc`) across the tool
/// loop. Pure: [`Policy::evaluate`] never touches disk.
pub struct Policy {
    mode: Mode,
    protected: GlobSet,
    allow: GlobSet,
    deny: GlobSet,
    secret_read: GlobSet,
}

impl Policy {
    /// Build a policy from user config, unioning the built-in protected and
    /// secret lists unless `use_default_protected` is false.
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
        })
    }

    /// A no-op policy matching pre-policy behavior: auto edits, nothing
    /// protected, no secret reads blocked.
    pub fn permissive() -> Policy {
        Policy {
            mode: Mode::Auto,
            protected: GlobSet::empty(),
            allow: GlobSet::empty(),
            deny: GlobSet::empty(),
            secret_read: GlobSet::empty(),
        }
    }

    /// Decide what to do with `action`. `path` must be repo-relative and already
    /// validated against escape by the caller.
    pub fn evaluate(&self, action: &Action) -> Decision {
        match action {
            Action::Edit { path } => self.evaluate_edit(path),
            Action::Read { path } => self.evaluate_read(path),
        }
    }

    fn evaluate_edit(&self, path: &str) -> Decision {
        // Explicit deny beats everything.
        if self.deny.is_match(path) {
            return Decision::Deny {
                reason: format!("`{path}` matches a permissions `deny` rule"),
            };
        }
        // Protected beats mode, unless the user explicitly allow-listed it.
        if self.protected.is_match(path) && !self.allow.is_match(path) {
            return Decision::Deny {
                reason: format!(
                    "`{path}` is a protected path; add it to permissions `allow` to override"
                ),
            };
        }
        if self.allow.is_match(path) {
            return Decision::Allow;
        }
        match self.mode {
            Mode::Auto => Decision::Allow,
            Mode::Ask => Decision::Prompt {
                preview: format!("edit {path}"),
            },
            Mode::Deny => Decision::Deny {
                reason: "permissions mode is `deny`".to_string(),
            },
        }
    }

    fn evaluate_read(&self, path: &str) -> Decision {
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
        c.mode = Mode::Deny;
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
    fn evaluate_mode_auto_allows_plain() {
        let p = policy(cfg());
        assert_eq!(
            p.evaluate(&Action::Edit {
                path: "src/main.rs"
            }),
            Decision::Allow
        );
    }

    #[test]
    fn evaluate_mode_ask_prompts_plain() {
        let mut c = cfg();
        c.mode = Mode::Ask;
        let p = policy(c);
        assert!(matches!(
            p.evaluate(&Action::Edit {
                path: "src/main.rs"
            }),
            Decision::Prompt { .. }
        ));
    }

    #[test]
    fn evaluate_mode_deny_denies_plain() {
        let mut c = cfg();
        c.mode = Mode::Deny;
        let p = policy(c);
        assert!(is_deny(&p.evaluate(&Action::Edit {
            path: "src/main.rs"
        })));
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
}
