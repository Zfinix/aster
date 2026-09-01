//! Deserialized `permissions:` section of `aster.yaml`.

use serde::Deserialize;

use crate::decision::Mode;

/// User-facing permission config, compiled into a [`crate::Policy`]. `allow`,
/// `ask`, and `deny` hold rules in one language: `Edit(glob)`, `Read(glob)`,
/// `Bash(command:*)`; a bare `Edit`, `Read`, or `Bash` covers everything.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PermissionsConfig {
    /// What happens to an action no rule matches.
    pub mode: Mode,
    /// Runs without asking.
    pub allow: Vec<String>,
    /// Always confirmed first.
    pub ask: Vec<String>,
    /// Refused outright.
    pub deny: Vec<String>,
    /// Skip the built-in rules. Leaves `.git`, CI workflows, risky commands,
    /// and secret files gated by nothing but the mode.
    pub use_default_rules: bool,
    /// Directories outside the repository the agent may read without asking.
    /// Absolute, or `~`-relative. Anything else outside the repo prompts.
    pub additional_directories: Vec<String>,
    /// Credential directories a command may read inside the sandbox without
    /// asking, written `<command>:<dir>` (e.g. `gh:~/.config/gh`). Meant for
    /// headless runs, which have no way to answer a prompt.
    pub allow_credentials: Vec<String>,
}

impl Default for PermissionsConfig {
    fn default() -> Self {
        Self {
            mode: Mode::default(),
            allow: Vec::new(),
            ask: Vec::new(),
            deny: Vec::new(),
            use_default_rules: true,
            additional_directories: Vec::new(),
            allow_credentials: Vec::new(),
        }
    }
}
