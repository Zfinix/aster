//! Terminal UIs: [`run`] is the live review TUI, [`run_chat`] the chat agent.

mod bottom_pane;
mod chat;
mod composer;
mod dictation;
mod guard;
mod helpers;
mod history;
mod markdown;
mod palettes;
mod render;
mod review;
mod step_label;
mod summary;
mod syntax;
mod term;
mod terminal;
pub(crate) mod theme;
mod user_themes;
mod voice;
mod wrap;

pub use chat::run_chat;
pub(crate) use helpers::mark_ansi;
pub use review::run;
pub(crate) use step_label::step_label;

pub(crate) const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
