//! Permission policy for Aster's file-mutating and file-reading tools.
//!
//! A [`Policy`] is compiled from the `permissions:` section of `aster.yaml` and
//! consulted before every edit or read. It is pure and UI-agnostic, returning a
//! [`Decision`] the caller acts on. Path-escape validation is the caller's job
//! upstream; the policy reasons only about repo-relative path strings.

mod action;
mod config;
mod decision;
pub mod defaults;
mod policy;

pub use action::Action;
pub use config::PermissionsConfig;
pub use decision::{Decision, Mode};
pub use policy::Policy;
