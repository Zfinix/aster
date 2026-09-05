//! `aster status`: what this repo's next turn would run with. The TUI's
//! `/status` minus the live session, so front-ends that own their own history
//! can fill in the rest themselves.

use std::env;
use std::path::Path;

use anyhow::{Context, Result};
use serde_json::json;

use crate::settings::Settings;

pub fn run() -> Result<()> {
    let repo_root = env::current_dir().context("could not determine the current directory")?;
    let settings = Settings::load(Some(&repo_root))?;
    let client = crate::config::provider::resolve_client(&settings, None)?;
    let limits = crate::chat::Limits::resolve(&settings.agent);

    let servers: Vec<_> = settings
        .mcp
        .servers
        .iter()
        .map(|(name, config)| json!({ "name": name, "disabled": config.disabled }))
        .collect();
    let enabled = servers.iter().filter(|s| s["disabled"] == false).count();

    let store = crate::persist::store().ok();
    let memory = store
        .as_ref()
        .and_then(|s| s.memory().list().ok())
        .map(|blocks| blocks.len());
    let sessions = store
        .as_ref()
        .and_then(|s| s.list_sessions(&repo_root).ok())
        .map(|metas| metas.len());

    if crate::json_mode() {
        println!(
            "{}",
            json!({
                "ok": true,
                "repo_root": repo_root.display().to_string(),
                "model": client.model,
                "provider": crate::init::provider_label(client.base_url()),
                "base_url": client.base_url(),
                "effort": client.effort().as_str(),
                "mode": settings.permissions.mode.as_str(),
                "limits": {
                    "max_tool_rounds": limits.max_tool_rounds,
                    "command_timeout_secs": limits.command_timeout_secs,
                    "compact_budget_chars": limits.compact_budget_chars,
                },
                "mcp": { "servers": servers, "enabled": enabled },
                "memory_blocks": memory,
                "sessions": sessions,
                "skills": count(&repo_root),
            })
        );
        return Ok(());
    }

    let rows = [
        ("model", client.model.clone()),
        ("provider", crate::init::provider_label(client.base_url())),
        ("effort", client.effort().as_str().to_string()),
        ("mode", settings.permissions.mode.as_str().to_string()),
        (
            "context",
            format!("{} chars before auto-compact", limits.compact_budget_chars),
        ),
        (
            "mcp",
            match servers.len() {
                0 => "none configured".to_string(),
                n => format!("{enabled} of {n} enabled"),
            },
        ),
        ("skills", count(&repo_root).to_string()),
        (
            "memory",
            memory.map_or("unavailable".into(), |n| format!("{n} blocks")),
        ),
        (
            "sessions",
            sessions.map_or("unavailable".into(), |n| n.to_string()),
        ),
    ];
    for (label, value) in rows {
        println!("{label:<10} {value}");
    }
    Ok(())
}

fn count(repo_root: &Path) -> usize {
    crate::chat::discover_skills(repo_root).visible().count()
}
