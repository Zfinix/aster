//! Deserialized `permissions:` section of `aster.yaml`.

use serde::Deserialize;

use crate::decision::Mode;

/// User-facing permission config, compiled into a [`crate::Policy`]. `allow`,
/// `ask`, and `deny` hold rules in one language: `Edit(glob)`, `Read(glob)`,
/// `Bash(command:*)`; a bare `Edit`, `Read`, or `Bash` covers everything.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PermissionsConfig {
    pub mode: Mode,
    pub allow: Vec<String>,
    pub ask: Vec<String>,
    pub deny: Vec<String>,
    pub use_default_rules: bool,
    pub additional_directories: Vec<String>,
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
