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
    /// Credential directories the user approved for this command. Subtracted
    /// from the credential deny list; hard-denied paths are never listed
    /// here, so approving one is impossible rather than merely refused.
    pub allowed_credentials: Vec<PathBuf>,
}

impl SandboxProfile {
    pub fn new(repo_root: &Path) -> Self {
        Self {
            repo_root: repo_root.to_path_buf(),
            readable_dirs: Vec::new(),
            writable_dirs: Vec::new(),
            network: false,
            timeout_secs: 30,
            allowed_credentials: Vec::new(),
        }
    }

    /// Grant read access to credential directories the user approved for this
    /// command. Anything not in [`credential_paths`] is ignored.
    pub fn allow_credentials(mut self, dirs: Vec<PathBuf>) -> Self {
        let known = credential_paths();
        self.allowed_credentials = resolve_existing(dirs)
            .into_iter()
            .filter(|dir| known.contains(dir))
            .collect();
        self
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

        let denied: Vec<PathBuf> = hard_denied()
            .into_iter()
            .chain(credential_paths())
            .filter(|path| !self.allowed_credentials.contains(path))
            .collect();
        if !denied.is_empty() {
            profile.push_str("(deny file-read* file-write*");
            for path in denied {
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

        // `$HOME` is not bound at all here, so an approved credential
        // directory has to be mounted in explicitly.
        for dir in &self.allowed_credentials {
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

/// Credential stores no command may read, approved or not. Under bwrap these
/// need no rule: `$HOME` is simply not bound into the namespace.
#[cfg(target_os = "macos")]
fn hard_denied() -> Vec<PathBuf> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };
    resolve_existing(["Library/Keychains"].map(|d| home.join(d)).to_vec())
}

/// Credential stores a command reaches only with the user's approval. A tool
/// that legitimately needs one names it through [`credentials_for`]; the host
/// asks, and passes what was approved to [`SandboxProfile::allow_credentials`].
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub fn credential_paths() -> Vec<PathBuf> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };
    resolve_existing(
        [".ssh", ".aws", ".gnupg", ".config/gh", ".kube"]
            .map(|d| home.join(d))
            .to_vec(),
    )
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn credential_paths() -> Vec<PathBuf> {
    Vec::new()
}

/// The credential directories `binary` legitimately needs, if any. Matching is
/// on the file name, so an absolute path resolves the same as a bare name.
/// `git` only reaches for keys when it talks to a remote or signs.
pub fn credentials_for(binary: &str, args: &[String]) -> Vec<PathBuf> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };
    let wanted: &[&str] = match command_name(binary).as_str() {
        "gh" => &[".config/gh"],
        "aws" | "sam" => &[".aws"],
        "kubectl" | "helm" | "k9s" => &[".kube"],
        "ssh" | "scp" | "sftp" | "ssh-add" => &[".ssh"],
        "gpg" | "gpg2" => &[".gnupg"],
        "git" if reaches_remote(args) => &[".ssh", ".gnupg"],
        _ => &[],
    };
    resolve_existing(wanted.iter().map(|d| home.join(d)).collect())
}

/// The name a command is keyed by: its file name, without a Windows suffix, so
/// `/opt/homebrew/bin/gh` and `gh` are the same command.
pub fn command_name(binary: &str) -> String {
    let name = Path::new(binary)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| binary.to_string());
    name.strip_suffix(".exe").unwrap_or(&name).to_string()
}

/// Whether a `git` invocation is one that needs a key: talking to a remote, or
/// signing. Everything else stays inside the repository.
fn reaches_remote(args: &[String]) -> bool {
    const REMOTE: [&str; 8] = [
        "push",
        "fetch",
        "pull",
        "clone",
        "ls-remote",
        "remote",
        "commit",
        "tag",
    ];
    args.iter()
        .find(|arg| !arg.starts_with('-'))
        .is_some_and(|sub| REMOTE.contains(&sub.as_str()))
}

/// Resolve paths through symlinks and drop the missing ones: Seatbelt rejects
/// the whole profile over one unresolvable `subpath`, and on macOS `/tmp` and
/// `/var` are symlinks into `/private`.
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
