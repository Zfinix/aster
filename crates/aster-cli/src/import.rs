//! Import MCP servers and chat histories from Claude Code, Codex, Cursor,
//! opencode, and Hermes.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use aster_persist::{MessageEvent, SessionMeta, TRANSCRIPT_VERSION};
use chrono::{DateTime, Utc};
use serde_json::{Value, json};

/// Which tool an import reads from. Omitted, every tool is tried.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Source {
    /// Claude Code: `~/.claude.json`, `.mcp.json`, `~/.claude/projects/`.
    Claude,
    /// Codex: `~/.codex/config.toml`, `~/.codex/sessions/`.
    Codex,
    /// Cursor: `~/.cursor/mcp.json` and its chat database.
    Cursor,
    /// opencode: `opencode.json` and `~/.local/share/opencode/opencode.db`.
    Opencode,
    /// Hermes: `~/.hermes/config.yaml` and `~/.hermes/state.db`.
    Hermes,
}

impl Source {
    fn name(self) -> &'static str {
        match self {
            Source::Claude => "claude",
            Source::Codex => "codex",
            Source::Cursor => "cursor",
            Source::Opencode => "opencode",
            Source::Hermes => "hermes",
        }
    }
}

fn picked(from: Option<Source>) -> Vec<Source> {
    match from {
        Some(source) => vec![source],
        None => vec![
            Source::Claude,
            Source::Codex,
            Source::Cursor,
            Source::Opencode,
            Source::Hermes,
        ],
    }
}

fn home() -> Result<PathBuf> {
    dirs::home_dir().context("no home directory")
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Scope {
    Global,
    Repo,
}

struct FoundServer {
    name: String,
    scope: Scope,
    value: Value,
}

/// `aster mcp import`: copy MCP servers into the standard `.mcp.json` files
/// aster reads natively (`~/.aster/mcp.json`, repo `.mcp.json`). Both shapes
/// come across: a `command` to spawn, or a `url` to connect to.
pub fn run_mcp_import(from: Option<Source>, repo_root: Option<&Path>) -> Result<()> {
    let settings = crate::settings::Settings::load(repo_root)?;
    let mut found: Vec<FoundServer> = Vec::new();
    let mut unusable: Vec<String> = Vec::new();
    for source in picked(from) {
        let servers = match source {
            Source::Claude => claude_mcp(repo_root)?,
            Source::Codex => codex_mcp()?,
            Source::Cursor => cursor_mcp(repo_root)?,
            Source::Opencode => opencode_mcp(repo_root)?,
            Source::Hermes => hermes_mcp()?,
        };
        for (name, scope, value) in servers {
            if found.iter().any(|s| s.name == name) {
                continue;
            }
            let usable = ["command", "url"].iter().any(|key| {
                value
                    .get(key)
                    .and_then(|v| v.as_str())
                    .is_some_and(|v| !v.is_empty())
            });
            match usable {
                true => found.push(FoundServer { name, scope, value }),
                false => unusable.push(name),
            }
        }
    }

    let (mut new, existing): (Vec<_>, Vec<_>) = found
        .into_iter()
        .partition(|s| !settings.mcp.servers.contains_key(&s.name));
    let existing: Vec<String> = existing.into_iter().map(|s| s.name).collect();

    if crate::picker::is_tty() && !new.is_empty() {
        let items: Vec<crate::picker::Item> = new
            .iter()
            .map(|s| crate::picker::Item {
                name: s.name.clone(),
                detail: spawn_line(&s.value),
            })
            .collect();
        let Some(chosen) =
            crate::picker::multi_select("Select MCP servers to import", &items, true)?
        else {
            println!("cancelled, nothing imported");
            return Ok(());
        };
        let keep: Vec<bool> = (0..new.len()).map(|i| chosen.contains(&i)).collect();
        let mut it = keep.iter();
        new.retain(|_| *it.next().expect("one flag per server"));
    }

    let mut written: Vec<(PathBuf, Vec<String>)> = Vec::new();
    for scope in [Scope::Repo, Scope::Global] {
        // Repo-scoped servers fall back to the global file outside a repo.
        let servers: Vec<&FoundServer> = new
            .iter()
            .filter(|s| match (scope, repo_root) {
                (Scope::Repo, Some(_)) => s.scope == Scope::Repo,
                (Scope::Repo, None) => false,
                (Scope::Global, Some(_)) => s.scope == Scope::Global,
                (Scope::Global, None) => true,
            })
            .collect();
        if servers.is_empty() {
            continue;
        }
        let path = match (scope, repo_root) {
            (Scope::Repo, Some(root)) => root.join(".mcp.json"),
            _ => home()?.join(".aster/mcp.json"),
        };
        let names = merge_mcp_json(&path, &servers)?;
        written.push((path, names));
    }

    if crate::json_mode() {
        println!(
            "{}",
            json!({
                "ok": true,
                "imported": written.iter().map(|(path, names)| json!({
                    "path": path.display().to_string(),
                    "servers": names,
                })).collect::<Vec<_>>(),
                "skipped_existing": existing,
                "skipped_unusable": unusable,
            })
        );
        return Ok(());
    }
    if written.is_empty() {
        println!("no new MCP servers to import");
    }
    for (path, names) in &written {
        println!(
            "imported {} server(s) into {}: {}",
            names.len(),
            path.display(),
            names.join(", ")
        );
    }
    if !existing.is_empty() {
        println!("already configured: {}", existing.join(", "));
    }
    if !unusable.is_empty() {
        println!(
            "skipped servers that name neither a command nor a url: {}",
            unusable.join(", ")
        );
    }
    Ok(())
}

fn spawn_line(value: &Value) -> String {
    let command = value
        .get("command")
        .and_then(|c| c.as_str())
        .unwrap_or_default();
    if command.is_empty() {
        return value
            .get("url")
            .and_then(|u| u.as_str())
            .unwrap_or_default()
            .to_string();
    }
    let mut out = command.to_string();
    for arg in value
        .get("args")
        .and_then(|a| a.as_array())
        .into_iter()
        .flatten()
        .filter_map(|a| a.as_str())
    {
        out.push(' ');
        out.push_str(arg);
    }
    out
}

fn merge_mcp_json(path: &Path, servers: &[&FoundServer]) -> Result<Vec<String>> {
    let mut config: Value = match std::fs::read_to_string(path) {
        Ok(text) => {
            serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))?
        }
        Err(_) => json!({}),
    };
    if !config.is_object() {
        bail!("{} is not a JSON object", path.display());
    }
    let map = config
        .as_object_mut()
        .expect("checked object")
        .entry("mcpServers")
        .or_insert_with(|| json!({}));
    let Some(map) = map.as_object_mut() else {
        bail!("mcpServers in {} is not an object", path.display());
    };
    let mut names = Vec::new();
    for server in servers {
        if map.contains_key(&server.name) {
            continue;
        }
        let mut entry = serde_json::Map::new();
        for key in ["command", "args", "env", "url", "headers", "type"] {
            if let Some(v) = server.value.get(key) {
                entry.insert(key.to_string(), v.clone());
            }
        }
        map.insert(server.name.clone(), Value::Object(entry));
        names.push(server.name.clone());
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    let out = serde_json::to_string_pretty(&config)?;
    std::fs::write(path, out + "\n").with_context(|| format!("writing {}", path.display()))?;
    Ok(names)
}

fn claude_mcp(repo_root: Option<&Path>) -> Result<Vec<(String, Scope, Value)>> {
    let mut out = Vec::new();
    if let Ok(text) = std::fs::read_to_string(home()?.join(".claude.json")) {
        let config: Value = serde_json::from_str(&text).context("parsing ~/.claude.json")?;
        push_servers(&mut out, Scope::Global, &config["mcpServers"]);
        if let Some(root) = repo_root {
            let project = &config["projects"][root.to_string_lossy().as_ref()];
            push_servers(&mut out, Scope::Repo, &project["mcpServers"]);
        }
    }
    Ok(out)
}

fn codex_mcp() -> Result<Vec<(String, Scope, Value)>> {
    let Ok(text) = std::fs::read_to_string(home()?.join(".codex/config.toml")) else {
        return Ok(Vec::new());
    };
    let config: toml::Value = toml::from_str(&text).context("parsing ~/.codex/config.toml")?;
    let Some(servers) = config.get("mcp_servers").and_then(|v| v.as_table()) else {
        return Ok(Vec::new());
    };
    Ok(servers
        .iter()
        .filter_map(|(name, v)| {
            let as_json = serde_json::to_value(v).ok()?;
            Some((name.clone(), Scope::Global, as_json))
        })
        .collect())
}

fn cursor_mcp(repo_root: Option<&Path>) -> Result<Vec<(String, Scope, Value)>> {
    let mut out = Vec::new();
    let mut paths = vec![(home()?.join(".cursor/mcp.json"), Scope::Global)];
    if let Some(root) = repo_root {
        paths.push((root.join(".cursor/mcp.json"), Scope::Repo));
    }
    for (path, scope) in paths {
        if let Ok(text) = std::fs::read_to_string(&path) {
            let config: Value = serde_json::from_str(&text)
                .with_context(|| format!("parsing {}", path.display()))?;
            push_servers(&mut out, scope, &config["mcpServers"]);
        }
    }
    Ok(out)
}

fn push_servers(out: &mut Vec<(String, Scope, Value)>, scope: Scope, servers: &Value) {
    if let Some(map) = servers.as_object() {
        out.extend(map.iter().map(|(k, v)| (k.clone(), scope, v.clone())));
    }
}

fn opencode_mcp(repo_root: Option<&Path>) -> Result<Vec<(String, Scope, Value)>> {
    let mut out = Vec::new();
    let mut paths = vec![(
        home()?.join(".config/opencode/opencode.json"),
        Scope::Global,
    )];
    if let Some(root) = repo_root {
        paths.push((root.join("opencode.json"), Scope::Repo));
    }
    for (path, scope) in paths {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let config: Value =
            serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        if let Some(map) = config["mcp"].as_object() {
            for (name, v) in map {
                out.push((name.clone(), scope, opencode_server(v)));
            }
        }
    }
    Ok(out)
}

fn hermes_mcp() -> Result<Vec<(String, Scope, Value)>> {
    let path = home()?.join(".hermes/config.yaml");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Ok(Vec::new());
    };
    let config: serde_yaml::Value =
        serde_yaml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
    let as_json = serde_json::to_value(&config["mcp_servers"]).unwrap_or(Value::Null);
    let mut out = Vec::new();
    push_servers(&mut out, Scope::Global, &as_json);
    Ok(out)
}

fn opencode_server(v: &Value) -> Value {
    let mut out = serde_json::Map::new();
    let mut argv = v["command"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter();
    if let Some(command) = argv.next() {
        out.insert("command".into(), command);
    }
    let args: Vec<Value> = argv.collect();
    if !args.is_empty() {
        out.insert("args".into(), args.into());
    }
    if let Some(env) = v.get("environment") {
        out.insert("env".into(), env.clone());
    }
    if let Some(url) = v.get("url") {
        out.insert("url".into(), url.clone());
    }
    Value::Object(out)
}

struct ImportedSession {
    id: String,
    title: Option<String>,
    model: Option<String>,
    created_at: DateTime<Utc>,
    messages: Vec<ImportedMessage>,
}

struct ImportedMessage {
    role: &'static str,
    text: String,
    ts: DateTime<Utc>,
}

/// Inclusive window over session creation times, plus list order.
pub struct TimeRange {
    pub(crate) since: Option<DateTime<Utc>>,
    pub(crate) until: Option<DateTime<Utc>>,
    pub(crate) oldest_first: bool,
}

impl TimeRange {
    pub fn parse(since: Option<&str>, until: Option<&str>) -> Result<Self> {
        Ok(Self {
            since: since.map(parse_when).transpose()?,
            until: until.map(parse_when_until).transpose()?,
            oldest_first: false,
        })
    }

    fn contains(&self, at: DateTime<Utc>) -> bool {
        self.since.is_none_or(|c| at >= c) && self.until.is_none_or(|c| at <= c)
    }

    fn sort(&self, sessions: &mut [ImportedSession]) {
        if self.oldest_first {
            sessions.sort_by_key(|s| s.created_at);
        } else {
            sessions.sort_by_key(|s| std::cmp::Reverse(s.created_at));
        }
    }
}

/// Accepts `2026-09-05`, `today`, or a span back like `30m`, `12h`, `7d`.
fn parse_when(input: &str) -> Result<DateTime<Utc>> {
    let input = input.trim();
    if let Ok(date) = chrono::NaiveDate::parse_from_str(input, "%Y-%m-%d") {
        return midnight(date).context("invalid date");
    }
    if input.eq_ignore_ascii_case("today") {
        return midnight(Utc::now().date_naive()).context("invalid date");
    }
    relative(input)
}

/// `--until` lands at the end of the given day, so the day itself is included.
fn parse_when_until(input: &str) -> Result<DateTime<Utc>> {
    let input = input.trim();
    if let Ok(date) = chrono::NaiveDate::parse_from_str(input, "%Y-%m-%d") {
        return date
            .and_hms_opt(23, 59, 59)
            .map(|dt| dt.and_utc())
            .context("invalid date");
    }
    parse_when(input)
}

fn midnight(date: chrono::NaiveDate) -> Option<DateTime<Utc>> {
    date.and_hms_opt(0, 0, 0).map(|dt| dt.and_utc())
}

fn relative(input: &str) -> Result<DateTime<Utc>> {
    let (digits, unit) = input.split_at(input.len().checked_sub(1).context("empty time value")?);
    let n: i64 = digits
        .parse()
        .with_context(|| format!("cannot read a date or span from {input:?}"))?;
    let duration = match unit {
        "m" => chrono::Duration::minutes(n),
        "h" => chrono::Duration::hours(n),
        "d" => chrono::Duration::days(n),
        _ => bail!("unknown span unit {unit:?}; use m, h, or d"),
    };
    Ok(Utc::now() - duration)
}

fn first_user_text(session: &ImportedSession) -> &str {
    session
        .messages
        .iter()
        .find(|m| m.role == "user")
        .map(|m| m.text.as_str())
        .unwrap_or("(untitled)")
}

/// `aster sessions import`: copy this repo's conversations into the session
/// store. Only user and assistant text carries over; tool traffic stays behind.
pub async fn run_sessions_import(
    from: Option<Source>,
    range: TimeRange,
    dry_run: bool,
) -> Result<()> {
    let repo_root = std::env::current_dir().context("could not determine the current directory")?;
    let store = crate::persist::store()?;

    let mut found = Vec::new();
    for source in picked(from) {
        let mut sessions = match source {
            Source::Claude => claude_sessions(&repo_root)?,
            Source::Codex => codex_sessions(&repo_root)?,
            Source::Cursor => cursor_sessions(&repo_root)?,
            Source::Opencode => opencode_sessions(&repo_root)?,
            Source::Hermes => hermes_sessions(&repo_root)?,
        };
        let before = sessions.len();
        sessions.retain(|s| range.contains(s.created_at));
        let dropped = before - sessions.len();
        range.sort(&mut sessions);
        found.push((source, sessions, dropped));
    }

    let all: usize = found.iter().map(|(_, s, _)| s.len()).sum();
    if !dry_run && crate::picker::is_tty() && all > 0 {
        let items: Vec<crate::picker::Item> = found
            .iter()
            .flat_map(|(_, sessions, _)| sessions.iter())
            .map(|s| crate::picker::Item {
                name: s.id.clone(),
                detail: format!(
                    "{} · {}",
                    s.created_at.format("%Y-%m-%d"),
                    s.title.as_deref().unwrap_or_else(|| first_user_text(s)),
                ),
            })
            .collect();
        let Some(chosen) = crate::picker::multi_select("Select sessions to import", &items, true)?
        else {
            println!("cancelled, nothing imported");
            return Ok(());
        };
        let mut index = 0usize;
        for (_, sessions, _) in &mut found {
            let keep: Vec<bool> = (0..sessions.len())
                .map(|i| chosen.contains(&(index + i)))
                .collect();
            index += sessions.len();
            let mut it = keep.iter();
            sessions.retain(|_| *it.next().expect("one flag per session"));
        }
    }

    let mut report = Vec::new();
    for (source, sessions, dropped) in &found {
        let mut imported = Vec::new();
        let mut skipped = 0usize;
        for session in sessions {
            if dry_run {
                imported.push(session.id.clone());
                continue;
            }
            if write_session(&store, &repo_root, session)? {
                imported.push(session.id.clone());
            } else {
                skipped += 1;
            }
        }
        report.push((*source, imported, skipped, *dropped));
    }

    if crate::json_mode() {
        let out: Vec<Value> = report
            .iter()
            .map(|(source, imported, skipped, dropped)| {
                json!({
                    "source": source.name(),
                    "imported": imported,
                    "skipped_existing": skipped,
                    "outside_range": dropped,
                    "dry_run": dry_run,
                })
            })
            .collect();
        println!("{}", json!({ "ok": true, "sources": out }));
        return Ok(());
    }
    let mut landed = 0usize;
    for (source, imported, skipped, dropped) in report {
        let verb = if dry_run { "would import" } else { "imported" };
        landed += imported.len();
        match (imported.len(), skipped, dropped) {
            (0, 0, 0) => println!("{}: nothing found for this repo", source.name()),
            (n, s, d) => {
                let mut line = format!("{}: {verb} {n} session(s)", source.name());
                if s > 0 {
                    line.push_str(&format!(", {s} already imported"));
                }
                if d > 0 {
                    line.push_str(&format!(", {d} outside the date range"));
                }
                println!("{line}");
            }
        }
    }
    if dry_run {
        return Ok(());
    }
    // Landed imports go straight into the picker: enter resumes one there.
    // Scoped to this repo, since that is what the import just filled.
    if landed > 0 && crate::picker::is_tty() {
        return crate::sessions::pick_session(store, &repo_root, false).await;
    }
    println!("resume one with `aster --resume <ID>`; `aster sessions` lists them");
    Ok(())
}

fn write_session(
    store: &aster_persist::Store,
    repo_root: &Path,
    session: &ImportedSession,
) -> Result<bool> {
    let meta = SessionMeta {
        id: session.id.clone(),
        v: TRANSCRIPT_VERSION,
        created_at: session.created_at,
        cwd: repo_root.to_string_lossy().into_owned(),
        repo_root: repo_root.to_string_lossy().into_owned(),
        model: session.model.clone(),
        aster_version: option_env!("CARGO_PKG_VERSION").map(str::to_string),
        title: session.title.clone(),
        schedule: None,
    };
    let Some(mut writer) = store.import_session(repo_root, meta)? else {
        return Ok(false);
    };
    for message in &session.messages {
        writer.append_message(MessageEvent {
            role: message.role.to_string(),
            content: Some(message.text.clone()),
            tool_calls: Vec::new(),
            tool_call_id: None,
            ts: message.ts,
            usage: None,
            annotations: Vec::new(),
            reasoning: None,
        })?;
    }
    Ok(true)
}

fn claude_sessions(repo_root: &Path) -> Result<Vec<ImportedSession>> {
    let slug: String = repo_root
        .to_string_lossy()
        .chars()
        .map(|c| if c == '/' || c == '.' { '-' } else { c })
        .collect();
    let dir = home()?.join(".claude/projects").join(slug);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Ok(Vec::new());
    };

    let mut sessions = Vec::new();
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;

        let mut messages = Vec::new();
        let mut model = None;
        let mut title = None;
        for line in text.lines() {
            let Ok(v) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            // Claude Code names its sessions with a title line; keep the last.
            if v["type"] == json!("ai-title") {
                title = v["aiTitle"].as_str().map(str::to_string).or(title);
                continue;
            }
            // Sidechains are subagent transcripts, meta lines are bookkeeping.
            if v["isSidechain"] == json!(true) || v["isMeta"] == json!(true) {
                continue;
            }
            let role = match v["type"].as_str() {
                Some("user") => "user",
                Some("assistant") => "assistant",
                _ => continue,
            };
            let text = content_text(&v["message"]["content"]);
            if text.is_empty() {
                continue;
            }
            if model.is_none() {
                model = v["message"]["model"].as_str().map(str::to_string);
            }
            let ts = timestamp(&v["timestamp"]);
            messages.push(ImportedMessage { role, text, ts });
        }
        if messages.is_empty() {
            continue;
        }
        sessions.push(ImportedSession {
            id: format!("claude-{stem}"),
            title,
            model,
            created_at: messages[0].ts,
            messages,
        });
    }
    Ok(sessions)
}

fn codex_sessions(repo_root: &Path) -> Result<Vec<ImportedSession>> {
    let root = home()?.join(".codex/sessions");
    let mut files = Vec::new();
    let mut stack = vec![root];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
                files.push(path);
            }
        }
    }

    let mut sessions = Vec::new();
    for path in files {
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let mut meta_id = None;
        let mut created = None;
        let mut in_repo = false;
        let mut messages = Vec::new();
        for line in text.lines() {
            let Ok(v) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            // Newer files wrap items in `payload`; older ones are the item.
            let item = if v["payload"].is_object() {
                &v["payload"]
            } else {
                &v
            };
            if v["type"] == json!("session_meta") || item.get("cwd").is_some() {
                if let Some(cwd) = item["cwd"].as_str() {
                    in_repo = Path::new(cwd).starts_with(repo_root);
                }
                meta_id = item["id"].as_str().map(str::to_string).or(meta_id);
                created = Some(timestamp(&v["timestamp"]));
                continue;
            }
            if item["type"] != json!("message") {
                continue;
            }
            let role = match item["role"].as_str() {
                Some("user") => "user",
                Some("assistant") => "assistant",
                _ => continue,
            };
            let text = content_text(&item["content"]);
            if text.is_empty() {
                continue;
            }
            let ts = timestamp(&v["timestamp"]);
            messages.push(ImportedMessage { role, text, ts });
        }
        if !in_repo || messages.is_empty() {
            continue;
        }
        let id = meta_id.unwrap_or_else(|| {
            path.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown")
                .to_string()
        });
        sessions.push(ImportedSession {
            id: format!("codex-{id}"),
            title: None,
            model: None,
            created_at: created.unwrap_or(messages[0].ts),
            messages,
        });
    }
    Ok(sessions)
}

fn cursor_sessions(repo_root: &Path) -> Result<Vec<ImportedSession>> {
    let Some(user_dir) = cursor_user_dir()? else {
        return Ok(Vec::new());
    };
    let global_db = user_dir.join("globalStorage/state.vscdb");
    if !global_db.exists() {
        return Ok(Vec::new());
    }

    let mut composers: Vec<(String, Option<String>, Option<i64>)> = Vec::new();
    let ws_root = user_dir.join("workspaceStorage");
    if let Ok(entries) = std::fs::read_dir(&ws_root) {
        for entry in entries.filter_map(|e| e.ok()) {
            let dir = entry.path();
            if !workspace_matches(&dir.join("workspace.json"), repo_root) {
                continue;
            }
            let rows = sqlite_rows(
                &dir.join("state.vscdb"),
                "SELECT value FROM ItemTable WHERE key='composer.composerData';",
            )
            .unwrap_or_default();
            for row in rows {
                let Ok(v) = serde_json::from_str::<Value>(&row) else {
                    continue;
                };
                for c in v["allComposers"].as_array().into_iter().flatten() {
                    if let Some(id) = c["composerId"].as_str() {
                        composers.push((
                            id.to_string(),
                            c["name"].as_str().map(str::to_string),
                            c["createdAt"].as_i64(),
                        ));
                    }
                }
            }
        }
    }

    let mut sessions = Vec::new();
    for (id, name, created_ms) in composers {
        // Ids splice into SQL, so anything but a uuid is refused outright.
        if !id.chars().all(|c| c.is_ascii_hexdigit() || c == '-') {
            continue;
        }
        let Some(data) = sqlite_rows(
            &global_db,
            &format!("SELECT value FROM cursorDiskKV WHERE key='composerData:{id}';"),
        )?
        .into_iter()
        .next() else {
            continue;
        };
        let Ok(composer) = serde_json::from_str::<Value>(&data) else {
            continue;
        };

        let created = created_ms
            .or(composer["createdAt"].as_i64())
            .and_then(DateTime::from_timestamp_millis)
            .unwrap_or_else(Utc::now);
        let mut messages = Vec::new();
        let headers = composer["fullConversationHeadersOnly"].as_array();
        if let Some(headers) = headers.filter(|h| !h.is_empty()) {
            let mut bubbles: BTreeMap<String, (i64, String)> = BTreeMap::new();
            for row in sqlite_rows(
                &global_db,
                &format!("SELECT value FROM cursorDiskKV WHERE key LIKE 'bubbleId:{id}:%';"),
            )? {
                let Ok(b) = serde_json::from_str::<Value>(&row) else {
                    continue;
                };
                if let (Some(bid), Some(kind)) = (b["bubbleId"].as_str(), b["type"].as_i64()) {
                    let text = b["text"].as_str().unwrap_or_default().to_string();
                    bubbles.insert(bid.to_string(), (kind, text));
                }
            }
            for header in headers {
                let Some((kind, text)) = header["bubbleId"].as_str().and_then(|b| bubbles.get(b))
                else {
                    continue;
                };
                push_cursor_message(&mut messages, *kind, text, created);
            }
        } else {
            // Older Cursor versions inline the conversation in the composer row.
            for item in composer["conversation"].as_array().into_iter().flatten() {
                let kind = item["type"].as_i64().unwrap_or(0);
                let text = item["text"].as_str().unwrap_or_default();
                push_cursor_message(&mut messages, kind, text, created);
            }
        }
        if messages.is_empty() {
            continue;
        }
        sessions.push(ImportedSession {
            id: format!("cursor-{id}"),
            title: name.filter(|n| !n.is_empty()),
            model: None,
            created_at: created,
            messages,
        });
    }
    Ok(sessions)
}

fn push_cursor_message(
    messages: &mut Vec<ImportedMessage>,
    kind: i64,
    text: &str,
    ts: DateTime<Utc>,
) {
    let role = match kind {
        1 => "user",
        2 => "assistant",
        _ => return,
    };
    if text.trim().is_empty() || injected(text) {
        return;
    }
    messages.push(ImportedMessage {
        role,
        text: text.to_string(),
        ts,
    });
}

fn cursor_user_dir() -> Result<Option<PathBuf>> {
    let home = home()?;
    Ok([
        home.join("Library/Application Support/Cursor/User"),
        home.join(".config/Cursor/User"),
    ]
    .into_iter()
    .find(|p| p.exists()))
}

fn workspace_matches(workspace_json: &Path, repo_root: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(workspace_json) else {
        return false;
    };
    let Ok(v) = serde_json::from_str::<Value>(&text) else {
        return false;
    };
    let Some(folder) = v["folder"].as_str() else {
        return false;
    };
    let path = folder.strip_prefix("file://").unwrap_or(folder);
    let path = path.replace("%20", " ");
    Path::new(&path).starts_with(repo_root)
}

fn opencode_sessions(repo_root: &Path) -> Result<Vec<ImportedSession>> {
    let db = home()?.join(".local/share/opencode/opencode.db");
    if !db.exists() {
        return Ok(Vec::new());
    }

    let mut sessions = Vec::new();
    // Child sessions (parent_id set) are subagent runs, not conversations.
    let rows = sqlite_json(
        &db,
        "SELECT id, title, model, directory, parent_id, time_created FROM session;",
    )?;
    for row in rows {
        if !row["parent_id"].is_null() {
            continue;
        }
        let Some(dir) = row["directory"].as_str() else {
            continue;
        };
        if !Path::new(dir).starts_with(repo_root) {
            continue;
        }
        let Some(sid) = row["id"].as_str() else {
            continue;
        };
        // Ids splice into SQL, so anything unexpected is refused outright.
        if !sid
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            continue;
        }

        let mut texts: BTreeMap<String, String> = BTreeMap::new();
        for part in sqlite_json(
            &db,
            &format!(
                "SELECT message_id, data FROM part WHERE session_id='{sid}' ORDER BY time_created, id;"
            ),
        )? {
            let Ok(data) = serde_json::from_str::<Value>(part["data"].as_str().unwrap_or(""))
            else {
                continue;
            };
            if data["type"] != json!("text") {
                continue;
            }
            let text = data["text"].as_str().unwrap_or_default();
            if text.trim().is_empty() || injected(text) {
                continue;
            }
            let Some(mid) = part["message_id"].as_str() else {
                continue;
            };
            let slot = texts.entry(mid.to_string()).or_default();
            if !slot.is_empty() {
                slot.push_str("\n\n");
            }
            slot.push_str(text);
        }

        let mut messages = Vec::new();
        for msg in sqlite_json(
            &db,
            &format!(
                "SELECT id, data, time_created FROM message WHERE session_id='{sid}' ORDER BY time_created, id;"
            ),
        )? {
            let Ok(data) = serde_json::from_str::<Value>(msg["data"].as_str().unwrap_or("")) else {
                continue;
            };
            let role = match data["role"].as_str() {
                Some("user") => "user",
                Some("assistant") => "assistant",
                _ => continue,
            };
            let Some(text) = msg["id"].as_str().and_then(|id| texts.get(id)) else {
                continue;
            };
            let ts = msg["time_created"]
                .as_i64()
                .and_then(DateTime::from_timestamp_millis)
                .unwrap_or_else(Utc::now);
            messages.push(ImportedMessage {
                role,
                text: text.clone(),
                ts,
            });
        }
        if messages.is_empty() {
            continue;
        }
        sessions.push(ImportedSession {
            id: format!("opencode-{sid}"),
            title: row["title"]
                .as_str()
                .filter(|t| !t.is_empty())
                .map(str::to_string),
            model: row["model"]
                .as_str()
                .filter(|m| !m.is_empty())
                .map(str::to_string),
            created_at: row["time_created"]
                .as_i64()
                .and_then(DateTime::from_timestamp_millis)
                .unwrap_or_else(Utc::now),
            messages,
        });
    }
    Ok(sessions)
}

fn hermes_sessions(repo_root: &Path) -> Result<Vec<ImportedSession>> {
    let db = home()?.join(".hermes/state.db");
    if !db.exists() {
        return Ok(Vec::new());
    }

    let mut sessions = Vec::new();
    // Child sessions (parent_session_id set) are subagent runs.
    let rows = sqlite_json(
        &db,
        "SELECT id, title, model, cwd, git_repo_root, started_at FROM sessions \
         WHERE parent_session_id IS NULL;",
    )?;
    for row in rows {
        let in_repo = [&row["git_repo_root"], &row["cwd"]]
            .iter()
            .filter_map(|v| v.as_str())
            .any(|dir| Path::new(dir).starts_with(repo_root));
        if !in_repo {
            continue;
        }
        let Some(sid) = row["id"].as_str() else {
            continue;
        };
        // Ids splice into SQL, so anything unexpected is refused outright.
        if !sid
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            continue;
        }

        let mut messages = Vec::new();
        for msg in sqlite_json(
            &db,
            &format!(
                "SELECT role, content, timestamp FROM messages WHERE session_id='{sid}' \
                 AND active=1 AND tool_call_id IS NULL ORDER BY id;"
            ),
        )? {
            let role = match msg["role"].as_str() {
                Some("user") => "user",
                Some("assistant") => "assistant",
                _ => continue,
            };
            let Some(text) = msg["content"].as_str() else {
                continue;
            };
            if text.trim().is_empty() || injected(text) {
                continue;
            }
            let ts = msg["timestamp"]
                .as_f64()
                .and_then(|s| DateTime::from_timestamp_millis((s * 1000.0) as i64))
                .unwrap_or_else(Utc::now);
            messages.push(ImportedMessage {
                role,
                text: text.to_string(),
                ts,
            });
        }
        if messages.is_empty() {
            continue;
        }
        sessions.push(ImportedSession {
            id: format!("hermes-{sid}"),
            title: row["title"]
                .as_str()
                .filter(|t| !t.is_empty())
                .map(str::to_string),
            model: row["model"]
                .as_str()
                .filter(|m| !m.is_empty())
                .map(str::to_string),
            created_at: row["started_at"]
                .as_f64()
                .and_then(|s| DateTime::from_timestamp_millis((s * 1000.0) as i64))
                .unwrap_or(messages[0].ts),
            messages,
        });
    }
    Ok(sessions)
}

fn sqlite_json(db: &Path, sql: &str) -> Result<Vec<Value>> {
    let output = Command::new("sqlite3")
        .arg("-readonly")
        .arg("-json")
        .arg(db)
        .arg(sql)
        .output()
        .context("running sqlite3 (required to read opencode's database)")?;
    if !output.status.success() {
        bail!(
            "sqlite3 failed on {}: {}",
            db.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.trim().is_empty() {
        return Ok(Vec::new());
    }
    let rows: Value =
        serde_json::from_str(stdout.trim()).context("parsing sqlite3 -json output")?;
    Ok(rows.as_array().cloned().unwrap_or_default())
}

fn sqlite_rows(db: &Path, sql: &str) -> Result<Vec<String>> {
    let output = Command::new("sqlite3")
        .arg("-readonly")
        .arg(db)
        .arg(sql)
        .output()
        .context("running sqlite3 (required to read Cursor's chat database)")?;
    if !output.status.success() {
        bail!(
            "sqlite3 failed on {}: {}",
            db.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect())
}

fn content_text(content: &Value) -> String {
    let parts: Vec<&str> = match content {
        Value::String(s) => vec![s.as_str()],
        Value::Array(items) => items
            .iter()
            .filter(|p| {
                matches!(
                    p["type"].as_str(),
                    Some("text") | Some("input_text") | Some("output_text") | None
                )
            })
            .filter_map(|p| p["text"].as_str())
            .collect(),
        _ => Vec::new(),
    };
    parts
        .into_iter()
        .filter(|t| !t.trim().is_empty() && !injected(t))
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn injected(text: &str) -> bool {
    const TAGS: &[&str] = &[
        "<ide_",
        "<system-reminder",
        "<command-name",
        "<local-command",
        "<bash-input",
        "<bash-stdout",
        "<user-prompt-submit-hook",
        "<session-start-hook",
        "<task-notification",
        "<permissions instructions>",
        "<environment_context",
        "<user_instructions",
        "<turn_context",
        "<queued-user-input",
    ];
    let t = text.trim_start();
    TAGS.iter().any(|tag| t.starts_with(tag))
}

fn timestamp(v: &Value) -> DateTime<Utc> {
    v.as_str()
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(Utc::now)
}

#[cfg(test)]
#[path = "tests/import_test.rs"]
mod tests;
