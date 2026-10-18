use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Once};
use std::{fs, io};

use anyhow::{Context, Result};
use aster_persist::{SessionWriter, Store};

pub type Recorder = Arc<Mutex<SessionWriter>>;

const CONFIG_FILES: &[&str] = &["aster.yaml", "aster.yml", ".aster.yaml", "mcp.json", ".env"];

/// The global `.env` Aster loads at startup. Config stays in `~/.aster` even
/// though data moved under the XDG root. Older builds kept these files in
/// the platform config directory; they are copied across before this path
/// is returned so a stored key is not silently dropped.
pub fn global_env_path() -> Option<PathBuf> {
    migrate_legacy_config();
    dirs::home_dir().map(|home| home.join(".aster/.env"))
}

/// User-global data: credentials, sessions, memory, skills. Config
/// (`aster.yaml`, `mcp.json`, `.env`) stays in `~/.aster`.
pub fn home() -> Result<PathBuf> {
    migrate_legacy_config();
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

fn migrate_legacy_config() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let Some(new) = dirs::home_dir().map(|home| home.join(".aster")) else {
            return;
        };
        let stamp = new.join(".legacy-config-migrated");
        if stamp.exists() {
            return;
        }
        let mut all_ok = true;
        for old in legacy_config_dirs() {
            if !old.is_dir() || old == new {
                continue;
            }
            if let Err(e) = migrate_config(&old, &new) {
                all_ok = false;
                tracing::warn!(
                    "could not migrate config {} to {}: {e:#}",
                    old.display(),
                    new.display()
                );
            }
        }
        if all_ok && let Err(e) = fs::create_dir_all(&new).and_then(|()| fs::write(&stamp, "")) {
            tracing::warn!(
                "could not stamp config migration at {}: {e}",
                stamp.display()
            );
        }
    });
}

fn legacy_config_dirs() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(config) = dirs::config_dir() {
        out.push(config.join("aster"));
    }
    out
}

fn migrate_config(from: &Path, to: &Path) -> io::Result<()> {
    fs::create_dir_all(to)?;
    for name in CONFIG_FILES {
        let src = from.join(name);
        if !src.is_file() {
            continue;
        }
        let dest = to.join(name);
        if dest.exists() {
            continue;
        }
        fs::copy(&src, &dest)?;
    }
    Ok(())
}

fn migrate_data(from: &PathBuf, to: &PathBuf) -> io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let name = entry.file_name();
        if CONFIG_FILES.iter().any(|c| name == *c) {
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

/// The memory backend for this session: the block store by default, the agent
/// memory repo when `memory.agent_repo` is on in settings.
pub fn memory() -> Result<aster_persist::Memory> {
    memory_in(&store()?)
}

/// Same, against a store the caller already holds (the TUI keeps one open).
pub fn memory_in(store: &Store) -> Result<aster_persist::Memory> {
    let cfg = crate::settings::Settings::load(None)
        .map(|s| s.memory)
        .unwrap_or_default();
    if !cfg.agent_repo.unwrap_or(false) {
        return Ok(store.memory_with(None));
    }
    Ok(store.memory_with(Some(amr_path()?)))
}

/// The agent memory repo path from settings, whether or not the backend is on.
/// `aster memory import` writes there even before the toggle is flipped.
pub fn amr_path() -> Result<PathBuf> {
    let cfg = crate::settings::Settings::load(None)
        .map(|s| s.memory)
        .unwrap_or_default();
    match cfg.repo_path.filter(|p| !p.trim().is_empty()) {
        Some(p) => match p.trim().strip_prefix("~/") {
            Some(rest) => Ok(dirs::home_dir()
                .context("could not determine home directory for the agent memory repo")?
                .join(rest)),
            None => Ok(PathBuf::from(p.trim().to_string())),
        },
        None => Ok(dirs::home_dir()
            .context("could not determine home directory for the agent memory repo")?
            .join("agent-memory")),
    }
}

#[cfg(test)]
#[path = "tests/persist_test.rs"]
mod tests;
