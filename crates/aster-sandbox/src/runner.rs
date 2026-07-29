//! Command execution inside the sandbox.

use std::time::Duration;

use anyhow::{Context, Result, bail};
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

/// Run a command inside the sandbox. The command is specified as a binary
/// path and a list of arguments. The working directory is set to the repo
/// root.
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
    for key in &["PATH", "HOME", "USER", "SHELL", "LANG", "LC_ALL", "TERM"] {
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

    let output = tokio::time::timeout(timeout, cmd.output())
        .await
        .map_err(|_| anyhow::anyhow!("command timed out after {}s", config.profile.timeout_secs))?
        .context("running sandboxed command")?;

    Ok(CommandOutput {
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        exit_code: output.status.code(),
        timed_out: false,
    })
}

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

#[cfg(test)]
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
        let result = run_command(&cfg, "sleep", &["10".into()]).await;
        assert!(result.is_err(), "should have timed out");
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
