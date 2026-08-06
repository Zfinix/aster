//! Command execution inside the sandbox.

use std::time::Duration;

use anyhow::{Context, Result, bail};
use tokio::io::AsyncReadExt;
use tokio::process::Command as TokioCommand;

use crate::SandboxBackend;
use crate::SandboxProfile;
use crate::detect_backend;

/// Configuration for running a command in the sandbox.
#[derive(Debug, Clone)]
pub struct SandboxConfig {
    pub profile: SandboxProfile,
    /// Environment variables to set (in addition to a filtered inherited set).
    pub env: Vec<(String, String)>,
    /// Environment variables to explicitly remove.
    pub unset_env: Vec<String>,
}

impl SandboxConfig {
    pub fn new(profile: SandboxProfile) -> Self {
        Self {
            profile,
            env: Vec::new(),
            unset_env: Vec::new(),
        }
    }

    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }

    pub fn unset(mut self, key: impl Into<String>) -> Self {
        self.unset_env.push(key.into());
        self
    }
}

/// The output of a sandboxed command.
#[derive(Debug, Clone)]
pub struct CommandOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
}

impl CommandOutput {
    /// True when the command exited successfully (code 0).
    pub fn success(&self) -> bool {
        self.exit_code == Some(0)
    }
}

/// Environment variables that are never inherited into the sandbox.
const DROPPED_ENV: &[&str] = &[
    "ASTER_API_KEY",
    "OPEN_ROUTER_API_KEY",
    "API_KEY",
    "GITHUB_TOKEN",
    "GH_TOKEN",
    "GITLAB_TOKEN",
    "AWS_ACCESS_KEY_ID",
    "AWS_SECRET_ACCESS_KEY",
    "ASTER_SESSION",
];

/// Environment variables re-added after `env_clear`, when set. The Windows
/// names are absent on Unix; without `SystemRoot` and friends most Windows
/// programs fail to start.
const INHERITED_ENV: &[&str] = &[
    "PATH",
    "HOME",
    "USER",
    "SHELL",
    "LANG",
    "LC_ALL",
    "TERM",
    // Dropping TMPDIR sends tools to fallback temp dirs the profile may not
    // allow; the parent's TMPDIR is under /var/folders, which is writable.
    "TMPDIR",
    "SystemRoot",
    "SystemDrive",
    "ComSpec",
    "TEMP",
    "TMP",
    "USERPROFILE",
    "APPDATA",
    "LOCALAPPDATA",
    "PATHEXT",
];

/// Run a command inside the sandbox. The command is specified as a binary
/// path and a list of arguments. The working directory is set to the repo
/// root. On timeout the child is killed and `timed_out` is set on the output.
pub async fn run_command(
    config: &SandboxConfig,
    binary: &str,
    args: &[String],
) -> Result<CommandOutput> {
    let backend = detect_backend();
    let timeout = Duration::from_secs(config.profile.timeout_secs);

    let mut cmd = match backend {
        SandboxBackend::Seatbelt => build_seatbelt_command(config, binary, args)?,
        SandboxBackend::Bubblewrap => build_bwrap_command(config, binary, args)?,
        SandboxBackend::ProcessLevel => {
            tracing::warn!("no OS sandbox available; running with process-level isolation only");
            build_process_command(config, binary, args)
        }
    };

    // Set the working directory to the repo root.
    cmd.current_dir(&config.profile.repo_root);

    // Filter environment: drop secrets and explicitly unset vars.
    cmd.env_clear();
    for (key, value) in &config.env {
        cmd.env(key, value);
    }
    // Re-add safe environment variables that commands commonly need.
    for key in INHERITED_ENV {
        if let Ok(val) = std::env::var(key)
            && !config.unset_env.iter().any(|k| k == key)
        {
            cmd.env(key, val);
        }
    }
    // Remove any dropped env that might have been re-added via config.env.
    for key in DROPPED_ENV {
        cmd.env_remove(key);
    }
    for key in &config.unset_env {
        cmd.env_remove(key);
    }

    cmd.stdin(std::process::Stdio::null());
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());
    cmd.kill_on_drop(true);
    // A fresh process group so a timeout can kill grandchildren too.
    #[cfg(unix)]
    cmd.process_group(0);

    let mut child = cmd.spawn().context("spawning sandboxed command")?;
    let pid = child.id();
    // Readers run as tasks so partial output survives a timeout; killing the
    // process group closes the pipes and lets them finish.
    let stdout_task = tokio::spawn(read_capped(child.stdout.take()));
    let stderr_task = tokio::spawn(read_capped(child.stderr.take()));

    let timed_out = tokio::time::timeout(timeout, child.wait()).await;
    if timed_out.is_err() {
        // Kill before joining the readers: a hung child holds the pipes open,
        // and the readers only finish once the pipes close.
        kill_process_group(pid);
    }
    // Bounded join: a grandchild that inherited the pipes can keep them open
    // after the child exits, so never wait on the readers indefinitely.
    let readers = async { tokio::join!(stdout_task, stderr_task) };
    let (stdout, stderr) = match tokio::time::timeout(READER_GRACE, readers).await {
        Ok((stdout, stderr)) => (stdout.unwrap_or_default(), stderr.unwrap_or_default()),
        Err(_) => {
            kill_process_group(pid);
            (String::new(), String::new())
        }
    };

    let Ok(status) = timed_out else {
        return Ok(CommandOutput {
            stdout,
            stderr,
            exit_code: None,
            timed_out: true,
        });
    };
    let status = status.context("running sandboxed command")?;

    Ok(CommandOutput {
        stdout,
        stderr,
        exit_code: status.code(),
        timed_out: false,
    })
}

/// Cap on captured bytes per stream; commands can emit gigabytes.
const MAX_CAPTURE_BYTES: usize = 2 * 1024 * 1024;

/// How long to wait for the stream readers after the child exits or is
/// killed, in case a leftover grandchild still holds the pipes open.
const READER_GRACE: Duration = Duration::from_secs(5);

/// Read a stream to the end, keeping at most [`MAX_CAPTURE_BYTES`] and
/// draining the rest so the child never blocks on a full pipe.
async fn read_capped<R>(reader: Option<R>) -> String
where
    R: tokio::io::AsyncRead + Unpin,
{
    let Some(mut reader) = reader else {
        return String::new();
    };
    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 8192];
    let mut truncated = false;
    loop {
        match reader.read(&mut chunk).await {
            Ok(0) => break,
            Ok(n) if buf.len() < MAX_CAPTURE_BYTES => {
                let take = n.min(MAX_CAPTURE_BYTES - buf.len());
                buf.extend_from_slice(&chunk[..take]);
                truncated = take < n;
            }
            Ok(_) => truncated = true,
            Err(_) => break,
        }
    }
    let mut out = String::from_utf8_lossy(&buf).into_owned();
    if truncated {
        out.push_str("\n[output truncated]");
    }
    out
}

/// SIGKILL the child's whole process group: `kill_on_drop` only reaches the
/// direct child, not grandchildren a shell left running in the background.
#[cfg(unix)]
fn kill_process_group(pid: Option<u32>) {
    use nix::sys::signal::{Signal, killpg};
    use nix::unistd::Pid;

    let Some(pid) = pid else { return };
    let Ok(pid) = i32::try_from(pid) else { return };
    let _ = killpg(Pid::from_raw(pid), Signal::SIGKILL);
}

#[cfg(not(unix))]
fn kill_process_group(_pid: Option<u32>) {}

#[cfg(target_os = "macos")]
fn build_seatbelt_command(
    config: &SandboxConfig,
    binary: &str,
    args: &[String],
) -> Result<TokioCommand> {
    let profile = config.profile.seatbelt_profile();
    let mut cmd = TokioCommand::new("sandbox-exec");
    cmd.arg("-p").arg(&profile);
    cmd.arg("--");
    cmd.arg(binary);
    for arg in args {
        cmd.arg(arg);
    }
    Ok(cmd)
}

#[cfg(not(target_os = "macos"))]
fn build_seatbelt_command(
    _config: &SandboxConfig,
    _binary: &str,
    _args: &[String],
) -> Result<TokioCommand> {
    bail!("seatbelt is only available on macOS");
}

#[cfg(target_os = "linux")]
fn build_bwrap_command(
    config: &SandboxConfig,
    binary: &str,
    args: &[String],
) -> Result<TokioCommand> {
    let bwrap_args = config.profile.bwrap_args();
    let mut cmd = TokioCommand::new("bwrap");
    for arg in &bwrap_args {
        cmd.arg(arg);
    }
    cmd.arg("--");
    cmd.arg(binary);
    for arg in args {
        cmd.arg(arg);
    }
    Ok(cmd)
}

#[cfg(not(target_os = "linux"))]
fn build_bwrap_command(
    _config: &SandboxConfig,
    _binary: &str,
    _args: &[String],
) -> Result<TokioCommand> {
    bail!("bubblewrap is only available on Linux");
}

fn build_process_command(_config: &SandboxConfig, binary: &str, args: &[String]) -> TokioCommand {
    let mut cmd = TokioCommand::new(binary);
    for arg in args {
        cmd.arg(arg);
    }
    cmd
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::path::Path;

    fn config() -> SandboxConfig {
        SandboxConfig::new(SandboxProfile::new(Path::new(".")))
    }

    /// Check if the OS sandbox is usable in this environment.
    /// (sandbox-exec can fail with permission denied when nested.)
    async fn can_run_sandboxed() -> bool {
        match detect_backend() {
            SandboxBackend::ProcessLevel => true,
            _ => {
                let cfg = SandboxConfig::new(SandboxProfile::new(Path::new(".")));
                match run_command(&cfg, "true", &[]).await {
                    Ok(out) => out.success(),
                    Err(_) => false,
                }
            }
        }
    }

    #[tokio::test]
    async fn run_simple_command() {
        if !can_run_sandboxed().await {
            return;
        }
        let cfg = config();
        let out = run_command(&cfg, "echo", &["hello".into()]).await.unwrap();
        assert!(
            out.success(),
            "exit_code={:?} stderr={}",
            out.exit_code,
            out.stderr
        );
        assert!(out.stdout.trim() == "hello", "{}", out.stdout);
    }

    #[tokio::test]
    async fn run_command_with_stderr() {
        if !can_run_sandboxed().await {
            return;
        }
        let cfg = config();
        let out = run_command(&cfg, "sh", &["-c".into(), "echo err >&2".into()])
            .await
            .unwrap();
        assert!(out.success());
        assert!(out.stderr.trim() == "err", "{}", out.stderr);
    }

    #[tokio::test]
    async fn run_command_times_out() {
        if !can_run_sandboxed().await {
            return;
        }
        let mut profile = SandboxProfile::new(Path::new("."));
        profile.timeout_secs = 1;
        let cfg = SandboxConfig::new(profile);
        let out = run_command(&cfg, "sleep", &["10".into()]).await.unwrap();
        assert!(out.timed_out, "should have timed out");
        assert!(!out.success());
        assert_eq!(out.exit_code, None);
    }

    #[tokio::test]
    async fn run_command_times_out_keeps_partial_output() {
        if !can_run_sandboxed().await {
            return;
        }
        let mut profile = SandboxProfile::new(Path::new("."));
        profile.timeout_secs = 1;
        let cfg = SandboxConfig::new(profile);
        let script = "echo partial-stdout; echo partial-stderr >&2; sleep 10";
        let out = run_command(&cfg, "sh", &["-c".into(), script.into()])
            .await
            .unwrap();
        assert!(out.timed_out);
        assert!(out.stdout.contains("partial-stdout"), "{}", out.stdout);
        assert!(out.stderr.contains("partial-stderr"), "{}", out.stderr);
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn run_command_temp_dirs_are_writable() {
        if !can_run_sandboxed().await {
            return;
        }
        let cfg = config();
        // The per-user dir from confstr(_CS_DARWIN_USER_TEMP_DIR), which bun
        // and friends fall back to, and the inherited $TMPDIR.
        let script = "d=$(getconf DARWIN_USER_TEMP_DIR) && touch \"$d/aster_sbx_probe\" \
                      && rm \"$d/aster_sbx_probe\" \
                      && test -n \"$TMPDIR\" && touch \"$TMPDIR/aster_sbx_probe\" \
                      && rm \"$TMPDIR/aster_sbx_probe\" && echo ok";
        let out = run_command(&cfg, "sh", &["-c".into(), script.into()])
            .await
            .unwrap();
        assert!(out.success(), "stderr={}", out.stderr);
        assert!(out.stdout.contains("ok"), "{}", out.stdout);
    }

    fn process_alive(pid: &str) -> bool {
        std::process::Command::new("kill")
            .args(["-0", pid])
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }

    #[tokio::test]
    async fn run_command_times_out_kills_grandchildren() {
        if !can_run_sandboxed().await {
            return;
        }
        let repo = tempfile::tempdir().unwrap();
        let mut profile = SandboxProfile::new(repo.path());
        profile.timeout_secs = 1;
        let cfg = SandboxConfig::new(profile);
        let script = "sleep 30 & echo $! > child.pid; wait";
        let out = run_command(&cfg, "sh", &["-c".into(), script.into()])
            .await
            .unwrap();
        assert!(out.timed_out);

        let pid = std::fs::read_to_string(repo.path().join("child.pid")).unwrap();
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(
            !process_alive(pid.trim()),
            "grandchild survived the timeout"
        );
    }

    #[tokio::test]
    async fn run_command_output_is_capped() {
        if !can_run_sandboxed().await {
            return;
        }
        let cfg = config();
        // ~4MiB against the 2MiB cap.
        let script = "head -c 4194304 /dev/zero | tr '\\0' 'a'";
        let out = run_command(&cfg, "sh", &["-c".into(), script.into()])
            .await
            .unwrap();
        assert!(out.success(), "stderr={}", out.stderr);
        assert!(out.stdout.ends_with("[output truncated]"));
        assert!(out.stdout.len() < 3 * 1024 * 1024, "{}", out.stdout.len());
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn writes_to_git_hooks_and_config_are_blocked() {
        if !can_run_sandboxed().await {
            return;
        }
        let repo = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(repo.path().join(".git/hooks")).unwrap();
        std::fs::write(repo.path().join(".git/config"), "[core]\n").unwrap();
        let cfg = SandboxConfig::new(SandboxProfile::new(repo.path()));

        let out = run_command(
            &cfg,
            "sh",
            &["-c".into(), "echo x > .git/hooks/pre-commit".into()],
        )
        .await
        .unwrap();
        assert!(!out.success(), "hook write was allowed");
        assert!(!repo.path().join(".git/hooks/pre-commit").exists());

        let out = run_command(&cfg, "sh", &["-c".into(), "echo x >> .git/config".into()])
            .await
            .unwrap();
        assert!(!out.success(), "config write was allowed");

        let out = run_command(&cfg, "sh", &["-c".into(), "echo x > ok.txt".into()])
            .await
            .unwrap();
        assert!(out.success(), "repo write broke: {}", out.stderr);
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn sensitive_paths_are_unreadable() {
        if !can_run_sandboxed().await {
            return;
        }
        let Some(ssh) = dirs::home_dir().map(|h| h.join(".ssh")) else {
            return;
        };
        if !ssh.exists() {
            return;
        }
        let repo = tempfile::tempdir().unwrap();
        let cfg = SandboxConfig::new(SandboxProfile::new(repo.path()));
        let script = format!("ls {}", ssh.display());
        let out = run_command(&cfg, "sh", &["-c".into(), script])
            .await
            .unwrap();
        assert!(!out.success(), "read of ~/.ssh was allowed: {}", out.stdout);
    }

    #[tokio::test]
    async fn run_command_nonzero_exit() {
        if !can_run_sandboxed().await {
            return;
        }
        let cfg = config();
        let out = run_command(&cfg, "sh", &["-c".into(), "exit 3".into()])
            .await
            .unwrap();
        assert!(!out.success());
        assert_eq!(out.exit_code, Some(3));
    }

    #[tokio::test]
    async fn secrets_are_dropped_from_env() {
        if !can_run_sandboxed().await {
            return;
        }
        let cfg = config().env("ASTER_API_KEY", "secret123");
        let out = run_command(&cfg, "sh", &["-c".into(), "echo $ASTER_API_KEY".into()])
            .await
            .unwrap();
        assert!(
            out.stdout.trim().is_empty(),
            "secret leaked: {}",
            out.stdout
        );
    }

    #[tokio::test]
    async fn custom_env_is_passed() {
        if !can_run_sandboxed().await {
            return;
        }
        let cfg = config().env("MY_VAR", "custom_value");
        let out = run_command(&cfg, "sh", &["-c".into(), "echo $MY_VAR".into()])
            .await
            .unwrap();
        assert_eq!(out.stdout.trim(), "custom_value");
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn writes_land_inside_the_repo_and_nowhere_else() {
        if !can_run_sandboxed().await {
            return;
        }
        let repo = tempfile::tempdir().unwrap();
        // Not a temp dir: those are writable by design.
        let blocked = dirs::home_dir().unwrap().join(".aster-sandbox-leak-probe");
        let cfg = SandboxConfig::new(SandboxProfile::new(repo.path()));

        let out = run_command(&cfg, "sh", &["-c".into(), "echo ok > inside.txt".into()])
            .await
            .unwrap();
        assert!(out.success(), "stderr={}", out.stderr);
        assert!(repo.path().join("inside.txt").exists());

        let script = format!("echo leaked > {}", blocked.display());
        let out = run_command(&cfg, "sh", &["-c".into(), script])
            .await
            .unwrap();
        let leaked = blocked.exists();
        let _ = std::fs::remove_file(&blocked);
        assert!(!leaked, "the write outside the repo landed");
        assert!(!out.success(), "the write outside the repo was allowed");
    }

    #[tokio::test]
    async fn working_directory_is_repo_root() {
        if !can_run_sandboxed().await {
            return;
        }
        let profile = SandboxProfile::new(&std::env::current_dir().unwrap());
        let cfg = SandboxConfig::new(profile);
        let out = run_command(&cfg, "pwd", &[]).await.unwrap();
        let expected = std::env::current_dir().unwrap().display().to_string();
        let actual = out.stdout.trim();
        assert!(
            actual.ends_with(&expected) || expected.ends_with(actual),
            "expected {expected}, got {actual}"
        );
    }
}
