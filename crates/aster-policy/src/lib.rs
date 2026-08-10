//! Permission policy for Aster's file-mutating, file-reading, and command tools.
//!
//! A [`Policy`] is compiled from the `permissions:` section of `aster.yaml` and
//! consulted before every edit, read, and command. One rule language covers all
//! three: `Edit(glob)`, `Read(glob)`, `Bash(command:*)`, sorted into `allow`,
//! `ask`, and `deny`. It is pure and UI-agnostic, returning a [`Decision`] the
//! caller acts on. Path-escape validation is the caller's job upstream; the
//! policy reasons only about repo-relative path strings.

mod action;
mod config;
mod decision;
pub mod defaults;
mod grants;
mod policy;
mod rule;
mod shell;

pub use action::Action;
pub use config::PermissionsConfig;
pub use decision::{Decision, Mode};
pub use grants::{CommandGrants, Grants};
pub use policy::Policy;
pub use rule::Rule;
