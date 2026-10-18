//! Commands that run without blocking the caller. A job keeps only the latest
//! tail of its output, and a hard deadline kills its process group.

use std::collections::VecDeque;
use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

use crate::runner::{kill_process_group, prepare_command};
use crate::{SandboxBackend, SandboxConfig, detect_backend};

const MAX_TAIL_BYTES: usize = 256 * 1024;
const POLL: Duration = Duration::from_millis(200);
const READER_GRACE: Duration = Duration::from_secs(5);

/// Whether a job runs inside the OS sandbox or with the full environment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Isolation {
    Sandboxed,
    Unsandboxed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobState {
    Running,
    Exited(Option<i32>),
    TimedOut,
    Stopped,
}

struct Shared {
    started: Instant,
    ended: Mutex<Option<Instant>>,
    state: Mutex<JobState>,
    output: Mutex<VecDeque<u8>>,
    stop: AtomicBool,
}

/// A running or finished background job. Cloning shares the same job.
#[derive(Clone)]
pub struct JobHandle {
    shared: Arc<Shared>,
}

impl JobHandle {
    pub fn state(&self) -> JobState {
        *self.shared.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn elapsed(&self) -> Duration {
        let ended = *self.shared.ended.lock().unwrap_or_else(|e| e.into_inner());
        ended.unwrap_or_else(Instant::now) - self.shared.started
    }

    /// The last `max_bytes` of stdout and stderr, interleaved as they arrived.
    pub fn tail(&self, max_bytes: usize) -> String {
        let output = self.shared.output.lock().unwrap_or_else(|e| e.into_inner());
        let skip = output.len().saturating_sub(max_bytes);
        let bytes: Vec<u8> = output.iter().skip(skip).copied().collect();
        String::from_utf8_lossy(&bytes).into_owned()
    }

    /// Ask the job to stop. Its process group is killed within a poll interval.
    pub fn stop(&self) {
        self.shared.stop.store(true, Ordering::Relaxed);
    }
}

/// Start `binary` in the background under `config`, whose profile timeout is
/// the job's deadline. `on_exit` runs once, after the final output is in.
pub fn spawn(
    config: &SandboxConfig,
    isolation: Isolation,
    binary: &str,
    args: &[String],
    on_exit: impl FnOnce(&JobHandle) + Send + 'static,
) -> Result<JobHandle> {
    let backend = match isolation {
        Isolation::Sandboxed => detect_backend(),
        Isolation::Unsandboxed => SandboxBackend::ProcessLevel,
    };
    let mut child = prepare_command(backend, config, binary, args)?
        .into_std()
        .spawn()
        .context("spawning background command")?;
    let handle = JobHandle {
        shared: Arc::new(Shared {
            started: Instant::now(),
            ended: Mutex::new(None),
            state: Mutex::new(JobState::Running),
            output: Mutex::new(VecDeque::new()),
            stop: AtomicBool::new(false),
        }),
    };
    let readers = [
        child
            .stdout
            .take()
            .map(|r| pump(r, Arc::clone(&handle.shared))),
        child
            .stderr
            .take()
            .map(|r| pump(r, Arc::clone(&handle.shared))),
    ];
    let deadline = Duration::from_secs(config.profile.timeout_secs);
    let job = handle.clone();
    std::thread::spawn(move || {
        let pid = Some(child.id());
        let state = loop {
            match child.try_wait() {
                Ok(Some(status)) => break JobState::Exited(status.code()),
                Ok(None) => {}
                Err(_) => break JobState::Exited(None),
            }
            let reason = match (
                job.shared.stop.load(Ordering::Relaxed),
                job.shared.started.elapsed() >= deadline,
            ) {
                (true, _) => Some(JobState::Stopped),
                (false, true) => Some(JobState::TimedOut),
                (false, false) => None,
            };
            if let Some(reason) = reason {
                kill_process_group(pid);
                let _ = child.kill();
                let _ = child.wait();
                break reason;
            }
            std::thread::sleep(POLL);
        };
        // A grandchild left running can hold the pipes open forever.
        let waited = Instant::now();
        while readers.iter().flatten().any(|r| !r.is_finished()) && waited.elapsed() < READER_GRACE
        {
            std::thread::sleep(POLL);
        }
        *job.shared.ended.lock().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
        *job.shared.state.lock().unwrap_or_else(|e| e.into_inner()) = state;
        on_exit(&job);
    });
    Ok(handle)
}

fn pump(mut reader: impl Read + Send + 'static, shared: Arc<Shared>) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let mut chunk = [0u8; 8192];
        while let Ok(n) = reader.read(&mut chunk) {
            if n == 0 {
                break;
            }
            let mut output = shared.output.lock().unwrap_or_else(|e| e.into_inner());
            output.extend(&chunk[..n]);
            let excess = output.len().saturating_sub(MAX_TAIL_BYTES);
            output.drain(..excess);
        }
    })
}

#[cfg(all(test, unix))]
#[path = "tests/jobs_test.rs"]
mod tests;
