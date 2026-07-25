//! Permission policy for Aster's file-mutating and file-reading tools.
//!
//! A [`Policy`] is compiled once from the `permissions:` section of `aster.yaml`
//! and consulted before every edit or read. It is pure and UI-agnostic: it
//! returns a [`Decision`], and the caller decides how to act on a
//! [`Decision::Prompt`] (confirm interactively, or refuse when headless).
//!
//! Path-escape validation (`..`, absolute, symlinked parents) is the caller's
//! job upstream; the policy reasons only about repo-relative path strings.

mod action;
mod config;
mod decision;
pub mod defaults;
mod policy;

pub use action::Action;
pub use config::PermissionsConfig;
pub use decision::{Decision, Mode};
pub use policy::Policy;
