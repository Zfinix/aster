use std::env;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use aster_persist::TranscriptEvent;
use clap::{Args, Subcommand};
use serde_json::{Value, json};

#[derive(Args)]
pub struct SessionsArgs {
    #[command(subcommand)]
    cmd: Option<SessionsCmd>,

    /// List sessions from every project, not just this folder. `show` and
    /// `delete` already look outside it when the id is not local.
    #[arg(long, global = true)]
    all: bool,
}

impl SessionsArgs {
    pub fn is_interactive(&self) -> bool {
        match &self.cmd {
            None => crate::picker::is_tty(),
            Some(SessionsCmd::Import { dry_run, .. }) => !dry_run && crate::picker::is_tty(),
            Some(_) => false,
        }
    }
}

#[derive(Subcommand)]
enum SessionsCmd {
    /// List saved sessions for this repo (the default).
    List,
    /// Print a session's full transcript by id.
    Show { id: String },
    /// Delete a saved session by id.
    Delete { id: String },
    /// Copy this repo's conversations from another coding tool (Claude Code, Codex, Cursor, opencode, Hermes).
    Import {
        /// Which tool to read; omitted, all three are tried.
        #[arg(long, value_enum)]
        from: Option<crate::import::Source>,
        /// Report what would be imported without writing anything.
        #[arg(long)]
        dry_run: bool,
    },
    /// Delete stale sessions: empty ones always, plus anything past the limits.
    Prune {
        /// Keep only this many of the newest sessions with turns in them.
        #[arg(long, value_name = "N")]
        keep: Option<usize>,
        /// Delete sessions older than this many days.
        #[arg(long, value_name = "DAYS")]
        older_than: Option<i64>,
    },
}

/// Interactive `aster sessions`: enter resumes the highlighted session in
/// the chat TUI, `d` deletes it in place.
pub(crate) async fn pick_session(
    store: aster_persist::Store,
    repo_root: &Path,
    all: bool,
) -> Result<()> {
    loop {
        let metas = if all {
            store.list_all_sessions()?
        } else {
            store.list_sessions(repo_root)?
        };
        if metas.is_empty() {
            println!("no sessions for this repo yet");
            return Ok(());
        }
        let (owned, items): (Vec<(String, PathBuf)>, Vec<crate::picker::Item>) = metas
            .into_iter()
            .map(|meta| {
                // With --all a row can belong to another checkout, so it has to
                // be read and deleted against its own repo root.
                let owner = PathBuf::from(&meta.repo_root);
                let transcript = store.resume(&owner, &meta.id).ok();
                let turns = transcript
                    .as_ref()
                    .map(|t| t.user_turn_count())
                    .unwrap_or(0);
                let title = transcript
                    .as_ref()
                    .and_then(|t| t.display_title())
                    .map(|s| truncate(s.trim(), 80))
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| "(empty)".into());
                let when = meta.created_at.format("%Y-%m-%d %H:%M");
                let project = if all {
                    format!("{} · ", project_name(&meta.repo_root))
                } else {
                    String::new()
                };
                let item = crate::picker::Item {
                    name: title,
                    detail: format!("{project}{when} · {turns} turns · {}", meta.id),
                };
                ((meta.id, owner), item)
            })
            .unzip();
        match crate::picker::select_action("Sessions", &items)? {
            Some((i, crate::picker::Action::Open)) => {
                let id = owned[i].0.clone();
                drop(store);
                return crate::chat::run(crate::chat::resume_args(&id)).await;
            }
            Some((i, crate::picker::Action::Delete)) => {
                let (id, owner) = &owned[i];
                store.delete_session(owner, id)?;
                eprintln!("deleted {id}");
            }
            None => return Ok(()),
        }
    }
}

pub async fn run_sessions(args: SessionsArgs) -> Result<()> {
    let repo_root = env::current_dir().context("could not determine the current directory")?;
    let store = crate::persist::store()?;
    let interactive = args.is_interactive();

    match args.cmd.unwrap_or(SessionsCmd::List) {
        SessionsCmd::Import { from, dry_run } => {
            return crate::import::run_sessions_import(from, dry_run).await;
        }
        SessionsCmd::List if interactive => {
            return pick_session(store, &repo_root, args.all).await;
        }
        SessionsCmd::List => {
            let metas = if args.all {
                store.list_all_sessions()?
            } else {
                store.list_sessions(&repo_root)?
            };
            let rows: Vec<(_, usize, String)> = metas
                .into_iter()
                .map(|meta| {
                    // With --all a session belongs to another checkout, so it
                    // has to be read against its own repo root.
                    let owner = PathBuf::from(&meta.repo_root);
                    let transcript = store.resume(&owner, &meta.id).ok();
                    let turns = transcript
                        .as_ref()
                        .map(|t| t.user_turn_count())
                        .unwrap_or(0);
                    let title = transcript
                        .as_ref()
                        .and_then(|t| t.display_title())
                        .map(|s| truncate(s.trim(), 80))
                        .unwrap_or_default();
                    (meta, turns, title)
                })
                .collect();

            if crate::json_mode() {
                let out: Vec<Value> = rows
                    .iter()
                    .map(|(meta, turns, title)| {
                        json!({
                            "id": meta.id,
                            "created_at": meta.created_at.to_rfc3339(),
                            "model": meta.model,
                            "turns": turns,
                            "title": title,
                        })
                    })
                    .collect();
                println!("{}", json!(out));
            } else if rows.is_empty() {
                println!("no sessions for this repo yet");
            } else {
                // Ids run from 12 characters to a 43-character UUID, so without
                // a shared width nothing lines up. They are padded rather than
                // shortened because `sessions show` needs the whole id.
                let id_width = rows
                    .iter()
                    .map(|(m, ..)| m.id.chars().count())
                    .max()
                    .unwrap_or(0);
                // Which project a session belongs to only matters when the list
                // spans more than this folder.
                let project_width = if args.all {
                    rows.iter()
                        .map(|(m, ..)| project_name(&m.repo_root).chars().count())
                        .max()
                        .unwrap_or(0)
                } else {
                    0
                };
                let room = terminal_width().saturating_sub(id_width + project_width + 16 + 10 + 8);
                for (meta, turns, title) in rows {
                    let when = meta.created_at.format("%Y-%m-%d %H:%M");
                    let title = match one_line(&title) {
                        text if text.is_empty() => "(empty)".into(),
                        text => truncate(&text, room.max(20)),
                    };
                    let turns = format!("{turns} {}", if turns == 1 { "turn" } else { "turns" });
                    let project = if args.all {
                        format!("{:<project_width$}  ", project_name(&meta.repo_root))
                    } else {
                        String::new()
                    };
                    println!(
                        "{:<id_width$}  {project}{when}  {turns:>8}  {title}",
                        meta.id,
                        id_width = id_width
                    );
                }
            }
        }
        SessionsCmd::Show { id } => {
            let owner = owner_of(&store, &repo_root, &id)?;
            let transcript = store
                .resume(&owner, &id)
                .with_context(|| format!("could not read session {id:?}"))?;
            if crate::json_mode() {
                let events: Vec<Value> = transcript
                    .events
                    .iter()
                    .map(|e| serde_json::to_value(e).unwrap_or(Value::Null))
                    .collect();
                println!("{}", json!({ "id": transcript.meta.id, "events": events }));
            } else {
                print_transcript(&transcript);
            }
        }
        SessionsCmd::Prune { keep, older_than } => {
            let metas = store.list_sessions(&repo_root)?;
            let cutoff = older_than.map(|days| chrono::Utc::now() - chrono::Duration::days(days));
            let mut kept = 0;
            let mut deleted = Vec::new();
            // Newest first, so `--keep` counts from the most recent.
            let mut metas = metas;
            metas.sort_by_key(|m| std::cmp::Reverse(m.created_at));
            for meta in metas {
                let turns = store
                    .resume(&repo_root, &meta.id)
                    .map(|t| t.user_turn_count())
                    .unwrap_or(0);
                let stale = cutoff.is_some_and(|c| meta.created_at < c);
                let over_limit = keep.is_some_and(|k| kept >= k);
                if turns == 0 || stale || over_limit {
                    if store.delete_session(&repo_root, &meta.id)? {
                        deleted.push(meta.id);
                    }
                    continue;
                }
                kept += 1;
            }
            if crate::json_mode() {
                println!(
                    "{}",
                    json!({ "ok": true, "deleted": deleted, "kept": kept })
                );
            } else {
                println!("deleted {} session(s), kept {kept}", deleted.len());
            }
        }
        SessionsCmd::Delete { id } => {
            let owner = owner_of(&store, &repo_root, &id)?;
            if !store.delete_session(&owner, &id)? {
                anyhow::bail!("no session {id:?}");
            }
            if crate::json_mode() {
                println!("{}", json!({ "ok": true, "id": id }));
            } else {
                println!("deleted session {id}");
            }
        }
    }
    Ok(())
}

/// The folder a session was started in, as a column: the last path component,
/// which is what tells two checkouts apart without spending the width.
fn project_name(repo_root: &str) -> String {
    Path::new(repo_root)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| repo_root.to_string())
}

/// The checkout that owns a session: this one when the id is local, otherwise
/// whichever project holds it. Sessions are filed per project, so an id from a
/// sibling folder is invisible to a plain lookup however recent it is, and
/// answering "no such session" there would be a lie.
fn owner_of(store: &aster_persist::Store, repo_root: &Path, id: &str) -> Result<PathBuf> {
    if store.resume(repo_root, id).is_ok() {
        return Ok(repo_root.to_path_buf());
    }
    store
        .list_all_sessions()?
        .into_iter()
        .find(|meta| meta.id == id)
        .map(|meta| PathBuf::from(meta.repo_root))
        .with_context(|| format!("no session {id:?} in any project"))
}

fn print_transcript(transcript: &aster_persist::SessionTranscript) {
    for event in &transcript.events {
        match event {
            TranscriptEvent::Session(m) => {
                println!(
                    "# session {} · {}",
                    m.id,
                    m.created_at.format("%Y-%m-%d %H:%M")
                );
            }
            TranscriptEvent::Title(t) => println!("\n[titled] {}", t.title),
            TranscriptEvent::Summary(s) => println!("\n[summary]\n{}", s.content),
            TranscriptEvent::Eviction(e) => {
                println!(
                    "\n[evicted] {} at #{} ({} chars): {}",
                    e.role, e.index, e.chars, e.reason
                )
            }
            TranscriptEvent::Message(msg) => {
                if !msg.tool_calls.is_empty() {
                    let names: Vec<&str> = msg
                        .tool_calls
                        .iter()
                        .map(|c| c.function.name.as_str())
                        .collect();
                    println!("\n{}: [calls {}]", msg.role, names.join(", "));
                }
                if let Some(content) = &msg.content {
                    let label = if msg.tool_call_id.is_some() {
                        "tool"
                    } else {
                        msg.role.as_str()
                    };
                    println!("\n{label}: {}", truncate(content.trim(), 2000));
                }
                if !msg.annotations.is_empty() {
                    println!("  sources:");
                    for a in &msg.annotations {
                        let label = a
                            .url_citation
                            .title
                            .as_deref()
                            .unwrap_or(&a.url_citation.url);
                        println!("    {label} — {url}", url = a.url_citation.url);
                    }
                }
            }
        }
    }
}

#[derive(Args)]
pub struct MemoryArgs {
    #[command(subcommand)]
    cmd: Option<MemoryCmd>,
}

#[derive(Subcommand)]
enum MemoryCmd {
    /// List stored memory (the default).
    List,
    /// Save a durable fact. Without --title it appends to project memory.
    Add {
        text: String,
        #[arg(long, value_name = "NAME")]
        title: Option<String>,
    },
    /// Delete a memory block by name, so a wrong fact can be taken back.
    Remove { name: String },
    /// Print a memory block in full.
    Show { name: String },
}

pub fn run_memory(args: MemoryArgs) -> Result<()> {
    let memory = crate::persist::store()?.memory();

    match args.cmd.unwrap_or(MemoryCmd::List) {
        MemoryCmd::List => {
            let blocks = memory.list()?;
            if crate::json_mode() {
                let out = json!({
                    "dir": memory.dir().display().to_string(),
                    "blocks": blocks.iter().map(|b| json!({
                        "name": b.name,
                        "description": b.description,
                    })).collect::<Vec<_>>(),
                });
                println!("{out}");
            } else {
                let context = memory.load_context()?;
                if context.trim().is_empty() {
                    println!("no memory stored yet");
                    return Ok(());
                }
                println!("memory dir: {}", memory.dir().display());
                if blocks.is_empty() {
                    println!("(project memory only, no blocks)");
                }
                for block in blocks {
                    if block.description.is_empty() {
                        println!("  {}", block.name);
                    } else {
                        println!("  {}  —  {}", block.name, block.description);
                    }
                }
            }
        }
        MemoryCmd::Remove { name } => {
            if !memory.forget(&name)? {
                anyhow::bail!("no memory block named {name:?}; `aster memory list` shows them all");
            }
            if crate::json_mode() {
                println!("{}", json!({ "ok": true, "removed": name }));
            } else {
                println!("removed {name}");
            }
        }
        MemoryCmd::Show { name } => {
            let body = memory.read_block(&name)?;
            if crate::json_mode() {
                println!("{}", json!({ "name": name, "body": body }));
            } else {
                println!("{body}");
            }
        }
        MemoryCmd::Add { text, title } => {
            let result = match &title {
                Some(title) => {
                    let path = memory.remember(title, &text, &text)?;
                    ("block", Some(path))
                }
                None => {
                    memory.append_project(&text)?;
                    ("project", None)
                }
            };
            if crate::json_mode() {
                println!(
                    "{}",
                    json!({
                        "ok": true,
                        "kind": result.0,
                        "path": result.1.map(|p| p.display().to_string()),
                    })
                );
            } else {
                match result.1 {
                    Some(path) => println!("saved block {:?} to {}", title, path.display()),
                    None => println!("appended to project memory"),
                }
            }
        }
    }
    Ok(())
}

/// Flatten a title to one line. A preview holding a newline otherwise wraps and
/// breaks every column below it.
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Usable columns, falling back to a sane width when stdout is not a terminal.
fn terminal_width() -> usize {
    crossterm::terminal::size()
        .map(|(cols, _)| cols as usize)
        .unwrap_or(100)
        .max(60)
}

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let cut: String = text.chars().take(max).collect();
    format!("{cut}…")
}
