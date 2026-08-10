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
