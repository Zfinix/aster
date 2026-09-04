use std::path::PathBuf;
use std::sync::{Arc, Mutex, Once};
use std::{fs, io};

use anyhow::{Context, Result};
use aster_persist::{SessionWriter, Store};

pub type Recorder = Arc<Mutex<SessionWriter>>;

/// The global `.env` Aster loads at startup. Config stays in `~/.aster` even
/// though data moved under the XDG root.
pub fn global_env_path() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".aster/.env"))
}

/// User-global data: credentials, sessions, memory, skills. Config
/// (`aster.yaml`, `mcp.json`, `.env`) stays in `~/.aster`.
pub fn home() -> Result<PathBuf> {
    let dir = data_root()?.join("aster");
    migrate_legacy_homes(&dir);
    Ok(dir)
}

fn data_root() -> Result<PathBuf> {
    if let Some(dir) = std::env::var_os("XDG_DATA_HOME").filter(|d| !d.is_empty()) {
        return Ok(PathBuf::from(dir));
    }
    Ok(dirs::home_dir()
        .context("could not determine home directory")?
        .join(".local/share"))
}

fn legacy_homes() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(home) = dirs::home_dir() {
        out.push(home.join(".aster"));
    }
    if let Some(config) = dirs::config_dir() {
        out.push(config.join("aster"));
    }
    out
}

fn migrate_legacy_homes(new: &PathBuf) {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let stamp = new.join(".legacy-migrated");
        if stamp.exists() {
            return;
        }
        let mut all_ok = true;
        for old in legacy_homes() {
            if !old.is_dir() || old == *new {
                continue;
            }
            if let Err(e) = migrate_data(&old, new) {
                all_ok = false;
                tracing::warn!(
                    "could not migrate {} to {}: {e:#}",
                    old.display(),
                    new.display()
                );
            }
        }
        // A failed pass leaves no stamp, so the next run retries.
        if all_ok && let Err(e) = fs::create_dir_all(new).and_then(|()| fs::write(&stamp, "")) {
            tracing::warn!("could not stamp migration at {}: {e}", stamp.display());
        }
    });
}

fn migrate_data(from: &PathBuf, to: &PathBuf) -> io::Result<()> {
    const CONFIG: &[&str] = &["aster.yaml", "aster.yml", ".aster.yaml", "mcp.json", ".env"];
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let name = entry.file_name();
        if CONFIG.iter().any(|c| name == *c) {
            continue;
        }
        let target = to.join(&name);
        if entry.file_type()?.is_dir() {
            merge_dir(&entry.path(), &target)?;
        } else if !target.exists() {
            fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

fn merge_dir(from: &PathBuf, to: &PathBuf) -> io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            merge_dir(&entry.path(), &target)?;
        } else if !target.exists() {
            fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

pub fn store() -> Result<Store> {
    Store::open(home()?)
}

#[cfg(test)]
#[path = "tests/persist_test.rs"]
mod tests;
