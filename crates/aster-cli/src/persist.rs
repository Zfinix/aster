use std::path::PathBuf;
use std::sync::{Arc, Mutex, Once};
use std::{fs, io};

use anyhow::{Context, Result};
use aster_persist::{SessionWriter, Store};

pub type Recorder = Arc<Mutex<SessionWriter>>;

/// Everything user-global lives here: credentials, sessions, memory, skills.
pub fn home() -> Result<PathBuf> {
    let dir = dirs::home_dir()
        .context("could not determine home directory")?
        .join(".aster");
    migrate_legacy_home(&dir);
    Ok(dir)
}

/// Where this data used to live, before it moved under `~/.aster`.
fn legacy_home() -> Option<PathBuf> {
    dirs::config_dir().map(|dir| dir.join("aster"))
}

/// Move `<config>/aster` across the first time the new path is asked for, so a
/// login, session, or memory from an older build is not silently orphaned.
/// Runs once per process and never overwrites anything already migrated.
fn migrate_legacy_home(new: &PathBuf) {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let Some(old) = legacy_home() else {
            return;
        };
        if !old.is_dir() || old == *new {
            return;
        }
        if let Err(e) = merge_dir(&old, new) {
            tracing::warn!(
                "could not migrate {} to {}: {e:#}",
                old.display(),
                new.display()
            );
        }
    });
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
}
