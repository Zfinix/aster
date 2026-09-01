//! Command execution inside the sandbox.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use tokio::io::AsyncReadExt;
use tokio::process::Command as TokioCommand;

use crate::SandboxBackend;
use crate::SandboxProfile;
use crate::detect_backend;

#[derive(Debug, Clone)]
pub struct SandboxConfig {
    pub profile: SandboxProfile,
    /// Environment variables to set (in addition to a filtered inherited set).
    pub env: Vec<(String, String)>,
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

#[derive(Debug, Clone)]
pub struct CommandOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
}

impl CommandOutput {
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
    let (out_buf, err_buf) = (captured(), captured());
    let stdout_task = tokio::spawn(read_capped(child.stdout.take(), out_buf.clone()));
    let stderr_task = tokio::spawn(read_capped(child.stderr.take(), err_buf.clone()));

    let timed_out = tokio::time::timeout(timeout, child.wait()).await;
    if timed_out.is_err() {
        // Kill before joining the readers: a hung child holds the pipes open,
        // and the readers only finish once the pipes close.
        kill_process_group(pid);
    }
    // Bounded join: a grandchild that inherited the pipes can keep them open
    // after the child exits, so never wait on the readers indefinitely. What
    // the child wrote before that is already captured, and the grandchild is
    // left alone: a command that backgrounds a server meant it to outlive the
    // call, and dropping the read end would take the server down with it.
    let readers = async { tokio::join!(stdout_task, stderr_task) };
    let _ = tokio::time::timeout(READER_GRACE, readers).await;
    let (stdout, stderr) = (snapshot(&out_buf), snapshot(&err_buf));

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

/// A stream's captured bytes, shared so a read still in progress can be
/// snapshotted: a grandchild holding the pipe must not cost the output the
/// child already wrote.
type Captured = Arc<Mutex<(Vec<u8>, bool)>>;

fn captured() -> Captured {
    Arc::new(Mutex::new((Vec::new(), false)))
}

/// The bytes read so far, as the caller renders them.
fn snapshot(captured: &Captured) -> String {
    let (buf, truncated) = &*captured.lock().unwrap_or_else(|e| e.into_inner());
    let mut out = String::from_utf8_lossy(buf).into_owned();
    if *truncated {
        out.push_str("\n[output truncated]");
    }
    out
}

/// Read a stream to the end, keeping at most [`MAX_CAPTURE_BYTES`] and
/// draining the rest so the child never blocks on a full pipe.
async fn read_capped<R>(reader: Option<R>, into: Captured)
where
    R: tokio::io::AsyncRead + Unpin,
{
    let Some(mut reader) = reader else {
        return;
    };
    let mut chunk = [0u8; 8192];
    loop {
        match reader.read(&mut chunk).await {
            Ok(0) => break,
            Ok(n) => {
                let (buf, truncated) = &mut *into.lock().unwrap_or_else(|e| e.into_inner());
                let take = n.min(MAX_CAPTURE_BYTES.saturating_sub(buf.len()));
                buf.extend_from_slice(&chunk[..take]);
                *truncated |= take < n;
            }
            Err(_) => break,
        }
    }
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
#[path = "tests/runner_test.rs"]
mod tests;
