//! The compiled decision engine.

use anyhow::{Context, Result};

use crate::action::Action;
use crate::config::PermissionsConfig;
use crate::decision::{Decision, Mode};
use crate::defaults;
use crate::rule::{Rule, first_match, parse_all};

/// A compiled, immutable set of rules. [`Policy::evaluate`] is pure and never
/// touches disk.
#[derive(Clone)]
pub struct Policy {
    mode: Mode,
    allow: Vec<Rule>,
    ask: Vec<Rule>,
    deny: Vec<Rule>,
    default_ask: Vec<Rule>,
    default_ask_commands: Vec<Rule>,
    default_deny: Vec<Rule>,
}

impl Policy {
    pub fn compile(cfg: &PermissionsConfig) -> Result<Policy> {
        let builtin = |rules: &[&str]| -> Result<Vec<Rule>> {
            if !cfg.use_default_rules {
                return Ok(Vec::new());
            }
            parse_all(&rules.iter().map(|s| s.to_string()).collect::<Vec<_>>())
        };
        Ok(Policy {
            mode: cfg.mode,
            allow: parse_all(&cfg.allow).context("permissions `allow`")?,
            ask: parse_all(&cfg.ask).context("permissions `ask`")?,
            deny: parse_all(&cfg.deny).context("permissions `deny`")?,
            default_ask: builtin(defaults::ASK_EDIT)?,
            default_ask_commands: builtin(defaults::ASK_BASH)?,
            default_deny: builtin(defaults::DENY_READ)?,
        })
    }

    /// A no-op policy: everything allowed, no rules at all.
    pub fn permissive() -> Policy {
        Policy {
            mode: Mode::Edit,
            allow: Vec::new(),
            ask: Vec::new(),
            deny: Vec::new(),
            default_ask: Vec::new(),
            default_ask_commands: Vec::new(),
            default_deny: Vec::new(),
        }
    }

    /// The compiled mode, for callers that need to check it directly
    /// (e.g. yolo triple-confirm in the sandbox layer).
    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// Move to a looser mode once the user has said so, e.g. after approving a
    /// plan. The rules are kept, so `deny` still denies; only the fallback the
    /// mode decides changes.
    pub fn promote(&mut self, mode: Mode) {
        if self.mode.stricter(mode) == self.mode {
            self.mode = mode;
        }
    }

    /// Move to a stricter mode, e.g. when the agent drops itself into `plan`
    /// before touching anything. Only ever tightens, so this cannot be a way
    /// around the configured ceiling.
    pub fn demote(&mut self, mode: Mode) {
        self.mode = self.mode.stricter(mode);
    }

    /// Path actions carry a repo-relative path, already validated against
    /// escape by the caller.
    pub fn evaluate(&self, action: &Action) -> Decision {
        if self.mode == Mode::Yolo {
            return Decision::Allow;
        }
        // User rules first, strictest bucket first, so one `allow` entry is
        // enough to override a built-in without disabling the whole set.
        if let Some(rule) = first_match(&self.deny, action) {
            return Decision::Deny {
                reason: format!(
                    "{} matches the `deny` rule `{}`",
                    subject(action),
                    rule.source()
                ),
            };
        }
        if let Some(rule) = first_match(&self.ask, action) {
            return Decision::Prompt {
                preview: preview(action, rule.source()),
            };
        }
        if first_match(&self.allow, action).is_some() {
            return Decision::Allow;
        }
        if let Some(rule) = first_match(&self.default_deny, action) {
            return Decision::Deny {
                reason: format!(
                    "{} matches the built-in rule `{}`; add it to permissions `allow` to override",
                    subject(action),
                    rule.source()
                ),
            };
        }
        if let Some(rule) = first_match(&self.default_ask, action) {
            return Decision::Prompt {
                preview: preview(action, rule.source()),
            };
        }
        if self.mode != Mode::Edit
            && let Some(rule) = first_match(&self.default_ask_commands, action)
        {
            return Decision::Prompt {
                preview: preview(action, rule.source()),
            };
        }
        self.by_mode(action)
    }

    fn by_mode(&self, action: &Action) -> Decision {
        match (self.mode, action) {
            (Mode::Yolo, _) => Decision::Allow,
            (_, Action::Read { .. }) => Decision::Allow,
            (Mode::Plan, Action::Edit { .. }) => Decision::Deny {
                reason: "permissions mode is `plan`, so edits are off".to_string(),
            },
            (Mode::Plan, Action::Exec { .. }) => Decision::Deny {
                reason: "permissions mode is `plan`, so command execution is off".to_string(),
            },
            (Mode::Manual, _) => Decision::Prompt {
                preview: bare_preview(action),
            },
            (Mode::Auto | Mode::Edit, _) => Decision::Allow,
        }
    }
}

fn subject(action: &Action) -> String {
    match action {
        Action::Edit { path } | Action::Read { path } => format!("`{path}`"),
        Action::Exec { binary, args } => format!("`{}`", command_line(binary, args)),
    }
}

fn preview(action: &Action, rule: &str) -> String {
    format!("{} ({rule})", bare_preview(action))
}

fn bare_preview(action: &Action) -> String {
    match action {
        Action::Edit { path } => format!("edit {path}"),
        Action::Read { path } => format!("read {path}"),
        Action::Exec { binary, args } => format!("run `{}`", command_line(binary, args)),
    }
}

fn command_line(binary: &str, args: &[&str]) -> String {
    const PREVIEW_LIMIT: usize = 200;
    let mut line = binary.to_string();
    for arg in args {
        line.push(' ');
        if arg.contains(char::is_whitespace) {
            line.push('"');
            line.push_str(arg);
            line.push('"');
        } else {
            line.push_str(arg);
        }
    }
    if line.len() > PREVIEW_LIMIT {
        let mut cut = PREVIEW_LIMIT;
        while !line.is_char_boundary(cut) {
            cut -= 1;
        }
        line.truncate(cut);
        line.push('…');
    }
    line
}

#[cfg(test)]
#[path = "tests/policy_test.rs"]
mod tests;
