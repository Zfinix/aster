//! Sub-agent fan-out: the `agent` tool dispatches cheap read-only collectors
//! in parallel, then the model hands their raw reports to the expensive
//! synthesis agent.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use aster_agents::AgentRegistry;
use aster_ai::AiClient;
use aster_policy::{Grants, Policy};
use futures_util::StreamExt;
use serde_json::Value;

use crate::chat::{SessionCtx, SubAgentOverrides, SwarmLimits};

/// Discover agents from the project and global roots, then return the
/// registry.  Called once at startup, shared via `Arc`.
pub(crate) fn discover_agents(repo_root: &std::path::Path) -> Arc<AgentRegistry> {
    let mut roots = vec![repo_root.join(".aster").join("agents")];
    match crate::persist::home() {
        Ok(home) => roots.push(home.join("agents")),
        Err(e) => tracing::debug!("no global agents root: {e:#}"),
    }
    Arc::new(AgentRegistry::discover(&roots))
}

#[derive(Debug, Clone)]
pub(crate) struct AgentTask {
    pub agent: String,
    pub task: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct TaskReport {
    pub agent: String,
    pub task: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub report: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// How a swarm task ended, reported to the UI between the `running` events the
/// parent emits up front and the single `tool_result` the model eventually sees.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProgressStatus {
    Done,
    Failed,
}

/// One swarm task completion, surfaced as an `agent_status` stream event.
#[derive(Debug, Clone)]
pub(crate) struct AgentProgress {
    pub agent: String,
    pub task: String,
    pub status: ProgressStatus,
    pub report: Option<String>,
    pub error: Option<String>,
    pub done: usize,
    pub total: usize,
}

#[derive(Clone)]
pub(crate) struct AgentDeps {
    pub client: AiClient,
    pub repo_root: PathBuf,
    pub policy: Arc<Policy>,
    pub grants: Arc<Grants>,
    pub credentials: Arc<aster_policy::CommandGrants>,
    pub probe: Arc<bash_tools::ToolProbe>,
    pub environment: Option<String>,
    pub limits: crate::chat::Limits,
    pub swarm: SwarmLimits,
    pub session_registry: Arc<AgentRegistry>,
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

fn condense(text: &str) -> Option<String> {
    let joined = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if joined.is_empty() {
        None
    } else {
        Some(clip(&joined, 120))
    }
}

fn tool_line(ev: &Value) -> Option<String> {
    let name = ev.get("name").and_then(Value::as_str)?;
    let args: Value = ev
        .get("arguments")
        .and_then(Value::as_str)
        .and_then(|s| crate::chat::parse_arguments(s).ok())
        .unwrap_or(Value::Null);
    let detail = if name == "run_command" {
        command_line(&args)
    } else {
        ["path", "dir", "query", "pattern", "file", "url", "name"]
            .iter()
            .find_map(|k| args.get(k).and_then(Value::as_str))
            .map(str::to_string)
    };
    Some(match detail {
        Some(d) => clip(&format!("{name} {d}"), 120),
        None => name.to_string(),
    })
}

fn command_line(args: &Value) -> Option<String> {
    let command = args.get("command").and_then(Value::as_str)?;
    let rest = args
        .get("args")
        .and_then(Value::as_array)
        .map(|v| v.iter().filter_map(Value::as_str).collect::<Vec<_>>())
        .unwrap_or_default();
    Some(
        std::iter::once(command)
            .chain(rest)
            .collect::<Vec<_>>()
            .join(" "),
    )
}

fn activity_sink(tx: tokio::sync::mpsc::UnboundedSender<String>) -> crate::chat::ChatEventSink {
    let narration = std::sync::Mutex::new(String::new());
    Box::new(move |ev| {
        match ev.get("type").and_then(Value::as_str).unwrap_or("") {
            "token" | "text" => {
                if let Some(content) = ev.get("content").and_then(Value::as_str) {
                    narration.lock().unwrap().push_str(content);
                }
            }
            "tool_call" => {
                let buffered = std::mem::take(&mut *narration.lock().unwrap());
                if let Some(line) = condense(&buffered) {
                    let _ = tx.send(line);
                }
                if let Some(line) = tool_line(&ev) {
                    let _ = tx.send(line);
                }
            }
            _ => {}
        };
    })
}

const WRAP_UP: &str = "Your time limit is nearly up. Stop calling tools and \
write your final report now from what you have gathered, noting what you did \
not get to.";

const SALVAGE_LINES: usize = 40;

fn push_salvage(log: &std::sync::Mutex<Vec<String>>, line: &str) {
    if let Ok(mut log) = log.lock() {
        if log.len() == SALVAGE_LINES {
            log.remove(0);
        }
        log.push(line.to_string());
    }
}

fn salvage_report(timeout_secs: u64, trail: &str) -> Option<String> {
    if trail.is_empty() {
        return None;
    }
    Some(format!(
        "No final report: the task hit its {timeout_secs}s time limit. \
         What it did before the cutoff:\n{trail}"
    ))
}

fn wrap_up_grace(timeout: std::time::Duration) -> std::time::Duration {
    (timeout / 5).clamp(
        std::time::Duration::from_secs(15),
        std::time::Duration::from_secs(60),
    )
}

async fn run_agent(
    def: &aster_agents::AgentDef,
    task: &str,
    deps: &AgentDeps,
    activity: tokio::sync::mpsc::UnboundedSender<String>,
    injected: Arc<std::sync::Mutex<Vec<String>>>,
) -> anyhow::Result<String> {
    let mut child_client = deps.client.clone();
    // An overridden model may belong to another provider than the parent's
    // endpoint, so it is re-paired with an endpoint that serves it.
    if let Some(model) = def
        .model
        .clone()
        .or_else(|| deps.swarm.collector_model.clone())
    {
        match crate::mom::target_for_model(&model, deps.client.base_url()) {
            Some(target) => {
                child_client.set_endpoint(&target.base_url, target.key);
                child_client.model = target.model_param;
            }
            None => child_client.model = model,
        }
    }

    let tool_allowlist: HashSet<String> = def
        .tools
        .as_deref()
        .map(|v| v.iter().map(String::from).collect::<Vec<_>>())
        .unwrap_or_else(|| {
            aster_agents::DEFAULT_TOOLS
                .iter()
                .map(|s| (*s).to_string())
                .collect()
        })
        .into_iter()
        .collect();

    let body = def.load_body()?;
    let max_rounds = def.max_rounds.unwrap_or(8);

    let overrides = Arc::new(SubAgentOverrides {
        prompt_body: body,
        tool_allowlist,
    });

    let child_ctx = SessionCtx {
        recorder: None,
        store: None,
        credentials: deps.credentials.clone(),
        // Not inherited: a sub-agent asks for its own out-of-repo writes.
        write_grants: Default::default(),
        skills: Arc::new(aster_skills::SkillSet::default()),
        instructions: Arc::new(crate::instructions::Instructions::default()),
        probe: deps.probe.clone(),
        plan: Default::default(),
        mcp: None,
        limits: crate::chat::Limits {
            max_tool_rounds: max_rounds,
            command_timeout_secs: deps.limits.command_timeout_secs,
            compact_budget_chars: deps.limits.compact_budget_chars,
        },
        environment: deps.environment.clone(),
        yolo: false,
        reads: Default::default(),
        previews: Default::default(),
        lookups: Default::default(),
        injected,
        agents: deps.session_registry.clone(),
        sub_agent: Some(overrides),
        swarm: deps.swarm.clone(),
    };

    let history = vec![aster_ai::ChatMessage {
        role: "user".into(),
        content: task.into(),
    }];

    let allow_edits = def
        .tools
        .as_deref()
        .map(|t| t.contains(&"edit_file".to_string()))
        .unwrap_or(false);

    let (reply, _edited, _compacted) = crate::chat::agent_turn_streaming(
        child_client,
        deps.repo_root.clone(),
        history,
        allow_edits,
        deps.policy.clone(),
        deps.grants.clone(),
        None,
        child_ctx,
        activity_sink(activity),
    )
    .await?;

    Ok(reply)
}

/// Fan out a batch of tasks concurrently, bounded by `max_concurrent`, in input
/// order regardless of completion order. `on_activity` fires per live action and
/// `on_complete` once per finished task, so the UI can render live progress.
pub(crate) async fn run_swarm<A, F>(
    tasks: Vec<AgentTask>,
    registry: &AgentRegistry,
    deps: &AgentDeps,
    on_activity: A,
    on_complete: F,
) -> Vec<TaskReport>
where
    A: Fn(&str, &str, String) + Send + Sync,
    F: Fn(AgentProgress) + Send + Sync,
{
    let concurrency = deps.swarm.max_concurrent.max(1);
    let timeout = std::time::Duration::from_secs(deps.swarm.agent_timeout_secs);
    let total = tasks.len();
    let done = Arc::new(AtomicUsize::new(0));
    let on_activity = Arc::new(on_activity);
    let on_complete = Arc::new(on_complete);

    let fut = futures_util::stream::iter(tasks.into_iter().enumerate())
        .map(|(i, task)| {
            let deps = deps.clone();
            let registry = registry.clone();
            let done = done.clone();
            let on_activity = on_activity.clone();
            let on_complete = on_complete.clone();
            async move {
                let n = done.fetch_add(1, Ordering::Relaxed) + 1;
                let progress =
                    |status: ProgressStatus, report: Option<String>, error: Option<String>| {
                        on_complete(AgentProgress {
                            agent: task.agent.clone(),
                            task: task.task.clone(),
                            status,
                            report,
                            error,
                            done: n,
                            total,
                        });
                    };

                let def = match registry.get(&task.agent) {
                    Some(d) => d.clone(),
                    None => {
                        progress(
                            ProgressStatus::Failed,
                            None,
                            Some(format!("unknown agent: {}", task.agent)),
                        );
                        return (
                            i,
                            TaskReport {
                                agent: task.agent.clone(),
                                task: task.task.clone(),
                                report: None,
                                error: Some(format!("unknown agent: {}", task.agent)),
                            },
                        );
                    }
                };

                // Forward activity lines while the run is in flight; the
                // channel keeps the child's 'static sink free of borrows.
                let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
                let injected: Arc<std::sync::Mutex<Vec<String>>> = Arc::default();
                let salvage = std::sync::Mutex::new(Vec::new());
                // At the deadline the task is told to wrap up, and the hard
                // kill waits one grace period more so the report can land.
                let grace = wrap_up_grace(timeout);
                let result = tokio::time::timeout(timeout + grace, async {
                    let run = run_agent(&def, &task.task, &deps, tx, injected.clone());
                    tokio::pin!(run);
                    let nudge = tokio::time::sleep(timeout);
                    tokio::pin!(nudge);
                    let mut nudged = false;
                    loop {
                        tokio::select! {
                            r = &mut run => {
                                while let Ok(line) = rx.try_recv() {
                                    push_salvage(&salvage, &line);
                                    on_activity(&task.agent, &task.task, line);
                                }
                                break r;
                            }
                            Some(line) = rx.recv() => {
                                push_salvage(&salvage, &line);
                                on_activity(&task.agent, &task.task, line);
                            }
                            _ = &mut nudge, if !nudged => {
                                nudged = true;
                                if let Ok(mut queue) = injected.lock() {
                                    queue.push(WRAP_UP.to_string());
                                }
                            }
                        }
                    }
                })
                .await;

                match result {
                    Ok(Ok(report)) => {
                        progress(ProgressStatus::Done, Some(report.clone()), None);
                        (
                            i,
                            TaskReport {
                                agent: task.agent,
                                task: task.task,
                                report: Some(report),
                                error: None,
                            },
                        )
                    }
                    Ok(Err(e)) => {
                        let err = format!("{e:#}");
                        progress(ProgressStatus::Failed, None, Some(err.clone()));
                        (
                            i,
                            TaskReport {
                                agent: task.agent,
                                task: task.task,
                                report: None,
                                error: Some(err),
                            },
                        )
                    }
                    Err(_elapsed) => {
                        let total = (timeout + grace).as_secs();
                        let err = format!("timed out after {total}s, wrap-up extension included");
                        let trail = salvage.lock().map(|log| log.join("\n")).unwrap_or_default();
                        progress(ProgressStatus::Failed, None, Some(err.clone()));
                        (
                            i,
                            TaskReport {
                                agent: task.agent,
                                task: task.task,
                                report: salvage_report(total, &trail),
                                error: Some(err),
                            },
                        )
                    }
                }
            }
        })
        .buffer_unordered(concurrency)
        .collect::<Vec<_>>()
        .await;

    let mut ordered: Vec<(usize, TaskReport)> = fut;
    ordered.sort_by_key(|(i, _)| *i);
    ordered.into_iter().map(|(_, r)| r).collect()
}

#[cfg(test)]
#[path = "tests/agents_test.rs"]
mod tests;
