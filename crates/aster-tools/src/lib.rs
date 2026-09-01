#![forbid(unsafe_code)]
//! Tool detection and tiered dispatch: `search_files`, `list_files`, and
//! `find_files` delegate here so they get `rg` / `fd` when present and fall back
//! when not. No policy dependency; secret-file filtering stays in the caller.

mod find;
mod list;
mod search;
mod suggest;

pub use find::find;
pub use list::list;
pub use search::{Hit, render, search};
pub use suggest::suggest;

use std::path::PathBuf;

pub(crate) use aster_models::SKIP_DIRS;

/// True when `entry` is one of [`SKIP_DIRS`], for use with `filter_entry`.
pub(crate) fn is_skipped(entry: &ignore::DirEntry) -> bool {
    entry.file_type().is_some_and(|t| t.is_dir())
        && entry
            .file_name()
            .to_str()
            .is_some_and(|name| SKIP_DIRS.contains(&name))
}

/// One-time probe for CLI tools on `PATH`. Cheap to clone; thread it as
/// `&ToolProbe` into tool functions.
#[derive(Debug, Clone, Default)]
pub struct ToolProbe {
    pub rg: Option<PathBuf>,
    pub fd: Option<PathBuf>,
}

impl ToolProbe {
    /// Detect which tools are available. Logs findings at `debug` level.
    pub fn detect() -> Self {
        let rg = which::which("rg").ok();
        let fd = which::which("fd").ok();
        if rg.is_some() {
            tracing::debug!("rg detected on PATH");
        }
        if fd.is_some() {
            tracing::debug!("fd detected on PATH");
        }
        Self { rg, fd }
    }
}
