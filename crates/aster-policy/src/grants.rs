//! Directories outside the repository that the agent is allowed to touch.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

/// Out-of-repo directories the agent may read: seeded from
/// `permissions.additional_directories`, then extended at runtime each time the
/// user approves a prompt, so approving a directory once covers the rest of the
/// session.
///
/// A lookup walks the candidate's ancestors against a hash set rather than
/// scanning the grants, so the cost is the path's depth no matter how many
/// grants have accumulated, and a grant automatically covers everything nested
/// under it.
///
/// Shared as `Arc<Grants>`: `grant` takes `&self` so a tool call deep in the
/// agent loop can record an approval without threading `&mut` through it.
#[derive(Debug, Default)]
pub struct Grants {
    roots: Mutex<HashSet<PathBuf>>,
}

impl Grants {
    /// Seed from already-expanded absolute directories. Callers resolve `~` and
    /// relative entries first; this type does no filesystem work of its own.
    pub fn new(roots: impl IntoIterator<Item = PathBuf>) -> Self {
        Self {
            roots: Mutex::new(roots.into_iter().collect()),
        }
    }

    /// True when `path` is a granted directory or sits under one.
    pub fn allows(&self, path: &Path) -> bool {
        let roots = self.lock();
        path.ancestors().any(|a| roots.contains(a))
    }

    /// Record an approval. Granting a directory already covered by a broader
    /// grant is a no-op, so repeated approvals cannot grow the set unboundedly
    /// along one branch.
    pub fn grant(&self, dir: PathBuf) {
        if self.allows(&dir) {
            return;
        }
        self.lock().insert(dir);
    }

    /// Every granted directory, sorted. For display and for persisting.
    pub fn granted(&self) -> Vec<PathBuf> {
        let mut out: Vec<PathBuf> = self.lock().iter().cloned().collect();
        out.sort();
        out
    }

    /// A poisoned lock means another thread panicked mid-update. The set is
    /// still structurally sound, and losing every grant would be worse.
    fn lock(&self) -> std::sync::MutexGuard<'_, HashSet<PathBuf>> {
        self.roots.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
#[path = "grants_tests.rs"]
mod tests;
