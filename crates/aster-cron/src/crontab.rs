//! Linux crontab backend: one line per schedule, managed through `crontab -`.

use anyhow::{Context, Result};

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

/// Android and the smaller container images ship no cron at all, and the bare
/// spawn error reads as a bug in aster rather than a fact about the machine.
pub(crate) fn missing_cron(err: std::io::Error) -> anyhow::Error {
    use std::io::ErrorKind::{NotFound, PermissionDenied};
    if matches!(err.kind(), NotFound | PermissionDenied) {
        anyhow::anyhow!(
            "this machine has no cron: `crontab` could not be run, so a schedule \
             cannot be installed here. Run it from a machine that has cron, or keep \
             the agent awake and let it schedule its own follow-up."
        )
    } else {
        anyhow::Error::new(err).context("running crontab")
    }
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
        .map_err(missing_cron)?;
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
