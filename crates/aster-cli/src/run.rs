//! `aster run <agent> "<task>"`: one agent, one task, no terminal. The entry
//! point the OS scheduler invokes, and a human can too.

use anyhow::{Context, Result, bail};
use clap::Args;
use std::path::PathBuf;
use std::sync::Arc;

use crate::agents::{AgentDeps, AgentTask, discover_agents, run_swarm};
use aster_policy::Policy;

#[derive(Debug, Args)]
pub(crate) struct RunArgs {
    /// The agent to run, as listed by `aster agents`.
    pub agent: String,
    /// The task handed to the agent.
    pub task: String,
    /// Emit a JSON object instead of prose.
    #[arg(long)]
    pub json: bool,
    /// Tag the recorded session with this schedule name (set by `aster cron`).
    #[arg(long)]
    pub schedule: Option<String>,
    /// Post a native notification when the run finishes.
    #[arg(long)]
    pub notify: bool,
    /// Working directory for the run; defaults to the current directory.
    #[arg(long)]
    pub cwd: Option<PathBuf>,
}

pub(crate) async fn run(args: RunArgs) -> Result<()> {
    let repo_root = match args.cwd {
        Some(ref dir) => {
            std::fs::canonicalize(dir).with_context(|| format!("no directory {}", dir.display()))?
        }
        None => std::env::current_dir().context("could not determine the current directory")?,
    };
    let settings = crate::settings::Settings::load(Some(&repo_root))?;
    let client = crate::config::provider::resolve_client(&settings, None)?;

    let permissions = settings.permissions.clone();
    let policy = Arc::new(Policy::compile(&permissions)?);
    let grants = Arc::new(crate::chat::configured_grants(&permissions, &repo_root));
    let credentials = Arc::new(crate::chat::configured_credentials(
        &permissions,
        &repo_root,
    ));

    let registry = discover_agents(&repo_root);
    if registry.get(&args.agent).is_none() {
        let mut known: Vec<&str> = registry.iter().map(|a| a.name.as_str()).collect();
        known.sort_unstable();
        bail!(
            "unknown agent {:?}. Known agents: {}",
            args.agent,
            if known.is_empty() {
                "(none discovered)".to_string()
            } else {
                known.join(", ")
            }
        );
    }

    let deps = AgentDeps {
        client,
        repo_root: repo_root.clone(),
        policy,
        grants,
        credentials,
        probe: Arc::new(bash_tools::ToolProbe::detect()),
        environment: crate::chat::environment_note(&repo_root),
        limits: crate::chat::Limits::resolve(&settings.agent),
        swarm: crate::chat::SwarmLimits::resolve(&settings.agents),
        session_registry: registry.clone(),
    };

    let reports = run_swarm(
        vec![AgentTask {
            agent: args.agent.clone(),
            task: args.task.clone(),
        }],
        &registry,
        &deps,
        |_, _, _| {},
        |_| {},
    )
    .await;
    let report = &reports[0];

    if let Some(schedule) = &args.schedule {
        record_scheduled_session(&deps, &args, schedule, report)?;
    }
    if args.notify {
        let body = match (&report.report, &report.error) {
            (Some(text), _) => first_line(text),
            (None, Some(err)) => format!("failed: {err}"),
            (None, None) => "finished with no report".to_string(),
        };
        let title = format!("aster: {}", args.schedule.as_deref().unwrap_or(&args.agent));
        let _ = aster_cron::notify::send(&title, &body);
    }

    if args.json {
        println!("{}", serde_json::to_string(report)?);
    } else {
        match (&report.report, &report.error) {
            (Some(text), _) => println!("{text}"),
            (None, Some(err)) => bail!("{err}"),
            (None, None) => println!("finished with no report"),
        }
    }
    Ok(())
}

fn first_line(text: &str) -> String {
    let line = text
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("finished");
    let mut cut = 120.min(line.len());
    while !line.is_char_boundary(cut) {
        cut -= 1;
    }
    line[..cut].to_string()
}

fn record_scheduled_session(
    deps: &AgentDeps,
    args: &RunArgs,
    schedule: &str,
    report: &crate::agents::TaskReport,
) -> Result<()> {
    let Some(store) = crate::persist::store().ok() else {
        return Ok(());
    };
    let mut writer = store.new_session_with_schedule(
        &deps.repo_root,
        &deps.repo_root,
        Some(deps.client.model.clone()),
        Some(schedule),
    )?;
    writer.append_message(aster_persist::MessageEvent::user(&args.task))?;
    let reply = report
        .report
        .clone()
        .or_else(|| report.error.clone())
        .unwrap_or_else(|| "finished with no report".to_string());
    writer.append_message(aster_persist::MessageEvent::assistant(
        Some(reply),
        Vec::new(),
    ))?;
    Ok(())
}
