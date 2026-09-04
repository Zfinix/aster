//! The cards and pickers the panel fills from the CLI: `/status`, `/memory`,
//! `/diff`, the MCP list, the endpoint catalog, and compaction.

use std::path::Path;
use std::process::Stdio;

use serde_json::{Value, json};
use tokio::process::Command;

use crate::cli::Cli;

/// The TUI's `/status` rows. Session-local facts (context spent, usage) are the
/// panel's to add: the CLI has no view of a conversation it is not running.
pub async fn status(cli: &Cli) -> Result<Value, String> {
    let s = cli.json(&["status"]).await?;
    let limits = &s["limits"];
    let servers = s["mcp"]["servers"].as_array().map_or(0, Vec::len);
    let rows = [
        ("model", text(&s["model"])),
        (
            "provider",
            format!("{} · {}", text(&s["provider"]), text(&s["base_url"])),
        ),
        ("effort", text(&s["effort"])),
        ("mode", text(&s["mode"])),
        (
            "context",
            format!(
                "{} chars before auto-compact",
                human(limits["compact_budget_chars"].as_u64().unwrap_or(0))
            ),
        ),
        (
            "rounds",
            format!(
                "{} tool rounds per turn",
                limits["max_tool_rounds"].as_u64().unwrap_or(0)
            ),
        ),
        (
            "mcp",
            match servers {
                0 => "none configured".to_string(),
                total => format!("{} of {total} enabled", s["mcp"]["enabled"]),
            },
        ),
        ("skills", text(&s["skills"])),
        ("memory", count(&s["memory_blocks"], "blocks")),
        ("sessions", count(&s["sessions"], "")),
    ];
    Ok(Value::Array(
        rows.into_iter()
            .map(|(label, value)| json!({ "label": label, "value": value }))
            .collect(),
    ))
}

/// The history size the CLI auto-compacts above, so the composer can show how
/// much of it the conversation has spent. Zero when the CLI cannot be asked.
pub async fn context_budget(cli: &Cli) -> u64 {
    cli.json(&["status"])
        .await
        .ok()
        .and_then(|s| s["limits"]["compact_budget_chars"].as_u64())
        .unwrap_or(0)
}

pub async fn memory_rows(cli: &Cli) -> Result<Value, String> {
    let parsed = cli.json(&["memory", "list"]).await?;
    let rows = parsed["blocks"]
        .as_array()
        .map(|blocks| {
            blocks
                .iter()
                .map(|block| json!({ "label": block["name"], "value": block["description"] }))
                .collect()
        })
        .unwrap_or_default();
    Ok(Value::Array(rows))
}

pub async fn mcp_servers(cli: &Cli) -> Value {
    cli.json(&["mcp", "list", "--no-connect"])
        .await
        .map(|parsed| parsed["servers"].clone())
        .unwrap_or(Value::Null)
}

pub async fn toggle_mcp(cli: &Cli, name: &str, disabled: bool) -> Result<(), String> {
    let action = if disabled { "disable" } else { "enable" };
    cli.json(&["mcp", action, name]).await.map(|_| ())
}

pub async fn providers(cli: &Cli) -> Value {
    cli.json(&["models", "--providers"])
        .await
        .map(|parsed| parsed["providers"].clone())
        .unwrap_or(Value::Null)
}

/// The login or key the endpoint in use still needs, from `aster config key`;
/// null once it has one.
pub async fn setup(cli: &Cli) -> Value {
    cli.json(&["config", "key"])
        .await
        .map(|parsed| parsed["setup"].clone())
        .unwrap_or(Value::Null)
}

/// An endpoint's catalog. One that will not answer is not fatal: the picker
/// still switched, and a model id can be typed by hand.
pub async fn models_for(cli: &Cli, model: &str) -> Value {
    let out = cli.run(&["models", "--model", model, "--json"], None).await;
    match out {
        Ok(out) if out.code == 0 => {
            serde_json::from_str(out.stdout.trim()).unwrap_or_else(|_| json!([]))
        }
        _ => json!([]),
    }
}

/// Fold the head of a transcript into a summary. The browser owns its history,
/// so the shorter one comes back for it to adopt rather than being applied here.
pub async fn compact(cli: &Cli, messages: &Value, model: Option<&str>) -> Result<Value, String> {
    let mut args: Vec<&str> = vec!["chat", "--messages-json", "-", "--compact", "--json"];
    if let Some(model) = model.filter(|m| !m.is_empty()) {
        args.push("--model");
        args.push(model);
    }
    let out = cli.run(&args, Some(&messages.to_string())).await?;
    let parsed: Value = serde_json::from_str(out.stdout.trim()).map_err(|_| {
        let stderr = out.stderr.trim();
        match stderr.is_empty() {
            true => format!("compacting failed (exit {})", out.code),
            false => stderr.to_string(),
        }
    })?;
    if parsed.get("ok") == Some(&Value::Bool(false)) || parsed.get("messages").is_none() {
        return Err(parsed["error"]
            .as_str()
            .unwrap_or("compacting failed")
            .to_string());
    }
    Ok(json!({
        "summary": parsed["summary"].as_str().unwrap_or_default(),
        "folded": parsed["folded"].as_u64().unwrap_or(0),
        "messages": parsed["messages"],
    }))
}

/// The slash commands this repo's skills contribute, deduplicated by name.
pub async fn skills(cli: &Cli) -> Value {
    let Ok(parsed) = cli.json(&["skills", "list"]).await else {
        return json!([]);
    };
    let from = |group: &Value, plugin_key: Option<&str>| -> Vec<Value> {
        group
            .as_array()
            .map(|entries| {
                entries
                    .iter()
                    .flat_map(|entry| {
                        let plugin = plugin_key.map(|key| entry[key].clone());
                        entry["skills"]
                            .as_array()
                            .cloned()
                            .unwrap_or_default()
                            .into_iter()
                            .map(move |skill| {
                                json!({
                                    "name": skill["name"],
                                    "detail": first_sentence(skill["description"].as_str().unwrap_or_default()),
                                    "plugin": plugin.clone(),
                                })
                            })
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    let mut seen = std::collections::HashSet::new();
    let commands: Vec<Value> = from(&parsed["scopes"], None)
        .into_iter()
        .chain(from(&parsed["plugins"], Some("plugin")))
        .filter(|command| seen.insert(command["name"].to_string()))
        .collect();
    Value::Array(commands)
}

/// Everything uncommitted, the same range the TUI's `/diff` shows.
pub async fn working_diff(root: &Path) -> Result<String, String> {
    let out = git(root, &["diff", "HEAD"]).await?;
    Ok(out)
}

/// Current branch, or nothing outside a git repo and on a detached head.
pub async fn branch(root: &Path) -> Option<String> {
    let branch = git(root, &["rev-parse", "--abbrev-ref", "HEAD"])
        .await
        .ok()?;
    let branch = branch.trim();
    (!branch.is_empty() && branch != "HEAD").then(|| branch.to_string())
}

async fn git(root: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .map_err(|e| format!("could not run git: {e}"))?;
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    if !out.status.success() && stdout.is_empty() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(match stderr.is_empty() {
            true => "could not run git".to_string(),
            false => stderr,
        });
    }
    Ok(stdout)
}

/// Skill descriptions run to a paragraph of trigger phrases; a menu row has
/// room for a sentence.
fn first_sentence(description: &str) -> &str {
    match description.split_once(". ") {
        Some((first, _)) => first,
        None => description,
    }
}

fn text(value: &Value) -> String {
    value
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| value.to_string())
}

fn count(value: &Value, unit: &str) -> String {
    match value.as_u64() {
        None => "unavailable".to_string(),
        Some(n) if unit.is_empty() => n.to_string(),
        Some(n) => format!("{n} {unit}"),
    }
}

fn human(n: u64) -> String {
    match n >= 1000 {
        true => format!("{}k", n.div_ceil(1000)),
        false => n.to_string(),
    }
}

#[cfg(test)]
#[path = "tests/info_test.rs"]
mod tests;
