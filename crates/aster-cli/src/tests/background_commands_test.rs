use std::path::Path;

use aster_sandbox::{Isolation, SandboxConfig, SandboxProfile};
use serde_json::json;

use super::*;

fn config() -> SandboxConfig {
    SandboxConfig::new(SandboxProfile::new(Path::new(".")).timeout(60))
}

fn job_id(started: &str) -> u64 {
    started
        .strip_prefix("Started background job ")
        .and_then(|rest| rest.split('.').next())
        .and_then(|id| id.parse().ok())
        .unwrap_or_else(|| panic!("no job id in {started:?}"))
}

#[cfg(unix)]
#[tokio::test]
async fn wait_returns_the_finished_job_and_its_output() {
    let started = start(
        &config(),
        Isolation::Unsandboxed,
        "sh",
        &["-c".into(), "sleep 1; echo eval done".into()],
    )
    .expect("start");
    let id = job_id(&started);

    let waited = tool(&json!({"action": "wait", "job": id, "seconds": 30}))
        .await
        .expect("wait");
    assert_eq!(
        waited,
        format!(
            "job {id} (`sh -c sleep 1; echo eval done`) exited with code 0 after 1s.\nLast output:\neval done\n"
        )
    );
}

#[cfg(unix)]
#[tokio::test]
async fn stop_kills_a_running_job() {
    let started = start(&config(), Isolation::Unsandboxed, "sleep", &["30".into()]).expect("start");
    let id = job_id(&started);

    let stopped = tool(&json!({"action": "stop", "job": id}))
        .await
        .expect("stop");
    assert!(
        stopped.starts_with(&format!("job {id} (`sleep 30`) was stopped after")),
        "{stopped}"
    );
}

#[tokio::test]
async fn an_unknown_job_says_how_to_find_the_real_ones() {
    let err = tool(&json!({"action": "output", "job": 999_999}))
        .await
        .expect_err("unknown job");
    assert_eq!(
        err.to_string(),
        "there is no background job 999999; `command_job` with action `list` shows them"
    );
}
