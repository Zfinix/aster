//! macOS launchd backend: one plist per schedule under `~/Library/LaunchAgents/`.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// The plist label and file stem, e.g. `com.aster.cron.nightly-review`.
pub fn label(name: &str) -> String {
    format!("com.aster.cron.{name}")
}

fn plist_path(name: &str) -> Result<PathBuf> {
    let home = dirs_home()?;
    Ok(home
        .join("Library")
        .join("LaunchAgents")
        .join(format!("{}.plist", label(name))))
}

fn dirs_home() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .context("no home directory")
}

/// Render the plist XML for one schedule. `program_args` is the full argv,
/// starting with the aster binary path.
pub fn render(
    name: &str,
    intervals: &[crate::schedule::CalendarInterval],
    program_args: &[String],
    working_dir: &Path,
    log_path: &Path,
) -> String {
    let mut calendar = String::new();
    for c in intervals {
        calendar.push_str("        <dict>\n");
        calendar.push_str(&format!(
            "            <key>Minute</key><integer>{}</integer>\n",
            c.minute
        ));
        calendar.push_str(&format!(
            "            <key>Hour</key><integer>{}</integer>\n",
            c.hour
        ));
        if let Some(d) = c.day {
            calendar.push_str(&format!(
                "            <key>Day</key><integer>{d}</integer>\n"
            ));
        }
        if let Some(m) = c.month {
            calendar.push_str(&format!(
                "            <key>Month</key><integer>{m}</integer>\n"
            ));
        }
        if let Some(w) = c.weekday {
            calendar.push_str(&format!(
                "            <key>Weekday</key><integer>{w}</integer>\n"
            ));
        }
        calendar.push_str("        </dict>\n");
    }

    let args = program_args
        .iter()
        .map(|a| format!("            <string>{}</string>\n", xml_escape(a)))
        .collect::<String>();

    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{label}</string>
    <key>ProgramArguments</key>
    <array>
{args}    </array>
    <key>WorkingDirectory</key>
    <string>{working_dir}</string>
    <key>StartCalendarInterval</key>
    <array>
{calendar}    </array>
    <key>StandardOutPath</key>
    <string>{log_path}</string>
    <key>StandardErrorPath</key>
    <string>{log_path}</string>
    <key>RunAtLoad</key>
    <false/>
</dict>
</plist>
"#,
        label = label(name),
        working_dir = xml_escape(&working_dir.to_string_lossy()),
        log_path = xml_escape(&log_path.to_string_lossy()),
    )
}

/// Write the plist and ask launchd to load it. Idempotent: an existing plist
/// for the same name is replaced.
pub fn install(
    name: &str,
    intervals: &[crate::schedule::CalendarInterval],
    program_args: &[String],
    working_dir: &Path,
    log_path: &Path,
) -> Result<PathBuf> {
    let path = plist_path(name)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    std::fs::write(
        &path,
        render(name, intervals, program_args, working_dir, log_path),
    )
    .with_context(|| format!("writing {}", path.display()))?;
    // A stale load would keep the old definition alive; bootout is allowed to
    // fail when nothing was loaded.
    let _ = std::process::Command::new("launchctl")
        .args(["bootout", &format!("gui/$UID/{}", label(name))])
        .status();
    let status = std::process::Command::new("launchctl")
        .args(["bootstrap", "gui/$UID"])
        .arg(&path)
        .status()
        .context("running launchctl bootstrap")?;
    anyhow::ensure!(status.success(), "launchctl bootstrap failed for {name}");
    Ok(path)
}

/// Unload and delete the plist. Missing plist is not an error.
pub fn remove(name: &str) -> Result<()> {
    let path = plist_path(name)?;
    let _ = std::process::Command::new("launchctl")
        .args(["bootout", &format!("gui/$UID/{}", label(name))])
        .status();
    if path.exists() {
        std::fs::remove_file(&path).with_context(|| format!("removing {}", path.display()))?;
    }
    Ok(())
}

pub fn is_installed(name: &str) -> bool {
    plist_path(name).map(|p| p.exists()).unwrap_or(false)
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
#[path = "tests/launchd_test.rs"]
mod tests;
