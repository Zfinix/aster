//! Deserialized `permissions:` section of `aster.yaml`.

use serde::Deserialize;

use crate::decision::Mode;

/// User-facing permission config, compiled into a [`crate::Policy`].
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PermissionsConfig {
    /// Gate for unmatched edit paths.
    pub mode: Mode,
    /// Globs always writable, overriding the protected list.
    pub allow: Vec<String>,
    pub deny: Vec<String>,
    /// Extra write globs, unioned with [`crate::defaults::PROTECTED`].
    pub protected: Vec<String>,
    /// Extra never-readable globs, unioned with [`crate::defaults::SECRET_READ`].
    pub secret_read: Vec<String>,
    /// Include the built-in protected and secret-read lists. Defaults to true.
    pub use_default_protected: bool,
    /// Directories outside the repository the agent may read without asking.
    /// Absolute, or `~`-relative. Anything else outside the repo prompts.
    pub additional_directories: Vec<String>,
    /// Commands the agent may run without asking. Matched by binary name
    /// (e.g. "cargo", "rg", "npm"). Empty = none allowed.
    pub allow_exec: Vec<String>,
    /// Commands the agent may never run. Matched by binary name.
    pub deny_exec: Vec<String>,
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
            deny: Vec::new(),
            protected: Vec::new(),
            secret_read: Vec::new(),
            use_default_protected: true,
            additional_directories: Vec::new(),
            allow_exec: Vec::new(),
            deny_exec: Vec::new(),
            allow_credentials: Vec::new(),
        }
    }
}
