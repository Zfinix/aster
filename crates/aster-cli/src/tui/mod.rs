//! Terminal UIs for Aster. Two surfaces share the same chrome and helpers:
//!
//! - [`run`] — the live review TUI ([`review`]). Runs the whole review in the
//!   background and renders each step as it happens (indexing, hypotheses,
//!   verification, findings landing), then drops into a follow-up chat.
//! - [`run_chat`] — the standalone conversational agent ([`chat`]), driven from
//!   `aster chat --tui`.
//!
//! Layout:
//! - [`guard`] — restores the terminal on every exit path.
//! - [`helpers`] — shared rendering and formatting (the mark, input box, chips).
//! - [`summary`] — reprints review results to the real terminal on exit.

mod chat;
mod guard;
mod helpers;
mod review;
mod summary;

pub use chat::run_chat;
pub use review::run;

use ratatui::style::Color;

/// Braille spinner frames, cycled while work is in flight. Shared by both TUIs.
const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
/// Brand orange, matching the desktop app's `--accent` (#f2764f).
const ACCENT: Color = Color::Rgb(242, 118, 79);
