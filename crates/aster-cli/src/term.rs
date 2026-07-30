//! Shared ANSI escape codes for terminal output.

pub(crate) const DIM: &str = "\x1b[2m";
pub(crate) const CYAN: &str = "\x1b[36m";
pub(crate) const GREEN: &str = "\x1b[32m";
pub(crate) const RED: &str = "\x1b[31m";
pub(crate) const RESET: &str = "\x1b[0m";
pub(crate) const BOLD: &str = "\x1b[1m";
/// Brand orange, matching the desktop app's `--accent` (#f2764f).
pub(crate) const ORANGE: &str = "\x1b[38;2;242;118;79m";
