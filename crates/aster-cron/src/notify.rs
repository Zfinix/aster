//! Native OS notifications with no new dependencies: `osascript` on macOS,
//! `notify-send` on Linux.

use anyhow::{Context, Result};

/// Post a desktop notification. On macOS this is a Notification Center banner;
/// the first one may need the terminal app to be allowed in System Settings.
pub fn send(title: &str, body: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        let script = format!(
            "display notification {} with title {}",
            shell_quote(body),
            shell_quote(title)
        );
        let status = std::process::Command::new("osascript")
            .arg("-e")
            .arg(&script)
            .status()
            .context("running osascript")?;
        anyhow::ensure!(status.success(), "osascript rejected the notification");
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let status = std::process::Command::new("notify-send")
            .arg(title)
            .arg(body)
            .status()
            .context("running notify-send")?;
        anyhow::ensure!(status.success(), "notify-send rejected the notification");
        Ok(())
    }
}

/// AppleScript string literal: double the backslash first, then the quotes.
#[cfg(target_os = "macos")]
fn shell_quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}
