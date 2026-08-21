//! Where this server keeps what it has to remember. The same place the CLI
//! keeps its own durable state, so one Aster's files are all in one directory.

use std::path::PathBuf;

/// `$XDG_DATA_HOME/aster`, or `~/.local/share/aster`. Mirrors the CLI's own
/// home; `None` only on a machine with neither.
pub fn home() -> Option<PathBuf> {
    let root = match std::env::var_os("XDG_DATA_HOME").filter(|dir| !dir.is_empty()) {
        Some(dir) => PathBuf::from(dir),
        None => dirs::home_dir()?.join(".local/share"),
    };
    Some(root.join("aster"))
}
