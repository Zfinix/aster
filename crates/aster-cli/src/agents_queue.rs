//! Background agent queue: `agent` calls with `background: true` are queued
//! here and drained by a driver task, so the main turn keeps working while
//! sub-agents run. Finished reports are pushed into the live turn's injected
//! queue (or parked until the next turn attaches one), so the model hears
//! about completions without polling.

use std::collections::VecDeque;
use std::pin::Pin;
use std::sync::{Arc, Mutex, OnceLock};

use serde_json::{Value, json};

use crate::agents::{AgentTask, TaskReport};

/// How the driver runs one batch. Production wires this to `run_swarm`;
/// tests stub it.
/// The runner's future is deliberately not `Send`: the agent pipeline is
/// recursive (an agent can dispatch agents) and its futures never were. The
/// driver therefore runs each batch on its own thread instead of spawning a
/// tokio task.
pub(crate) type SwarmRunner = Arc<
    dyn Fn(Vec<AgentTask>, TaskCallbacks) -> Pin<Box<dyn Future<Output = Vec<TaskReport>>>>
        + Send
        + Sync,
>;

type CompleteSink = Arc<dyn Fn(TaskReport) + Send + Sync>;
type ActivitySink = Arc<dyn Fn(&str, &str, String) + Send + Sync>;

/// Per-batch callbacks the driver hands to the runner, so completions and
/// activity surface while the batch is still in flight.
#[derive(Clone)]
pub(crate) struct TaskCallbacks {
    pub on_complete: CompleteSink,
    pub on_activity: ActivitySink,
}

// Caps. Reports reach the model context, so every path here is bounded.
const MAX_PENDING_TASKS: usize = 32;
const MAX_DONE_HISTORY: usize = 20;
const MAX_PARKED: usize = 12;
const MAX_INJECTED_ENTRIES: usize = 24;
const REPORT_CLIP: usize = 2_000;
const TASK_CLIP: usize = 120;

/// One queued batch, sent to the driver thread.
struct BatchJob {
    id: u64,
    tasks: Vec<AgentTask>,
    runner: SwarmRunner,
}

struct RunningBatch {
    id: u64,
    total: usize,
    done: usize,
}

struct DoneTask {
    agent: String,
    task: String,
    ok: bool,
}

/// What the live turn exposes for report delivery. `events` is optional
/// because not every entry point has a sink to offer.
struct Attached {
    injected: Arc<Mutex<Vec<String>>>,
    events: Option<crate::chat::ChatEventSink>,
}

struct Inner {
    /// Task text for `check`, kept separate from the jobs channel.
    pending: VecDeque<(u64, Vec<AgentTask>)>,
    running: Option<RunningBatch>,
    done: VecDeque<DoneTask>,
    next_id: u64,
    attached: Option<Attached>,
    parked: VecDeque<String>,
    dropped: usize,
    driver_started: bool,
}

pub(crate) struct BackgroundAgents {
    inner: Mutex<Inner>,
    jobs: Mutex<Option<std::sync::mpsc::Receiver<BatchJob>>>,
    tx: std::sync::mpsc::Sender<BatchJob>,
}

static QUEUE: OnceLock<Arc<BackgroundAgents>> = OnceLock::new();

impl BackgroundAgents {
    pub(crate) fn new() -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        Self {
            inner: Mutex::new(Inner {
                pending: VecDeque::new(),
                running: None,
                done: VecDeque::new(),
                next_id: 1,
                attached: None,
                parked: VecDeque::new(),
                dropped: 0,
                driver_started: false,
            }),
            jobs: Mutex::new(Some(rx)),
            tx,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().expect("background agents lock")
    }

    /// Point report delivery at the live turn. Parked reports that landed
    /// between turns flush into the fresh injected queue here.
    pub(crate) fn attach(
        &self,
        injected: Arc<Mutex<Vec<String>>>,
        events: Option<crate::chat::ChatEventSink>,
    ) {
        let mut inner = self.lock();
        let mut attached = Attached {
            injected,
            events: inner.attached.as_ref().and_then(|a| a.events.clone()),
        };
        if let Some(events) = events {
            attached.events = Some(events);
        }
        let parked: Vec<String> = inner.parked.drain(..).collect();
        inner.attached = Some(attached);
        for line in parked {
            if !push_capped(&inner.attached.as_ref().expect("just set").injected, &line) {
                inner.dropped += 1;
            }
        }
        if inner.dropped > 0 {
            let dropped = inner.dropped;
            inner.dropped = 0;
            push_capped(
                &inner.attached.as_ref().expect("just set").injected,
                &format!(
                    "({dropped} earlier background report(s) were dropped to keep the context small)"
                ),
            );
        }
    }

    /// Queue a batch for background execution. Returns the batch id.
    pub(crate) fn submit(
        self: &Arc<Self>,
        tasks: Vec<AgentTask>,
        runner: SwarmRunner,
    ) -> Result<u64, String> {
        let id = {
            let mut inner = self.lock();
            let pending: usize = inner.pending.iter().map(|(_, t)| t.len()).sum();
            if pending + tasks.len() > MAX_PENDING_TASKS {
                return Err(format!(
                    "the background queue is full ({pending} task(s) waiting, cap {MAX_PENDING_TASKS}); \
                     check results with the agent tool's `check` option before sending more"
                ));
            }
            let id = inner.next_id;
            inner.next_id += 1;
            inner.pending.push_back((id, tasks.clone()));
            id
        };
        self.ensure_driver();
        if self.tx.send(BatchJob { id, tasks, runner }).is_err() {
            let mut inner = self.lock();
            inner.pending.retain(|(i, _)| *i != id);
            return Err(
                "the background driver is gone; run the task in the foreground instead".into(),
            );
        }
        Ok(id)
    }

    fn ensure_driver(self: &Arc<Self>) {
        let rx = {
            let mut inner = self.lock();
            if inner.driver_started {
                return;
            }
            inner.driver_started = true;
            self.jobs.lock().expect("jobs lock").take()
        };
        let Some(rx) = rx else { return };
        let queue = Arc::clone(self);
        // One worker thread for the whole process: batches run one at a time,
        // each bounded internally by the swarm's max_concurrent. If the thread
        // ever dies, the receiver goes back and a later submit starts a fresh
        // one rather than failing forever.
        std::thread::spawn(move || {
            let ran = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("background driver runtime");
                while let Ok(job) = rx.recv() {
                    {
                        let mut inner = queue.lock();
                        inner.pending.retain(|(id, _)| *id != job.id);
                        inner.running = Some(RunningBatch {
                            id: job.id,
                            total: job.tasks.len(),
                            done: 0,
                        });
                    }
                    let total = job.tasks.len();
                    let callbacks = queue.callbacks(job.id, total);
                    let runner = job.runner;
                    let tasks = job.tasks;
                    let ran = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        rt.block_on(runner(tasks, callbacks))
                    }));
                    if ran.is_err() {
                        queue.deliver(format!(
                            "Background batch {} stopped unexpectedly; its tasks did not finish.",
                            job.id
                        ));
                    }
                    let mut inner = queue.lock();
                    if let Some(running) = &inner.running
                        && running.id == job.id
                    {
                        inner.running = None;
                    }
                }
            }));
            if ran.is_err() {
                tracing::error!("the background driver stopped unexpectedly");
            }
            let mut inner = queue.inner.lock().unwrap_or_else(|e| e.into_inner());
            inner.driver_started = false;
            inner.running = None;
            *queue.jobs.lock().expect("jobs lock") = Some(rx);
        });
    }

    fn callbacks(self: &Arc<Self>, batch_id: u64, total: usize) -> TaskCallbacks {
        let queue = Arc::downgrade(self);
        TaskCallbacks {
            on_complete: Arc::new(move |report: TaskReport| {
                if let Some(queue) = queue.upgrade() {
                    queue.report_done(batch_id, total, &report);
                }
            }),
            on_activity: Arc::new(|_agent: &str, _task: &str, _line: String| {}),
        }
    }

    fn report_done(&self, batch_id: u64, total: usize, report: &TaskReport) {
        let (status, detail) = match &report.error {
            Some(err) => ("failed", clip(err, REPORT_CLIP)),
            None => (
                "finished",
                report
                    .report
                    .as_deref()
                    .map(|r| clip(r, REPORT_CLIP))
                    .unwrap_or_else(|| "no report text".into()),
            ),
        };
        {
            let mut inner = self.lock();
            if let Some(running) = &mut inner.running
                && running.id == batch_id
            {
                running.done += 1;
            }
            inner.done.push_back(DoneTask {
                agent: report.agent.clone(),
                task: report.task.clone(),
                ok: report.error.is_none(),
            });
            if inner.done.len() > MAX_DONE_HISTORY {
                inner.done.pop_front();
            }
        }
        let line = format!(
            "Background agent {agent} {status} (batch {batch_id}, {done}/{total} done): {detail}",
            agent = report.agent,
            done = self.done_count(batch_id),
        );
        self.deliver(line);
        self.emit(json!({
            "type": "agent_status",
            "call_id": format!("bg-{batch_id}"),
            "agent": report.agent,
            "task": clip(&report.task, TASK_CLIP),
            "status": status,
            "done": self.done_count(batch_id),
            "total": total,
        }));
    }

    fn done_count(&self, batch_id: u64) -> usize {
        self.lock()
            .running
            .as_ref()
            .filter(|r| r.id == batch_id)
            .map(|r| r.done)
            .unwrap_or(0)
    }

    fn deliver(&self, line: String) {
        let mut inner = self.lock();
        match &inner.attached {
            Some(attached) => {
                if !push_capped(&attached.injected, &line) {
                    inner.dropped += 1;
                }
            }
            None => {
                if inner.parked.len() >= MAX_PARKED {
                    inner.parked.pop_front();
                    inner.dropped += 1;
                }
                inner.parked.push_back(line);
            }
        }
    }

    fn emit(&self, event: Value) {
        let events = self.lock().attached.as_ref().and_then(|a| a.events.clone());
        if let Some(events) = events {
            events(event);
        }
    }

    /// One-line status for the agent tool's `check` option.
    pub(crate) fn status_text(&self) -> String {
        let inner = self.lock();
        let mut lines = Vec::new();
        if let Some(running) = &inner.running {
            lines.push(format!(
                "batch {} running: {}/{} done",
                running.id, running.done, running.total
            ));
        }
        for (id, tasks) in &inner.pending {
            lines.push(format!(
                "batch {} queued: {} task(s): {}",
                id,
                tasks.len(),
                tasks
                    .iter()
                    .map(|t| format!("{}: {}", t.agent, clip(&t.task, TASK_CLIP)))
                    .collect::<Vec<_>>()
                    .join("; ")
            ));
        }
        if lines.is_empty() && inner.done.is_empty() {
            return "No background agent work has been queued.".to_string();
        }
        for done in inner.done.iter().rev().take(5) {
            lines.push(format!(
                "batch history: {agent} {} on \"{}\"",
                if done.ok { "finished" } else { "failed" },
                clip(&done.task, TASK_CLIP),
                agent = done.agent,
            ));
        }
        lines.join("\n")
    }
}

/// Push a line into an injected queue unless it is already at its cap.
/// Returns false when the line was dropped.
fn push_capped(injected: &Arc<Mutex<Vec<String>>>, line: &str) -> bool {
    let Ok(mut queue) = injected.lock() else {
        return false;
    };
    if queue.len() >= MAX_INJECTED_ENTRIES {
        return false;
    }
    queue.push(line.to_string());
    true
}

fn clip(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut cut = max;
    while !s.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}…", &s[..cut])
}

/// The process-wide queue. Created on first use; lives until exit.
pub(crate) fn queue() -> &'static Arc<BackgroundAgents> {
    QUEUE.get_or_init(|| Arc::new(BackgroundAgents::new()))
}

/// Attach the live turn's report targets. Called at turn start and before a
/// background submit, so completions reach the turn that can hear them.
pub(crate) fn attach(
    injected: Arc<Mutex<Vec<String>>>,
    events: Option<crate::chat::ChatEventSink>,
) {
    queue().attach(injected, events);
}

/// Queue a batch in the process-wide queue.
pub(crate) fn submit(tasks: Vec<AgentTask>, runner: SwarmRunner) -> Result<u64, String> {
    Arc::clone(queue()).submit(tasks, runner)
}

/// Status for the agent tool's `check` option.
pub(crate) fn status_text() -> String {
    queue().status_text()
}

#[cfg(test)]
#[path = "tests/agents_queue_test.rs"]
mod tests;
