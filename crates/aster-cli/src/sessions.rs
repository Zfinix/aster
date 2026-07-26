use std::env;

use anyhow::{Context, Result};
use aster_persist::TranscriptEvent;
use clap::{Args, Subcommand};
use serde_json::{Value, json};

#[derive(Args)]
pub struct SessionsArgs {
    #[command(subcommand)]
    cmd: Option<SessionsCmd>,
    /// Emit JSON instead of text (for editors and the desktop app).
    #[arg(long, global = true)]
    json: bool,
}

#[derive(Subcommand)]
enum SessionsCmd {
    /// List saved sessions for this repo (the default).
    List,
    /// Print a session's full transcript by id.
    Show { id: String },
}

pub fn run_sessions(args: SessionsArgs) -> Result<()> {
    let repo_root = env::current_dir().context("could not determine the current directory")?;
    let store = crate::persist::store()?;

    match args.cmd.unwrap_or(SessionsCmd::List) {
        SessionsCmd::List => {
            let metas = store.list_sessions(&repo_root)?;
            let rows: Vec<(_, usize, String)> = metas
                .into_iter()
                .map(|meta| {
                    let transcript = store.resume(&repo_root, &meta.id).ok();
                    let turns = transcript
                        .as_ref()
                        .map(|t| t.user_turn_count())
                        .unwrap_or(0);
                    let title = transcript
                        .as_ref()
                        .and_then(|t| t.first_user_text())
                        .map(|s| truncate(s.trim(), 80))
                        .unwrap_or_default();
                    (meta, turns, title)
                })
                .collect();

            if args.json {
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
                for (meta, turns, title) in rows {
                    let when = meta.created_at.format("%Y-%m-%d %H:%M");
                    let title = if title.is_empty() {
                        "(empty)".into()
                    } else {
                        title
                    };
                    println!("{}  {when}  {turns:>3} turns  {title}", meta.id);
                }
            }
        }
        SessionsCmd::Show { id } => {
            let transcript = store
                .resume(&repo_root, &id)
                .with_context(|| format!("no session {id:?} for this repo"))?;
            if args.json {
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
    }
    Ok(())
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
            TranscriptEvent::Summary(s) => println!("\n[summary]\n{}", s.content),
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
            }
        }
    }
}

#[derive(Args)]
pub struct MemoryArgs {
    #[command(subcommand)]
    cmd: Option<MemoryCmd>,
    /// Emit JSON instead of text (for editors and the desktop app).
    #[arg(long, global = true)]
    json: bool,
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
}

pub fn run_memory(args: MemoryArgs) -> Result<()> {
    let memory = crate::persist::store()?.memory();

    match args.cmd.unwrap_or(MemoryCmd::List) {
        MemoryCmd::List => {
            let blocks = memory.list()?;
            if args.json {
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
            if args.json {
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

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let cut: String = text.chars().take(max).collect();
    format!("{cut}…")
}
