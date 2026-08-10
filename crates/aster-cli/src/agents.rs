//! Sub-agent fan-out: the `agent` tool dispatches cheap read-only collectors
//! in parallel, then the model hands their raw reports to the expensive
//! synthesizer.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use aster_agents::AgentRegistry;
use aster_ai::AiClient;
use aster_policy::{Grants, Policy};
use futures_util::StreamExt;

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

/// One task in a batch `agent` tool call.
#[derive(Debug, Clone)]
pub(crate) struct AgentTask {
    pub agent: String,
    pub task: String,
}

/// The result of one agent task, ready for serialization into the tool result.
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

/// Dependencies a sub-agent run needs from the parent session.
#[derive(Clone)]
pub(crate) struct AgentDeps {
    pub client: AiClient,
    pub repo_root: PathBuf,
    pub policy: Arc<Policy>,
    pub grants: Arc<Grants>,
    /// The parent's credential approvals: a sub-agent is the same user in the
    /// same session, so it inherits them rather than re-asking.
    pub credentials: Arc<aster_policy::CommandGrants>,
    pub probe: Arc<bash_tools::ToolProbe>,
    pub environment: Option<String>,
    pub limits: crate::chat::Limits,
    pub swarm: SwarmLimits,
    pub session_registry: Arc<AgentRegistry>,
}

/// Run a single sub-agent, returning its final text answer.
async fn run_agent(
    def: &aster_agents::AgentDef,
    task: &str,
    deps: &AgentDeps,
) -> anyhow::Result<String> {
    let model = def
        .model
        .clone()
        .or_else(|| deps.swarm.collector_model.clone())
        .unwrap_or_else(|| deps.client.model.clone());
    let mut child_client = deps.client.clone();
    child_client.model = model;

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
        lookups: Default::default(),
        injected: Default::default(),
        agents: deps.session_registry.clone(),
        sub_agent: Some(overrides),
        swarm: deps.swarm.clone(),
    };

    let history = vec![aster_ai::ChatMessage {
        role: "user".into(),
        content: task.to_string(),
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
        Box::new(|_| {}),
    )
    .await?;

    Ok(reply)
}

/// Fan out a batch of tasks concurrently, bounded by `max_concurrent`.
/// Preserves input order regardless of completion order. Calls `on_complete`
/// once per finished task so the UI can render live progress; the parent emits
/// the `running` events for the whole batch up front.
pub(crate) async fn run_swarm<F>(
    tasks: Vec<AgentTask>,
    registry: &AgentRegistry,
    deps: &AgentDeps,
    on_complete: F,
) -> Vec<TaskReport>
where
    F: Fn(AgentProgress) + Send + Sync,
{
    let concurrency = deps.swarm.max_concurrent.max(1);
    let timeout = std::time::Duration::from_secs(deps.swarm.agent_timeout_secs);
    let total = tasks.len();
    let done = Arc::new(AtomicUsize::new(0));
    let on_complete = Arc::new(on_complete);

    let fut = futures_util::stream::iter(tasks.into_iter().enumerate())
        .map(|(i, task)| {
            let deps = deps.clone();
            let registry = registry.clone();
            let done = done.clone();
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

                let result =
                    tokio::time::timeout(timeout, run_agent(&def, &task.task, &deps)).await;

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
                        let err = format!("timed out after {}s", timeout.as_secs());
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
