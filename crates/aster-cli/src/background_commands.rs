//! `run_command` with `background: true`, and the `command_job` tool that
//! lists, reads, waits on, and stops those jobs. Exit notices reach the model
//! through the background agent queue, the same way finished sub-agents do.

use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use aster_sandbox::{Isolation, JobHandle, JobState, SandboxConfig};
use serde_json::{Value, json};

use crate::util::{elapsed, truncate};

pub(crate) const JOB_TIMEOUT_SECS: u64 = 4 * 60 * 60;
const MAX_RUNNING: usize = 8;
const MAX_FINISHED_KEPT: usize = 16;
const MAX_OUTPUT_BYTES: usize = 10_000;
const NOTICE_TAIL_BYTES: usize = 1_500;
const DEFAULT_WAIT_SECS: u64 = 300;
const MAX_WAIT_SECS: u64 = 600;
const LABEL_CHARS: usize = 100;

struct Job {
    id: u64,
    label: String,
    handle: JobHandle,
}

#[derive(Default)]
struct Jobs {
    next_id: u64,
    list: Vec<Job>,
}

fn jobs() -> std::sync::MutexGuard<'static, Jobs> {
    static JOBS: OnceLock<Mutex<Jobs>> = OnceLock::new();
    JOBS.get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// Start an already authorized command as a background job.
pub(crate) fn start(
    config: &SandboxConfig,
    isolation: Isolation,
    binary: &str,
    args: &[String],
) -> Result<String> {
    let mut jobs = jobs();
    let running = jobs
        .list
        .iter()
        .filter(|j| j.handle.state() == JobState::Running)
        .count();
    if running >= MAX_RUNNING {
        bail!(
            "{running} commands are already running in the background; wait for one or stop it \
             with `command_job` before starting another"
        );
    }
    jobs.next_id += 1;
    let id = jobs.next_id;
    let label = truncate(
        &std::iter::once(binary)
            .chain(args.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join(" "),
        LABEL_CHARS,
    );
    let notice_label = label.clone();
    let handle = aster_sandbox::spawn(config, isolation, binary, args, move |job| {
        crate::agents_queue::notify(format!(
            "Background command job {id} (`{notice_label}`) {}.\nLast output:\n{}",
            describe(job),
            tail(job, NOTICE_TAIL_BYTES)
        ));
    })?;
    jobs.list.push(Job { id, label, handle });
    let finished = jobs
        .list
        .iter()
        .filter(|j| j.handle.state() != JobState::Running)
        .count();
    let mut excess = finished.saturating_sub(MAX_FINISHED_KEPT);
    jobs.list.retain(|j| {
        let drop = excess > 0 && j.handle.state() != JobState::Running;
        excess -= usize::from(drop);
        !drop
    });
    Ok(format!(
        "Started background job {id}. You will get a message when it exits. Use `command_job` \
         to read its output, wait on it, or stop it. It is stopped after {}.",
        elapsed(JOB_TIMEOUT_SECS)
    ))
}

fn tail(job: &JobHandle, max_bytes: usize) -> String {
    match job.tail(max_bytes) {
        tail if tail.trim().is_empty() => "(no output yet)".into(),
        tail => tail,
    }
}

fn describe(job: &JobHandle) -> String {
    let took = elapsed(job.elapsed().as_secs());
    match job.state() {
        JobState::Running => format!("has been running for {took}"),
        JobState::Exited(Some(code)) => format!("exited with code {code} after {took}"),
        JobState::Exited(None) => format!("was killed by a signal after {took}"),
        JobState::TimedOut => format!("was stopped at its {took} time limit"),
        JobState::Stopped => format!("was stopped after {took}"),
    }
}

fn find(id: u64) -> Result<(String, JobHandle)> {
    match jobs().list.iter().find(|j| j.id == id) {
        Some(job) => Ok((job.label.clone(), job.handle.clone())),
        None => {
            bail!("there is no background job {id}; `command_job` with action `list` shows them")
        }
    }
}

fn report(id: u64, label: &str, job: &JobHandle) -> String {
    format!(
        "job {id} (`{label}`) {}.\nLast output:\n{}",
        describe(job),
        tail(job, MAX_OUTPUT_BYTES)
    )
}

/// The `command_job` tool.
pub(crate) async fn tool(args: &Value) -> Result<String> {
    let job_id = || {
        args["job"]
            .as_u64()
            .ok_or_else(|| anyhow::anyhow!("command_job needs a `job` id for this action"))
    };
    match args["action"].as_str().unwrap_or("list") {
        "list" => {
            let jobs = jobs();
            if jobs.list.is_empty() {
                return Ok("No commands have been started in the background.".into());
            }
            Ok(jobs
                .list
                .iter()
                .map(|j| format!("job {} (`{}`) {}", j.id, j.label, describe(&j.handle)))
                .collect::<Vec<_>>()
                .join("\n"))
        }
        "output" => {
            let id = job_id()?;
            let (label, job) = find(id)?;
            Ok(report(id, &label, &job))
        }
        "wait" => {
            let id = job_id()?;
            let (label, job) = find(id)?;
            let secs = args["seconds"]
                .as_u64()
                .unwrap_or(DEFAULT_WAIT_SECS)
                .clamp(1, MAX_WAIT_SECS);
            let until = Instant::now() + Duration::from_secs(secs);
            while job.state() == JobState::Running && Instant::now() < until {
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
            Ok(report(id, &label, &job))
        }
        "stop" => {
            let id = job_id()?;
            let (label, job) = find(id)?;
            job.stop();
            while job.state() == JobState::Running {
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
            Ok(report(id, &label, &job))
        }
        other => bail!("unknown command_job action `{other}`; use list, output, wait, or stop"),
    }
}

pub(crate) fn schema() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "command_job",
            "description": "Check on commands started with run_command background:true. `list` shows every job, `output` shows a job's state and latest output, `wait` blocks until the job exits or `seconds` pass (default 300, max 600) and then shows the same, `stop` kills it. Prefer `wait` over calling `output` in a loop.",
            "parameters": {
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["list", "output", "wait", "stop"] },
                    "job": { "type": "integer", "description": "The job id run_command returned" },
                    "seconds": { "type": "integer", "minimum": 1, "maximum": MAX_WAIT_SECS, "description": "For `wait`: how long to wait at most" }
                },
                "required": ["action"]
            }
        }
    })
}

#[cfg(test)]
#[path = "tests/background_commands_test.rs"]
mod tests;
