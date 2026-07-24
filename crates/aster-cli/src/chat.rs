//! `aster chat`: a conversational turn with the review-agent persona, plus an
//! agentic tool loop (read, list, search, and optionally edit files in the
//! repo). Provider resolution is identical to `aster review` (cwd `.env` via
//! dotenvy, env, `aster.yaml`, defaults), so chat works anywhere a review
//! works. The desktop app drives this with `--messages-json - --json`.

use std::io::{IsTerminal, Read};
use std::path::{Path, PathBuf};
use std::{env, fs, io};

use anyhow::{Context, Result, bail};
use aster_ai::{AiClient, ChatMessage};
use clap::Args;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::edits::{self, EditBlock};

const AGENT_SYSTEM_PROMPT: &str = include_str!("../prompts/aster-agent.md");
const CHAT_TEMPERATURE: f32 = 0.4;
/// Hard stop for the tool loop, so a confused model cannot spin forever.
const MAX_TOOL_ROUNDS: usize = 12;
/// Caps on tool output, so one fat file cannot blow the context.
const MAX_TOOL_RESULT_CHARS: usize = 24_000;
const MAX_SEARCH_HITS: usize = 80;
const MAX_LIST_ENTRIES: usize = 200;

/// Appended to the persona when the tool loop is active.
const TOOLS_PROMPT: &str = "\n\n## Tools\n\n\
You can inspect the repository with `read_file`, `list_files`, and \
`search_files`, and change it with `edit_file` when it is available. Ground \
every claim about the code in what you actually read. Only edit files when the \
user asked for a change; keep edits minimal and in the file's existing style. \
After editing, state plainly which files you changed and what the change does. \
If `edit_file` is unavailable, say so and describe the change instead.";

#[derive(Args)]
pub struct ChatArgs {
    /// One-shot question, e.g. `aster chat "why is finding 2 critical?"`.
    #[arg(value_name = "PROMPT", conflicts_with = "messages_json")]
    prompt: Option<String>,

    /// Read a JSON array of {"role","content"} messages from PATH, or `-` for
    /// stdin. Roles: "user" | "assistant" | "system". For editors and UIs.
    #[arg(long, value_name = "PATH")]
    messages_json: Option<String>,

    /// Model override (else ASTER_MODEL, aster.yaml, default).
    #[arg(long, value_name = "MODEL")]
    model: Option<String>,

    /// Let the agent edit repo files via its edit_file tool.
    #[arg(long)]
    allow_edits: bool,

    /// Plain single-shot chat: no read/search/edit tools.
    #[arg(long)]
    no_tools: bool,

    /// Open an interactive chat TUI in the terminal. The optional PROMPT seeds
    /// the first question.
    #[arg(long, conflicts_with_all = ["messages_json", "json", "no_tools"])]
    tui: bool,

    /// Emit {"reply": "...", "edits": [...], "usage": {...}} as one JSON
    /// object on stdout.
    #[arg(long)]
    json: bool,
}

#[derive(Deserialize)]
struct WireMessage {
    role: String,
    content: String,
}

pub async fn run(args: ChatArgs) -> Result<()> {
    let repo_root = env::current_dir().context("could not determine the current directory")?;
    let settings = crate::settings::Settings::load(Some(&repo_root))?;

    let llm = crate::provider::resolve(&settings.review, args.model.as_deref())?;
    let client = AiClient::new(llm.base_url, llm.api_key, llm.model);

    // Interactive TUI: only when stdout is a real terminal, so piped/CI runs
    // still get the one-shot behavior.
    if args.tui && io::stdout().is_terminal() {
        let seed = args.prompt.clone();
        return crate::tui::run_chat(client, repo_root, args.allow_edits, seed).await;
    }

    let history = read_history(&args)?;
    let mut edited: Vec<String> = Vec::new();
    let reply = if args.no_tools {
        let mut messages = vec![ChatMessage {
            role: "system".into(),
            content: AGENT_SYSTEM_PROMPT.into(),
        }];
        messages.extend(history);
        client
            .complete_messages(&messages, CHAT_TEMPERATURE)
            .await?
    } else {
        agent_loop(&client, &repo_root, &history, args.allow_edits, &mut edited).await?
    };

    if args.json {
        let u = client.usage_snapshot();
        let out = json!({
            "reply": reply,
            "edits": edited,
            "usage": {
                "prompt_tokens": u.prompt_tokens,
                "completion_tokens": u.completion_tokens,
                "total_tokens": u.total_tokens,
                "requests": u.requests,
                "estimated_cost_usd": u.estimated_cost_usd,
                "estimated": u.estimated,
            },
        });
        println!("{out}");
    } else {
        println!("{reply}");
        for path in &edited {
            eprintln!("  ✎ edited {path}");
        }
        crate::review::print_usage(client.usage_snapshot());
    }
    Ok(())
}

/// The conversation to send, minus the system prompt: either the positional
/// prompt as a single user message, or the full `--messages-json` history.
fn read_history(args: &ChatArgs) -> Result<Vec<ChatMessage>> {
    if let Some(path) = args.messages_json.as_deref() {
        let raw = if path == "-" {
            let mut buf = String::new();
            io::stdin()
                .read_to_string(&mut buf)
                .context("reading messages from stdin")?;
            buf
        } else {
            fs::read_to_string(path).with_context(|| format!("reading {path}"))?
        };
        let wire: Vec<WireMessage> = serde_json::from_str(&raw)
            .context("parsing --messages-json: expected a JSON array of {role, content}")?;
        if wire.is_empty() {
            bail!("nothing to ask; --messages-json was an empty array");
        }
        return wire
            .into_iter()
            .map(|m| match m.role.as_str() {
                "user" | "assistant" | "system" => Ok(ChatMessage {
                    role: m.role,
                    content: m.content,
                }),
                other => bail!("unsupported role {other:?} in --messages-json"),
            })
            .collect();
    }

    match args.prompt.as_deref().map(str::trim) {
        Some(prompt) if !prompt.is_empty() => Ok(vec![ChatMessage {
            role: "user".into(),
            content: prompt.into(),
        }]),
        _ => bail!("nothing to ask; pass a prompt (aster chat \"...\") or --messages-json"),
    }
}

/// One full agentic turn, owning its inputs so it can be spawned as a task
/// (the chat TUI drives it this way). Returns the reply and the files edited.
pub(crate) async fn agent_turn(
    client: AiClient,
    repo_root: PathBuf,
    history: Vec<ChatMessage>,
    allow_edits: bool,
) -> Result<(String, Vec<String>)> {
    let mut edited = Vec::new();
    let reply = agent_loop(&client, &repo_root, &history, allow_edits, &mut edited).await?;
    Ok((reply, edited))
}

/// Drive the model's tool calls until it answers in plain text (or the round
/// cap trips). Tool failures are returned to the model as tool results, so it
/// can retry with more context instead of dying.
async fn agent_loop(
    client: &AiClient,
    repo_root: &Path,
    history: &[ChatMessage],
    allow_edits: bool,
    edited: &mut Vec<String>,
) -> Result<String> {
    let mut wire: Vec<Value> = vec![json!({
        "role": "system",
        "content": format!("{AGENT_SYSTEM_PROMPT}{TOOLS_PROMPT}"),
    })];
    for m in history {
        wire.push(serde_json::to_value(m)?);
    }
    let tools = tool_defs(allow_edits);

    for round in 0..MAX_TOOL_ROUNDS {
        let msg = match client
            .complete_tools(wire.clone(), tools.clone(), CHAT_TEMPERATURE)
            .await
        {
            Ok(msg) => msg,
            // Some models reject tool definitions outright; degrade to plain
            // chat instead of failing the turn. Only safe on the first round,
            // before any tool turns entered the history.
            Err(e) if round == 0 && is_tool_unsupported(&e) => {
                tracing::debug!("model rejected tools; falling back to plain chat: {e:#}");
                let mut messages = vec![ChatMessage {
                    role: "system".into(),
                    content: AGENT_SYSTEM_PROMPT.into(),
                }];
                messages.extend(history.iter().cloned());
                return client.complete_messages(&messages, CHAT_TEMPERATURE).await;
            }
            Err(e) => return Err(e),
        };

        if msg.tool_calls.is_empty() {
            return msg
                .content
                .filter(|c| !c.trim().is_empty())
                .context("model returned an empty reply");
        }

        wire.push(json!({
            "role": "assistant",
            "content": msg.content,
            "tool_calls": msg.tool_calls,
        }));
        for call in &msg.tool_calls {
            let result = exec_tool(
                repo_root,
                allow_edits,
                &call.function.name,
                &call.function.arguments,
                edited,
            );
            tracing::debug!(tool = %call.function.name, "tool call executed");
            wire.push(json!({
                "role": "tool",
                "tool_call_id": call.id,
                "content": truncate(&result, MAX_TOOL_RESULT_CHARS),
            }));
        }
    }

    // Round cap tripped: force a final plain answer out of what was gathered.
    wire.push(json!({
        "role": "user",
        "content": "Stop using tools and answer now with what you have.",
    }));
    let msg = client
        .complete_tools(wire, Vec::new(), CHAT_TEMPERATURE)
        .await?;
    msg.content
        .filter(|c| !c.trim().is_empty())
        .context("model returned an empty reply after the tool-round limit")
}

fn is_tool_unsupported(e: &anyhow::Error) -> bool {
    let text = format!("{e:#}").to_lowercase();
    text.contains("tool") || text.contains("function")
}

fn tool_defs(allow_edits: bool) -> Vec<Value> {
    let mut tools = vec![
        json!({
            "type": "function",
            "function": {
                "name": "read_file",
                "description": "Read a file from the repository, with line numbers. Optionally a line range.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Repo-relative file path" },
                        "start_line": { "type": "integer", "description": "First line, 1-based (optional)" },
                        "end_line": { "type": "integer", "description": "Last line, inclusive (optional)" }
                    },
                    "required": ["path"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "list_files",
                "description": "List the entries of a repository directory. Directories end with '/'.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "dir": { "type": "string", "description": "Repo-relative directory; omit for the root" }
                    }
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "search_files",
                "description": "Case-insensitive substring search across repository files. Returns path:line: text.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "Text to search for" },
                        "dir": { "type": "string", "description": "Repo-relative directory to search under (optional)" }
                    },
                    "required": ["query"]
                }
            }
        }),
    ];
    if allow_edits {
        tools.push(json!({
            "type": "function",
            "function": {
                "name": "edit_file",
                "description": "Replace text in a repository file. `search` must be copied verbatim from the file and match exactly once; include surrounding lines to disambiguate.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Repo-relative file path" },
                        "search": { "type": "string", "description": "Exact existing text to replace" },
                        "replace": { "type": "string", "description": "Replacement text" }
                    },
                    "required": ["path", "search", "replace"]
                }
            }
        }));
    }
    tools
}

/// Execute one tool call. Failures come back as plain text so the model can
/// react (retry with more context, pick another file) instead of dying.
fn exec_tool(
    repo_root: &Path,
    allow_edits: bool,
    name: &str,
    arguments: &str,
    edited: &mut Vec<String>,
) -> String {
    let args: Value = match serde_json::from_str(arguments) {
        Ok(v) => v,
        Err(e) => return format!("error: tool arguments were not valid JSON: {e}"),
    };
    let str_arg = |key: &str| args[key].as_str().map(str::to_string);

    let result = match name {
        "read_file" => str_arg("path")
            .context("read_file needs a `path`")
            .and_then(|path| {
                read_numbered(
                    repo_root,
                    &path,
                    args["start_line"].as_u64().map(|n| n as usize),
                    args["end_line"].as_u64().map(|n| n as usize),
                )
            }),
        "list_files" => list_files(repo_root, str_arg("dir").as_deref().unwrap_or("")),
        "search_files" => str_arg("query")
            .context("search_files needs a `query`")
            .and_then(|q| search_files(repo_root, &q, str_arg("dir").as_deref().unwrap_or(""))),
        "edit_file" if !allow_edits => Err(anyhow::anyhow!(
            "editing is disabled for this chat; tell the user to enable Allow edits"
        )),
        "edit_file" => edit_file(repo_root, &args, edited),
        other => Err(anyhow::anyhow!("unknown tool: {other}")),
    };
    result.unwrap_or_else(|e| format!("error: {e:#}"))
}

fn read_numbered(
    repo_root: &Path,
    path: &str,
    start: Option<usize>,
    end: Option<usize>,
) -> Result<String> {
    let (_, content) = edits::read_repo_file(repo_root, path)?;
    let lines: Vec<&str> = content.lines().collect();
    let from = start.unwrap_or(1).max(1) - 1;
    let to = end.unwrap_or(lines.len()).min(lines.len());
    if from >= to {
        bail!("empty range: the file has {} lines", lines.len());
    }
    let body = lines[from..to]
        .iter()
        .enumerate()
        .map(|(i, l)| format!("{:>5} | {l}", from + i + 1))
        .collect::<Vec<_>>()
        .join("\n");
    Ok(body)
}

/// Directories never worth walking: mirrors the review path filter's defaults.
const SKIP_DIRS: &[&str] = &[
    ".git",
    "target",
    "node_modules",
    "dist",
    "build",
    "out",
    "vendor",
    ".hg",
    ".svn",
];

fn list_files(repo_root: &Path, dir: &str) -> Result<String> {
    let base = if dir.is_empty() {
        repo_root.to_path_buf()
    } else {
        edits::resolve_in_repo(repo_root, dir)?
    };
    let mut entries: Vec<String> = fs::read_dir(&base)
        .with_context(|| format!("listing {}", base.display()))?
        .filter_map(|e| e.ok())
        .map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if e.path().is_dir() {
                format!("{name}/")
            } else {
                name
            }
        })
        .collect();
    entries.sort();
    entries.truncate(MAX_LIST_ENTRIES);
    Ok(entries.join("\n"))
}

fn search_files(repo_root: &Path, query: &str, dir: &str) -> Result<String> {
    if query.trim().is_empty() {
        bail!("empty search query");
    }
    let base = if dir.is_empty() {
        repo_root.to_path_buf()
    } else {
        edits::resolve_in_repo(repo_root, dir)?
    };
    let needle = query.to_lowercase();
    let mut hits = Vec::new();
    let mut stack = vec![base];
    while let Some(current) = stack.pop() {
        if hits.len() >= MAX_SEARCH_HITS {
            break;
        }
        let Ok(entries) = fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if path.is_dir() {
                if !name.starts_with('.') && !SKIP_DIRS.contains(&name.as_str()) {
                    stack.push(path);
                }
                continue;
            }
            let Ok(content) = fs::read_to_string(&path) else {
                continue;
            };
            let rel = path
                .strip_prefix(repo_root)
                .unwrap_or(&path)
                .to_string_lossy()
                .into_owned();
            for (no, line) in content.lines().enumerate() {
                if line.to_lowercase().contains(&needle) {
                    hits.push(format!("{rel}:{}: {}", no + 1, truncate(line.trim(), 240)));
                    if hits.len() >= MAX_SEARCH_HITS {
                        break;
                    }
                }
            }
            if hits.len() >= MAX_SEARCH_HITS {
                break;
            }
        }
    }
    if hits.is_empty() {
        return Ok("no matches".into());
    }
    Ok(hits.join("\n"))
}

fn edit_file(repo_root: &Path, args: &Value, edited: &mut Vec<String>) -> Result<String> {
    let path = args["path"].as_str().context("edit_file needs a `path`")?;
    let block = EditBlock {
        search: args["search"]
            .as_str()
            .context("edit_file needs `search`")?
            .to_string(),
        replace: args["replace"]
            .as_str()
            .context("edit_file needs `replace`")?
            .to_string(),
    };
    let (resolved, content) = edits::read_repo_file(repo_root, path)?;
    let updated = edits::apply_block(&content, &block)?;
    fs::write(&resolved, &updated).with_context(|| format!("writing {}", resolved.display()))?;
    if !edited.iter().any(|p| p == path) {
        edited.push(path.to_string());
    }
    Ok(format!("edited {path}:\n{}", edits::preview(&block)))
}

fn truncate(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let mut cut = max;
    while !text.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}\n... [truncated]", &text[..cut])
}
