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

/// Check if a binary exists on PATH.
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn binary_on_path(binary: &str) -> bool {
    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&paths).any(|dir| dir.join(binary).is_file())
}
