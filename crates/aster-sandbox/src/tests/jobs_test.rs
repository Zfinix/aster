use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

use super::*;
use crate::SandboxProfile;

fn start(timeout_secs: u64, script: &str) -> (JobHandle, mpsc::Receiver<JobState>) {
    let config = SandboxConfig::new(SandboxProfile::new(Path::new(".")).timeout(timeout_secs));
    let (tx, rx) = mpsc::channel();
    let handle = spawn(
        &config,
        Isolation::Unsandboxed,
        "sh",
        &["-c".into(), script.into()],
        move |job| {
            let _ = tx.send(job.state());
        },
    )
    .expect("spawn");
    (handle, rx)
}

#[test]
fn a_job_reports_its_exit_code_and_output() {
    let (handle, exited) = start(30, "echo hello; echo oops >&2; exit 3");
    assert_eq!(
        exited.recv_timeout(Duration::from_secs(10)),
        Ok(JobState::Exited(Some(3)))
    );
    let tail = handle.tail(1024);
    assert!(tail.contains("hello") && tail.contains("oops"), "{tail}");
}

#[test]
fn spawn_returns_while_the_job_is_still_running() {
    let (handle, exited) = start(30, "sleep 30");
    assert_eq!(handle.state(), JobState::Running);
    handle.stop();
    assert_eq!(
        exited.recv_timeout(Duration::from_secs(10)),
        Ok(JobState::Stopped)
    );
}

#[test]
fn the_deadline_kills_a_job() {
    let (_handle, exited) = start(1, "sleep 30");
    assert_eq!(
        exited.recv_timeout(Duration::from_secs(10)),
        Ok(JobState::TimedOut)
    );
}

#[test]
fn the_tail_keeps_the_latest_output() {
    let (handle, exited) = start(30, "for i in $(seq 1 50000); do echo line$i; done");
    exited.recv_timeout(Duration::from_secs(20)).expect("exit");
    let tail = handle.tail(64);
    assert!(tail.ends_with("line50000\n"), "{tail}");
}

#[test]
#[ignore = "slow: spawns real sandboxed processes"]
fn a_sandboxed_job_runs_and_drops_secrets() {
    let config = SandboxConfig::new(SandboxProfile::new(Path::new(".")).timeout(30))
        .env("GITHUB_TOKEN", "secret");
    let (tx, rx) = mpsc::channel();
    let handle = spawn(
        &config,
        Isolation::Sandboxed,
        "sh",
        &["-c".into(), "echo token=${GITHUB_TOKEN:-none}".into()],
        move |job| {
            let _ = tx.send(job.state());
        },
    )
    .expect("spawn");
    assert_eq!(
        rx.recv_timeout(Duration::from_secs(10)),
        Ok(JobState::Exited(Some(0)))
    );
    assert_eq!(handle.tail(1024), "token=none\n");
}
