//! The outcome of evaluating an [`crate::Action`] against a [`crate::Policy`].

use serde::Deserialize;

/// How edits are gated once a write is actually attempted. Protected-path and
/// explicit deny rules override the mode; only unmatched paths fall through to it.
#[derive(Debug, Default, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// Apply edits without confirmation (preserves pre-policy behavior).
    #[default]
    Auto,
    /// Ask for per-edit confirmation. Prompts in the TUI; denied when headless.
    Ask,
    /// Never edit.
    Deny,
}

/// What the caller should do with the action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// Proceed.
    Allow,
    /// Refuse, with a human-readable reason.
    Deny { reason: String },
    /// Requires user confirmation; `preview` describes the pending change. The
    /// caller either prompts (interactive) or treats it as a denial (headless).
    Prompt { preview: String },
}
