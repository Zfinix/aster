//! Tiered directory listing: `fd` → hand-rolled `fs::read_dir`.

use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::ToolProbe;

/// List entries of `base` up to `max_entries`. Directories end with `/`.
/// Tries `fd`, then falls back to `fs::read_dir`.
/// `fd` hides `.gitignore`d entries and the manual fallback does not, so the
/// same directory listed differently depending on what was installed. Retry
/// with `--no-ignore` when the filtered pass comes back empty, matching how
/// `find` and `search` reach ignored paths.
pub fn list(probe: &ToolProbe, base: &Path, max_entries: usize) -> Result<String> {
    if let Some(fd) = &probe.fd {
        let entries = list_with_fd(fd, base, max_entries, true)?;
        if !entries.is_empty() {
            return Ok(entries);
        }
        return list_with_fd(fd, base, max_entries, false);
    }
    list_manual(base, max_entries)
}

/// Shell out to `fd`. Faster on large directories.
fn list_with_fd(
    fd: &Path,
    base: &Path,
    max_entries: usize,
    respect_ignore: bool,
) -> Result<String> {
    let mut cmd = Command::new(fd);
    cmd.args([
        "--color",
        "never",
        "--max-results",
        &max_entries.to_string(),
    ]);
    if !respect_ignore {
        cmd.arg("--no-ignore").arg("--hidden");
        for dir in crate::SKIP_DIRS {
            cmd.arg("--exclude").arg(dir);
        }
    }
    let output = cmd.arg(base).output().context("running fd")?;

    if !output.status.success() && !output.status.code().is_some_and(|c| c == 1) {
        bail!(
            "fd failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut entries: Vec<String> = stdout
        .lines()
        .map(|line| {
            let path = Path::new(line);
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| line.to_string());
            if path.is_dir() {
                format!("{name}/")
            } else {
                name
            }
        })
        .collect();
    entries.sort();
    entries.truncate(max_entries);
    if entries.is_empty() {
        return Ok(String::new());
    }
    Ok(entries.join("\n"))
}

/// Hand-rolled fallback: `fs::read_dir` + sort + truncate.
fn list_manual(base: &Path, max_entries: usize) -> Result<String> {
    let mut entries: Vec<String> = fs::read_dir(base)
        .with_context(|| format!("listing {}", base.display()))?
        .filter_map(|e| e.ok())
        .map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if e.path().is_dir() {
                format!("{name}/")
            } else {
                name
            }
        })
        .collect();
    entries.sort();
    entries.truncate(max_entries);
    Ok(entries.join("\n"))
}

#[cfg(test)]
#[path = "tests/list_test.rs"]
mod tests;
