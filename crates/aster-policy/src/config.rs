//! Deserialized `permissions:` section of `aster.yaml`.

use serde::Deserialize;

use crate::decision::Mode;

/// User-facing permission config. Compiled into a [`crate::Policy`] via
/// [`crate::Policy::compile`].
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PermissionsConfig {
    /// Gate for unmatched edit paths. Defaults to [`Mode::Auto`].
    pub mode: Mode,
    /// Globs always writable, overriding the protected list.
    pub allow: Vec<String>,
    /// Globs never writable.
    pub deny: Vec<String>,
    /// Extra always-blocked write globs, unioned with [`crate::defaults::PROTECTED`].
    pub protected: Vec<String>,
    /// Extra never-readable globs, unioned with [`crate::defaults::SECRET_READ`].
    pub secret_read: Vec<String>,
    /// Include the built-in protected and secret-read lists. Escape hatch;
    /// defaults to true.
    pub use_default_protected: bool,
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
        }
    }
}
