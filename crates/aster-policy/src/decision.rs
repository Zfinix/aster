//! The outcome of evaluating an [`crate::Action`] against a [`crate::Policy`].

use serde::Deserialize;

/// Agent permission mode. Controls the edit and execution guardrails.
/// Deny rules always override the mode; modes higher in the order
/// grant more freedom (plan < manual < auto < edit < yolo).
#[derive(Debug, Default, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// Explore and propose a plan; never edit.
    #[serde(alias = "deny")]
    Plan,
    /// Confirm every edit before it lands.
    #[serde(alias = "ask")]
    Manual,
    /// Apply what passes the safety check, ask before anything risky.
    Auto,
    /// Apply edits without confirmation.
    #[default]
    Edit,
    /// No guardrails: policy checks and isolation are skipped entirely.
    /// Toggled with `/yolo` behind a confirm; turns the theme red.
    #[serde(alias = "yolo")]
    Yolo,
}

impl Mode {
    /// The more restrictive of the two.
    /// Used so a caller-supplied mode (e.g. a CLI flag) can only tighten
    /// aster.yaml's configured mode, never loosen it.
    pub fn stricter(self, other: Mode) -> Mode {
        fn rank(m: Mode) -> u8 {
            match m {
                Mode::Plan => 0,
                Mode::Manual => 1,
                Mode::Auto => 2,
                Mode::Edit => 3,
                Mode::Yolo => 4,
            }
        }
        if rank(self) <= rank(other) {
            self
        } else {
            other
        }
    }

    /// The lowercase name used on the CLI, in aster.yaml, and on the JSON wire.
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Plan => "plan",
            Mode::Manual => "manual",
            Mode::Auto => "auto",
            Mode::Edit => "edit",
            Mode::Yolo => "yolo",
        }
    }

    /// One line describing what the mode does, for menus and headers.
    pub fn description(self) -> &'static str {
        match self {
            Mode::Plan => "explore the code and present a plan before editing",
            Mode::Manual => "ask for approval before each edit",
            Mode::Auto => "apply what passes the safety check, pause for anything risky",
            Mode::Edit => "edit files without asking",
            Mode::Yolo => "no guardrails, unrestricted",
        }
    }

    /// True when the mode lets the agent write at all.
    pub fn can_edit(self) -> bool {
        matches!(self, Mode::Manual | Mode::Auto | Mode::Edit | Mode::Yolo)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stricter_plan_beats_everything() {
        assert_eq!(Mode::Plan.stricter(Mode::Edit), Mode::Plan);
        assert_eq!(Mode::Edit.stricter(Mode::Plan), Mode::Plan);
        assert_eq!(Mode::Plan.stricter(Mode::Manual), Mode::Plan);
    }

    #[test]
    fn stricter_manual_beats_auto_and_edit() {
        assert_eq!(Mode::Manual.stricter(Mode::Auto), Mode::Manual);
        assert_eq!(Mode::Edit.stricter(Mode::Manual), Mode::Manual);
        assert_eq!(Mode::Auto.stricter(Mode::Edit), Mode::Auto);
    }

    #[test]
    fn stricter_same_mode_is_a_no_op() {
        assert_eq!(Mode::Edit.stricter(Mode::Edit), Mode::Edit);
        assert_eq!(Mode::Plan.stricter(Mode::Plan), Mode::Plan);
    }

    #[test]
    fn yolo_is_least_strict() {
        assert_eq!(Mode::Yolo.stricter(Mode::Edit), Mode::Edit);
        assert_eq!(Mode::Yolo.stricter(Mode::Plan), Mode::Plan);
        assert_eq!(Mode::Edit.stricter(Mode::Yolo), Mode::Edit);
    }

    #[test]
    fn deserializes_legacy_ask_and_deny_names() {
        assert_eq!(
            serde_json::from_str::<Mode>("\"ask\"").expect("ask parses"),
            Mode::Manual
        );
        assert_eq!(
            serde_json::from_str::<Mode>("\"deny\"").expect("deny parses"),
            Mode::Plan
        );
    }
}

/// What the caller should do with the action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Deny {
        reason: String,
    },
    /// `preview` describes the pending change. The caller prompts (interactive)
    /// or treats it as a denial (headless).
    Prompt {
        preview: String,
    },
}

#[test]
fn deserializes_yolo() {
    assert_eq!(
        serde_json::from_str::<Mode>("\"yolo\"").expect("yolo parses"),
        Mode::Yolo
    );
}
