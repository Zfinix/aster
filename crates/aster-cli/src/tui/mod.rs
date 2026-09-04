//! Terminal UIs: [`run`] is the live review TUI, [`run_chat`] the chat agent.

mod bottom_pane;
mod chat;
mod composer;
mod guard;
mod helpers;
mod history;
mod markdown;
mod render;
mod review;
mod summary;
mod syntax;
mod term;
mod terminal;
pub(crate) mod theme;
mod wrap;

pub use chat::run_chat;
pub(crate) use chat::step_label;
pub(crate) use helpers::mark_ansi;
pub use review::run;

pub(crate) const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
