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

/// Credential directories a *named command* may read inside the sandbox,
/// approved one at a time by the user.
///
/// Deliberately not [`Grants`]: that set widens the agent's own file tools, and
/// approving `gh` must not make `~/.config/gh` readable to `read_file` or to
/// the next `cat`. Keying on the command is the whole point, so the pair is
/// what gets stored.
#[derive(Debug, Default)]
pub struct CommandGrants {
    pairs: Mutex<HashSet<(String, PathBuf)>>,
}

impl CommandGrants {
    /// Seed from already-expanded `(command, directory)` pairs.
    pub fn new(pairs: impl IntoIterator<Item = (String, PathBuf)>) -> Self {
        Self {
            pairs: Mutex::new(pairs.into_iter().collect()),
        }
    }

    /// True when `command` may read `dir` or something under an approved parent.
    pub fn allows(&self, command: &str, dir: &Path) -> bool {
        let pairs = self.lock();
        dir.ancestors()
            .any(|a| pairs.contains(&(command.to_string(), a.to_path_buf())))
    }

    pub fn grant(&self, command: &str, dir: PathBuf) {
        if self.allows(command, &dir) {
            return;
        }
        self.lock().insert((command.to_string(), dir));
    }

    /// Every approved pair, sorted. For display and for persisting.
    pub fn granted(&self) -> Vec<(String, PathBuf)> {
        let mut out: Vec<(String, PathBuf)> = self.lock().iter().cloned().collect();
        out.sort();
        out
    }

    /// The directories `command` may read, for handing to the sandbox.
    pub fn dirs_for(&self, command: &str) -> Vec<PathBuf> {
        let mut out: Vec<PathBuf> = self
            .lock()
            .iter()
            .filter(|(name, _)| name == command)
            .map(|(_, dir)| dir.clone())
            .collect();
        out.sort();
        out
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashSet<(String, PathBuf)>> {
        self.pairs.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
#[path = "tests/grants_test.rs"]
mod tests;
