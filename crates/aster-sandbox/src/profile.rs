//! Sandbox profile generation for OS-native isolation.
//!
//! On macOS, generates a Seatbelt (sandbox-exec) profile that allows most
//! operations but restricts filesystem writes to the repo root and temp
//! directories. On Linux, generates bwrap args that achieve the same effect.

use std::path::{Path, PathBuf};

/// Describes the filesystem and network boundaries for a sandboxed command.
#[derive(Debug, Clone)]
pub struct SandboxProfile {
    /// The repository root: readable and writable.
    pub repo_root: PathBuf,
    /// Additional directories the command may read. Only enforced by bwrap;
    /// Seatbelt allows reads everywhere and restricts writes and network.
    pub readable_dirs: Vec<PathBuf>,
    /// Additional directories the command may write to (beyond repo and temp).
    pub writable_dirs: Vec<PathBuf>,
    /// Whether network access is allowed.
    pub network: bool,
    /// Maximum execution time in seconds.
    pub timeout_secs: u64,
}

impl SandboxProfile {
    pub fn new(repo_root: &Path) -> Self {
        Self {
            repo_root: repo_root.to_path_buf(),
            readable_dirs: Vec::new(),
            writable_dirs: Vec::new(),
            network: false,
            timeout_secs: 30,
        }
    }

    pub fn readable(mut self, dir: PathBuf) -> Self {
        self.readable_dirs.push(dir);
        self
    }

    pub fn writable(mut self, dir: PathBuf) -> Self {
        self.writable_dirs.push(dir);
        self
    }

    pub fn network(mut self, allow: bool) -> Self {
        self.network = allow;
        self
    }

    pub fn timeout(mut self, secs: u64) -> Self {
        self.timeout_secs = secs;
        self
    }

    /// Generate a macOS Seatbelt profile string for `sandbox-exec -p`.
    ///
    /// Starts from `(allow default)`, denies all writes, then re-allows the
    /// writable set. Seatbelt takes the last matching rule, so ordering does
    /// the work; the alternative, one deny with `(not (subpath ...))` filters,
    /// ORs its filters together and denies everything.
    #[cfg(target_os = "macos")]
    pub fn seatbelt_profile(&self) -> String {
        let mut profile = String::new();
        profile.push_str("(version 1)\n");
        profile.push_str("(allow default)\n");
        profile.push_str("(deny file-write*)\n");
        // Commands routinely redirect to /dev/null and write to their tty.
        profile.push_str(
            "(allow file-write-data (require-all (path \"/dev/null\") (vnode-type CHARACTER-DEVICE)))\n",
        );
        profile.push_str("(allow file-write* (subpath \"/dev\"))\n");

        let writable = self.writable_paths();
        if !writable.is_empty() {
            profile.push_str("(allow file-write*");
            for dir in writable {
                profile.push_str(&format!(" (subpath \"{}\")", escape(&dir)));
            }
            profile.push_str(")\n");
        }

        // After the repo allow, so the last-match rule makes these win.
        let protected = self.protected_repo_paths();
        if !protected.is_empty() {
            profile.push_str("(deny file-write*");
            for path in protected {
                profile.push_str(&format!(" (subpath \"{}\")", escape(&path)));
            }
            profile.push_str(")\n");
        }

        let sensitive = sensitive_read_paths();
        if !sensitive.is_empty() {
            profile.push_str("(deny file-read* file-write*");
            for path in sensitive {
                profile.push_str(&format!(" (subpath \"{}\")", escape(&path)));
            }
            profile.push_str(")\n");
        }

        if !self.network {
            profile.push_str("(deny network*)\n");
        }

        profile
    }

    /// Repo paths that stay read-only even though the repo is writable: a git
    /// hook or CI workflow written in the sandbox would run unsandboxed later.
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    fn protected_repo_paths(&self) -> Vec<PathBuf> {
        resolve_existing(
            [".git/hooks", ".git/config", ".github/workflows"]
                .map(|p| self.repo_root.join(p))
                .to_vec(),
        )
    }

    /// All directories the command may write to: repo root, temp dirs, and any
    /// explicitly writable dirs.
    #[cfg(target_os = "macos")]
    fn writable_paths(&self) -> Vec<PathBuf> {
        let mut paths = vec![self.repo_root.clone()];
        paths.extend(self.writable_dirs.clone());
        paths.extend(["/tmp", "/var/folders", "/var/tmp"].map(PathBuf::from));
        // Build caches, without which cargo, npm, and friends fail on their
        // first fetch. Everything else under $HOME stays read-only.
        if let Some(home) = dirs::home_dir() {
            paths.extend(
                [
                    ".cargo",
                    ".rustup",
                    ".npm",
                    ".bun",
                    ".yarn",
                    "Library/pnpm",
                    ".cache",
                    "Library/Caches",
                ]
                .map(|d| home.join(d)),
            );
        }
        resolve_existing(paths)
    }

    /// Generate `bwrap` arguments for Linux.
    #[cfg(target_os = "linux")]
    pub fn bwrap_args(&self) -> Vec<String> {
        let mut args = vec![
            "--ro-bind".to_string(),
            "/usr".to_string(),
            "/usr".to_string(),
            "--ro-bind".to_string(),
            "/lib".to_string(),
            "/lib".to_string(),
            "--ro-bind".to_string(),
            "/bin".to_string(),
            "/bin".to_string(),
            "--symlink".to_string(),
            "usr/lib64".to_string(),
            "/lib64".to_string(),
            "--proc".to_string(),
            "/proc".to_string(),
            "--dev".to_string(),
            "/dev".to_string(),
            "--bind".to_string(),
            self.repo_root.display().to_string(),
            self.repo_root.display().to_string(),
        ];

        for dir in &self.readable_dirs {
            args.push("--ro-bind".into());
            args.push(dir.display().to_string());
            args.push(dir.display().to_string());
        }

        for dir in &self.writable_dirs {
            args.push("--bind".into());
            args.push(dir.display().to_string());
            args.push(dir.display().to_string());
        }

        // Later binds mount over the repo bind, making these read-only.
        for path in self.protected_repo_paths() {
            args.push("--ro-bind".into());
            args.push(path.display().to_string());
            args.push(path.display().to_string());
        }

        // Bind /tmp writable for builds and tests.
        args.push("--bind".into());
        args.push("/tmp".into());
        args.push("/tmp".into());

        if !self.network {
            args.push("--unshare-net".into());
        }

        args.push("--die-with-parent".into());

        args
    }
}

/// Credential stores a sandboxed command has no business reading. Under bwrap
/// these need no rule: `$HOME` is simply not bound into the namespace.
#[cfg(target_os = "macos")]
fn sensitive_read_paths() -> Vec<PathBuf> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };
    resolve_existing(
        [
            ".ssh",
            ".aws",
            ".gnupg",
            ".config/gh",
            ".kube",
            "Library/Keychains",
        ]
        .map(|d| home.join(d))
        .to_vec(),
    )
}

/// Resolve paths through symlinks and drop the missing ones: Seatbelt rejects
/// the whole profile over one unresolvable `subpath`, and on macOS `/tmp` and
/// `/var` are symlinks into `/private`.
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn resolve_existing(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for path in paths {
        let Ok(resolved) = path.canonicalize() else {
            continue;
        };
        if !out.contains(&resolved) {
            out.push(resolved);
        }
    }
    out
}

/// Quote a path for a Seatbelt string literal.
#[cfg(target_os = "macos")]
fn escape(path: &Path) -> String {
    path.display()
        .to_string()
        .replace('\\', r"\\")
        .replace('"', "\\\"")
}

#[cfg(test)]
#[path = "tests/profile_test.rs"]
mod tests;
