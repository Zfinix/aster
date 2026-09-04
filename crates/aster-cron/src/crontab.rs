//! Linux crontab backend: one line per schedule, managed through `crontab -`.

use anyhow::{Context, Result};

/// The marker comment that scopes every aster-owned line, so `remove` never
/// touches entries the user wrote themselves.
fn marker(name: &str) -> String {
    format!("ASTER-CRON:{name}")
}

/// The crontab line for one schedule.
pub fn line(name: &str, cron: &str, command: &str) -> String {
    format!("{cron} {command} # {}", marker(name))
}

/// True when the current crontab already carries `name`'s entry.
pub fn is_installed(name: &str) -> bool {
    current_crontab()
        .map(|text| text.lines().any(|l| l.contains(&marker(name))))
        .unwrap_or(false)
}

/// Replace (or add) `name`'s entry in the crontab, leaving every other line
/// byte for byte.
pub fn install(name: &str, cron: &str, command: &str) -> Result<()> {
    let entry = line(name, cron, command);
    let text = current_crontab().unwrap_or_default();
    let mut lines: Vec<String> = text
        .lines()
        .filter(|l| !l.contains(&marker(name)))
        .map(str::to_string)
        .collect();
    lines.push(entry);
    write_crontab(&lines.join("\n"))
}

/// Take `name`'s entry back out. A missing entry is not an error.
pub fn remove(name: &str) -> Result<()> {
    let Some(text) = current_crontab() else {
        return Ok(());
    };
    let lines: Vec<&str> = text
        .lines()
        .filter(|l| !l.contains(&marker(name)))
        .collect();
    write_crontab(&lines.join("\n"))
}

fn current_crontab() -> Option<String> {
    let out = std::process::Command::new("crontab")
        .arg("-l")
        .output()
        .ok()?;
    if !out.status.success() {
        // Exit 1 with no output means the user has no crontab yet.
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn write_crontab(content: &str) -> Result<()> {
    use std::io::Write;
    let mut child = std::process::Command::new("crontab")
        .arg("-")
        .stdin(std::process::Stdio::piped())
        .spawn()
        .context("running crontab")?;
    child
        .stdin
        .take()
        .context("crontab stdin")?
        .write_all(content.as_bytes())
        .context("writing crontab")?;
    let status = child.wait().context("waiting for crontab")?;
    anyhow::ensure!(status.success(), "crontab rejected the new table");
    Ok(())
}

#[cfg(test)]
#[path = "tests/crontab_test.rs"]
mod tests;
