//! Terminal UIs: [`run`] is the live review TUI, [`run_chat`] the chat agent.

mod chat;
mod guard;
mod helpers;
mod review;
mod summary;

pub use chat::run_chat;
pub use review::run;

use ratatui::style::Color;

const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
/// Brand orange, matching the desktop app's `--accent` (#f2764f).
const ACCENT: Color = Color::Rgb(242, 118, 79);
