//! Machine-managed, per-repo record of directories approved for out-of-repo access.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// Approved directories for one repository, newest write wins.
pub struct GrantStore {
    path: PathBuf,
}

impl GrantStore {
    pub(crate) fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// Every persisted directory. A missing or unreadable file reads as empty:
    /// losing grants costs a prompt, while failing the run costs the turn.
    pub fn load(&self) -> Vec<PathBuf> {
        let Ok(text) = std::fs::read_to_string(&self.path) else {
            return Vec::new();
        };
        match serde_json::from_str::<BTreeSet<PathBuf>>(&text) {
            Ok(dirs) => dirs.into_iter().collect(),
            Err(e) => {
                tracing::warn!("ignoring unreadable grants at {}: {e}", self.path.display());
                Vec::new()
            }
        }
    }

    /// Add `dir` to the persisted set. Storing a `BTreeSet` keeps the file
    /// stable across writes, so it diffs cleanly if anyone inspects it.
    pub fn add(&self, dir: &Path) -> Result<()> {
        let mut dirs: BTreeSet<PathBuf> = self.load().into_iter().collect();
        if !dirs.insert(dir.to_path_buf()) {
            return Ok(());
        }
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        let text = serde_json::to_string_pretty(&dirs).context("serializing grants")?;
        std::fs::write(&self.path, text).with_context(|| format!("writing {}", self.path.display()))
    }
}

#[cfg(test)]
#[path = "tests/grants_test.rs"]
mod tests;
