#![forbid(unsafe_code)]
//! Tool detection and tiered dispatch for the best available CLI tool on
//! each platform. The chat agent's `search_files`, `list_files`, and
//! `find_files` delegate here so they get `rg` / `fd` when present and fall
//! back to embedded or hand-rolled implementations when they are not.
//!
//! This crate has no policy dependency. Secret-file filtering stays in the
//! caller.

mod find;
mod list;
mod search;
mod suggest;

pub use find::find;
pub use list::list;
pub use search::search;
pub use suggest::suggest;

use std::path::PathBuf;

/// One-time probe for CLI tools on `PATH`. Cheap to clone; thread it as
/// `&ToolProbe` into tool functions.
#[derive(Debug, Clone, Default)]
pub struct ToolProbe {
    /// Path to `rg` if found on `PATH`.
    pub rg: Option<PathBuf>,
    /// Path to `fd` if found on `PATH`.
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
