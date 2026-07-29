#![forbid(unsafe_code)]
//! OS-native sandboxing for running untrusted commands safely.
//!
//! On macOS, uses `sandbox-exec` (Seatbelt) to restrict filesystem writes to
//! granted directories and block network access. On Linux, uses `bwrap`
//! (bubblewrap) when available. On other platforms, degrades to a
//! process-level sandbox: filtered environment, cwd set, timeout, and no
//! network isolation — with a warning logged.
//!
//! The sandbox is a boundary, not a guarantee. The caller must still evaluate
//! policy and approval before running a command.

mod profile;
mod runner;

pub use profile::SandboxProfile;
pub use runner::{CommandOutput, SandboxConfig, run_command};

use std::path::PathBuf;

/// Which sandbox backend is active on this platform
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxBackend {
    /// macOS Seatbelt via `sandbox-exec`.
    Seatbelt,
    /// Linux `bubblewrap` via `bwrap`.
    Bubblewrap,
    /// Process-level only: no OS-enforced isolation.
    ProcessLevel,
}

/// Detect the best available sandbox backend on the current platform.
pub fn detect_backend() -> SandboxBackend {
    #[cfg(target_os = "macos")]
    {
        if binary_on_path("sandbox-exec") {
            return SandboxBackend::Seatbelt;
        }
    }
    #[cfg(target_os = "linux")]
    {
        if binary_on_path("bwrap") {
            return SandboxBackend::Bubblewrap;
        }
    }
    SandboxBackend::ProcessLevel
}

/// Whether the current platform has a real OS-level sandbox available.
pub fn has_os_sandbox() -> bool {
    !matches!(detect_backend(), SandboxBackend::ProcessLevel)
}

/// Check if a binary exists on PATH by running `which`.
fn binary_on_path(binary: &str) -> bool {
    std::process::Command::new("which")
        .arg(binary)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Resolve a binary path on PATH (used by the runner to find sandbox tools).
#[allow(dead_code)]
fn which_binary(binary: &str) -> Option<PathBuf> {
    let output = std::process::Command::new("which")
        .arg(binary)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if path.is_empty() {
        None
    } else {
        Some(PathBuf::from(path))
    }
}
