//! Permission policy for Aster's edit, read, and command tools. A [`Policy`] is
//! compiled from `permissions:` in `aster.yaml` and consulted before each one,
//! returning a [`Decision`]. Path-escape validation is the caller's job upstream.

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
