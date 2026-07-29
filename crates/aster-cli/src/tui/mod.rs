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
mod theme;
mod wrap;

pub use chat::run_chat;
pub use review::run;

use ratatui::style::Color;

const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
/// The shaded band the bottom pane sits on, the same tone as a turn rail.
const PANE_BG: Color = Color::Rgb(0x19, 0x19, 0x19);
/// Brand orange, matching the desktop app's `--accent` (#f2764f).
const ACCENT: Color = Color::Rgb(242, 118, 79);
