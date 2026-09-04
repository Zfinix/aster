//! Native OS notifications: on macOS they are posted through a tiny
//! `Aster Notifier.app` bundle so the banner carries the aster logo instead
//! of the Script Editor one; `notify-send` on Linux.

use anyhow::{Context, Result};
use std::path::PathBuf;

const NOTIFIER_BUNDLE_ID: &str = "dev.aster.notifier";

/// Post a desktop notification. On macOS this is a Notification Center banner;
/// the first one may need the terminal app to be allowed in System Settings.
pub fn send(title: &str, body: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        if send_via_notifier_app(title, body).is_ok() {
            return Ok(());
        }
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

#[cfg(target_os = "macos")]
fn send_via_notifier_app(title: &str, body: &str) -> Result<()> {
    ensure_notifier_app()?;
    let status = std::process::Command::new("terminal-notifier")
        .args(["-sender", NOTIFIER_BUNDLE_ID])
        .arg("-title")
        .arg(title)
        .arg("-message")
        .arg(body)
        .status()
        .context("running terminal-notifier")?;
    anyhow::ensure!(
        status.success(),
        "terminal-notifier rejected the notification"
    );
    Ok(())
}

#[cfg(target_os = "macos")]
fn ensure_notifier_app() -> Result<PathBuf> {
    let home = std::env::var("HOME").context("HOME is not set")?;
    let app: PathBuf = [
        home.as_str(),
        "Library",
        "Application Support",
        "aster",
        "Aster Notifier.app",
    ]
    .into_iter()
    .collect();
    let contents = app.join("Contents");
    let resources = contents.join("Resources");
    std::fs::create_dir_all(&resources).context("creating Aster Notifier.app")?;
    let icon = resources.join("applet.icns");
    if !icon.exists() {
        const ICON: &[u8] = include_bytes!("../assets/aster.icns");
        std::fs::write(&icon, ICON).context("writing the notifier icon")?;
    }
    let plist = contents.join("Info.plist");
    std::fs::write(&plist, plist_xml()).context("writing the notifier Info.plist")?;
    let bin = contents.join("MacOS").join("Aster Notifier");
    std::fs::create_dir_all(bin.parent().unwrap())?;
    std::fs::write(&bin, "#!/bin/sh\nexit 0\n").context("writing the notifier stub")?;
    set_executable(&bin)?;
    let _ = std::process::Command::new("/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister")
        .arg("-f")
        .arg(&app)
        .status();
    Ok(app)
}

#[cfg(target_os = "macos")]
fn set_executable(path: &std::path::Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(path)?.permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(path, perms).context("making the notifier stub executable")?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn plist_xml() -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleIdentifier</key>
    <string>{NOTIFIER_BUNDLE_ID}</string>
    <key>CFBundleName</key>
    <string>Aster Notifier</string>
    <key>CFBundleDisplayName</key>
    <string>Aster Notifier</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleExecutable</key>
    <string>Aster Notifier</string>
    <key>CFBundleIconFile</key>
    <string>applet.icns</string>
    <key>CFBundleVersion</key>
    <string>1</string>
    <key>CFBundleShortVersionString</key>
    <string>1.0</string>
    <key>LSUIElement</key>
    <true/>
</dict>
</plist>
"#
    )
}

#[cfg(target_os = "macos")]
fn shell_quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}
