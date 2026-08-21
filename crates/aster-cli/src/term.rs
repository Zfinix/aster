//! Shared ANSI escape codes for terminal output.

pub(crate) const DIM: &str = "\x1b[2m";
pub(crate) const CYAN: &str = "\x1b[36m";
pub(crate) const GREEN: &str = "\x1b[32m";
pub(crate) const RED: &str = "\x1b[31m";
pub(crate) const RESET: &str = "\x1b[0m";
pub(crate) const BOLD: &str = "\x1b[1m";
/// Brand orange, matching the desktop app's `--accent` (#f2764f).
pub(crate) const ORANGE: &str = "\x1b[38;2;242;118;79m";

/// Wrap `text` in an escape code, or leave it bare when the output is not a
/// terminal. `NO_COLOR` disables; `CLICOLOR_FORCE` forces it on into a pipe.
pub(crate) fn paint(code: &str, text: &str) -> String {
    use std::io::IsTerminal;
    let color = std::env::var_os("NO_COLOR").is_none()
        && (std::io::stdout().is_terminal() || std::env::var_os("CLICOLOR_FORCE").is_some());
    match color {
        true => format!("{code}{text}{RESET}"),
        false => text.to_string(),
    }
}
