use std::path::PathBuf;
use std::sync::{Arc, Mutex, Once};
use std::{fs, io};

use anyhow::{Context, Result};
use aster_persist::{SessionWriter, Store};

pub type Recorder = Arc<Mutex<SessionWriter>>;

/// User-global data: credentials, sessions, memory, skills. Config
/// (`aster.yaml`, `mcp.json`, `.env`) stays in `~/.aster`.
pub fn home() -> Result<PathBuf> {
    let dir = data_root()?.join("aster");
    migrate_legacy_homes(&dir);
    Ok(dir)
}

/// The XDG data dir other coding tools share: `~/.local/share` on every
/// platform, unless `XDG_DATA_HOME` overrides it.
fn data_root() -> Result<PathBuf> {
    if let Some(dir) = std::env::var_os("XDG_DATA_HOME").filter(|d| !d.is_empty()) {
        return Ok(PathBuf::from(dir));
    }
    Ok(dirs::home_dir()
        .context("could not determine home directory")?
        .join(".local/share"))
}

/// Where this data used to live: `~/.aster`, and `<config>/aster` before that.
/// Newest first, so its files win a collision during migration.
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

/// Copy legacy data across the first time the new path is asked for, so a
/// login, session, or memory from an older build is not silently orphaned.
/// A stamp makes it truly one-time: re-copying would resurrect deleted data.
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

/// Copy a legacy home's data into the new one, leaving config files behind:
/// they keep living in `~/.aster`, only data moves.
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

/// Copy every entry under `from` into `to`, keeping anything that is already
/// there. The source is left in place: losing a credential to a half-finished
/// move is worse than a stale directory.
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
mod tests {
    use super::*;

    #[test]
    fn merge_keeps_what_the_new_home_already_has() {
        let root = tempfile::tempdir().unwrap();
        let old = root.path().join("old");
        let new = root.path().join("new");
        fs::create_dir_all(old.join("sessions")).unwrap();
        fs::write(old.join("credentials.json"), "old").unwrap();
        fs::write(old.join("sessions/a.jsonl"), "a").unwrap();
        fs::create_dir_all(&new).unwrap();
        fs::write(new.join("credentials.json"), "new").unwrap();

        merge_dir(&old, &new).unwrap();

        assert_eq!(
            fs::read_to_string(new.join("credentials.json")).unwrap(),
            "new"
        );
        assert_eq!(
            fs::read_to_string(new.join("sessions/a.jsonl")).unwrap(),
            "a"
        );
        assert!(old.exists(), "the old directory is left alone");
    }

    #[test]
    fn migration_moves_data_but_leaves_config_behind() {
        let root = tempfile::tempdir().unwrap();
        let old = root.path().join("old");
        let new = root.path().join("new");
        fs::create_dir_all(old.join("sessions")).unwrap();
        fs::write(old.join("sessions/a.jsonl"), "a").unwrap();
        fs::write(old.join("credentials.json"), "cred").unwrap();
        fs::write(old.join("aster.yaml"), "review: {}").unwrap();
        fs::write(old.join("mcp.json"), "{}").unwrap();

        migrate_data(&old, &new).unwrap();

        assert!(new.join("sessions/a.jsonl").exists());
        assert!(new.join("credentials.json").exists());
        assert!(!new.join("aster.yaml").exists(), "config stays in ~/.aster");
        assert!(!new.join("mcp.json").exists());
    }
}
