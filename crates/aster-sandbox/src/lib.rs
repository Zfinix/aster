#![forbid(unsafe_code)]
//! OS-native sandboxing for running untrusted commands. `sandbox-exec` (Seatbelt)
//! on macOS, `bwrap` on Linux, elsewhere a process-level fallback with no network
//! isolation. A boundary, not a guarantee: policy and approval come first.

mod profile;
mod runner;

pub use profile::{SandboxProfile, command_name, credential_paths, credentials_for};
pub use runner::{CommandOutput, SandboxConfig, run_command};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxBackend {
    /// macOS Seatbelt via `sandbox-exec`.
    Seatbelt,
    /// Linux `bubblewrap` via `bwrap`.
    Bubblewrap,
    /// Process-level only: no OS-enforced isolation.
    ProcessLevel,
}

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

pub fn has_os_sandbox() -> bool {
    !matches!(detect_backend(), SandboxBackend::ProcessLevel)
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn binary_on_path(binary: &str) -> bool {
    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&paths).any(|dir| dir.join(binary).is_file())
}
