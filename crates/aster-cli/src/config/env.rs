//! `aster env`: every environment variable Aster reads that is neither a key
//! (owned by `aster key`) nor an `aster.yaml` alias (owned by `aster config`).
//! Writes land in the same `.env` files `aster key` owns.

use std::path::Path;
use std::{env, fs};

use anyhow::{Context, Result, bail};
use aster_ai::keys::env_non_empty;
use clap::{Args, Subcommand};
use cliclack::input;
use serde_json::json;

use super::key::{self, Source};
use crate::term::{BOLD, DIM, paint};
use crate::util::or_cancel;

#[derive(Args)]
pub struct EnvArgs {
    #[command(subcommand)]
    command: Option<EnvCmd>,
}

#[derive(Subcommand)]
enum EnvCmd {
    /// Every variable Aster reads, whether it is set, and which layer supplies it.
    List,
    /// Store a variable in `.env`, asking for the value when it is not given.
    Set(SetArgs),
    /// Print one variable's live value, for scripts and the editors' reveal button.
    Get(GetArgs),
    /// Take a variable back out of `.env`.
    Unset(UnsetArgs),
}

#[derive(Args)]
struct SetArgs {
    /// The variable, e.g. `ASTER_MAX_TOOL_ROUNDS`. `aster env list` spells them all.
    #[arg(value_name = "VAR")]
    var: String,

    /// The value. Left out, it is asked for.
    #[arg(value_name = "VALUE")]
    value: Option<String>,

    /// Read the value from stdin instead, so it never appears in the process
    /// list or an error message.
    #[arg(long, conflicts_with = "value")]
    stdin: bool,

    /// Write this repo's `.env` instead of `~/.aster/.env`, and git-ignore it.
    #[arg(long)]
    local: bool,
}

#[derive(Args)]
struct GetArgs {
    /// The variable to read, e.g. `ASTER_MAX_TOOL_ROUNDS`.
    #[arg(value_name = "VAR")]
    var: String,
}

#[derive(Args)]
struct UnsetArgs {
    /// The variable to clear.
    #[arg(value_name = "VAR")]
    var: String,

    /// Only clear `~/.aster/.env`, leaving a repo `.env` that sets it.
    #[arg(long, conflicts_with = "local")]
    global: bool,

    /// Only clear this repo's `.env`, leaving the global one that sets it.
    #[arg(long)]
    local: bool,
}

struct Var {
    var: &'static str,
    group: &'static str,
    /// What the settings UI renders: `number`, `text`, `bool`, or `json`.
    kind: &'static str,
    secret: bool,
    help: &'static str,
}

/// The variables Aster reads at runtime. A var that stops being read should
/// leave this table the same commit that drops the read.
const VARS: &[Var] = &[
    Var { var: "ASTER_MAX_TOOL_ROUNDS", group: "Turns and limits", kind: "number", secret: false, help: "Tool rounds the agent may spend on one try" },
    Var { var: "ASTER_COMMAND_TIMEOUT", group: "Turns and limits", kind: "number", secret: false, help: "Seconds before a running command is stopped" },
    Var { var: "ASTER_COMPACT_BUDGET", group: "Turns and limits", kind: "number", secret: false, help: "Transcript size in characters before it is folded into a summary" },
    Var { var: "ASTER_GOAL_MAX_TURNS", group: "Turns and limits", kind: "number", secret: false, help: "Tries a goal check may run" },
    Var { var: "ASTER_AGENT_MAX_CONCURRENT", group: "Turns and limits", kind: "number", secret: false, help: "Sub-agents running at the same time" },
    Var { var: "ASTER_AGENT_MAX_PER_TURN", group: "Turns and limits", kind: "number", secret: false, help: "Sub-agent fan-out allowed per turn" },
    Var { var: "ASTER_AGENT_TIMEOUT", group: "Turns and limits", kind: "number", secret: false, help: "Seconds before a sub-agent is stopped" },
    Var { var: "ASTER_TIMEOUT_SECS", group: "Turns and limits", kind: "number", secret: false, help: "Seconds before a model request is abandoned" },
    Var { var: "ASTER_MAX_RETRIES", group: "Turns and limits", kind: "number", secret: false, help: "Retries on a failed model request" },
    Var { var: "ASTER_DEADLINE_SECS", group: "Turns and limits", kind: "number", secret: false, help: "Seconds a whole turn may take" },
    Var { var: "ASTER_ROUTER_TIER", group: "Models", kind: "text", secret: false, help: "Automatic model tier when the model is auto" },
    Var { var: "ASTER_HYPOTHESIS_MODEL", group: "Models", kind: "text", secret: false, help: "Model that drafts review findings" },
    Var { var: "ASTER_VERIFY_MODEL", group: "Models", kind: "text", secret: false, help: "Model that double-checks review findings" },
    Var { var: "ASTER_COLLECTOR_MODEL", group: "Models", kind: "text", secret: false, help: "Model that collects sub-agent reports" },
    Var { var: "ASTER_ANALYZERS", group: "Code review", kind: "text", secret: false, help: "Runtime analyzers for review, comma separated: semgrep, ast-grep. Empty means the model alone" },
    Var { var: "ASTER_ASTGREP_RULES", group: "Code review", kind: "text", secret: false, help: "Path to extra ast-grep rules" },
    Var { var: "ASTER_REPO", group: "Code review", kind: "text", secret: false, help: "Repo name shown in review reports when none is detected" },
    Var { var: "ASTER_MCP_EXTRA", group: "MCP", kind: "json", secret: false, help: "Extra MCP servers as JSON, merged over aster.yaml" },
    Var { var: "ASTER_PROMPT_CACHE", group: "Toggles", kind: "text", secret: false, help: "Set to off, 0, or false to disable prompt caching headers" },
    Var { var: "ASTER_NO_BROWSER", group: "Toggles", kind: "bool", secret: false, help: "Set to anything to stop previews from opening a browser" },
    Var { var: "ASTER_NO_UPDATE_CHECK", group: "Toggles", kind: "bool", secret: false, help: "Set to anything to skip the update and announcement checks" },
    Var { var: "ASTER_WEBMCP_CDP_URL", group: "Toggles", kind: "text", secret: false, help: "Chrome DevTools endpoint the web MCP tools connect to" },
    Var { var: "ASTER_TELEGRAM_TOKEN", group: "Remote", kind: "text", secret: true, help: "Bot token for the Telegram remote" },
    Var { var: "ASTER_REMOTE_USERS", group: "Remote", kind: "text", secret: false, help: "Telegram user ids allowed to talk to Aster, comma separated" },
];

pub struct Row {
    entry: &'static Var,
    source: Source,
    /// The live value, for non-secrets only. Secrets carry a masked tail.
    value: Option<String>,
    masked: Option<String>,
}

impl Row {
    fn set(&self) -> bool {
        self.source != Source::Unset
    }
}

fn rows(repo_root: &Path) -> Vec<Row> {
    VARS.iter()
        .map(|entry| Row {
            source: key::source(entry.var, repo_root),
            value: match entry.secret {
                true => None,
                false => env_non_empty(entry.var),
            },
            masked: match entry.secret {
                true => key::masked(entry.var),
                false => None,
            },
            entry,
        })
        .collect()
}

pub fn run(args: EnvArgs) -> Result<()> {
    let repo_root = env::current_dir().context("could not determine the current directory")?;
    match args.command {
        None => list(&repo_root),
        Some(EnvCmd::List) => list(&repo_root),
        Some(EnvCmd::Set(a)) => set(&repo_root, &a.var, a.value.as_deref(), a.local, a.stdin),
        Some(EnvCmd::Get(a)) => get(&repo_root, &a.var),
        Some(EnvCmd::Unset(a)) => unset(&repo_root, &a.var, a.global, a.local),
    }
}

fn list(repo_root: &Path) -> Result<()> {
    let all = rows(repo_root);

    if crate::json_mode() {
        let vars: Vec<_> = all
            .iter()
            .map(|r| {
                json!({
                    "var": r.entry.var,
                    "group": r.entry.group,
                    "help": r.entry.help,
                    "kind": r.entry.kind,
                    "secret": r.entry.secret,
                    "set": r.set(),
                    "source": r.source.as_str(),
                    "masked": r.masked,
                    "value": r.value,
                })
            })
            .collect();
        println!("{}", json!({ "ok": true, "vars": vars }));
        return Ok(());
    }

    let mut groups: Vec<&str> = VARS.iter().map(|v| v.group).collect();
    groups.dedup();
    for group in groups {
        let group_rows: Vec<&Row> = all.iter().filter(|r| r.entry.group == group).collect();
        println!("{}", paint(BOLD, group));
        let width = group_rows.iter().map(|r| r.entry.var.len()).max().unwrap_or(0);
        for row in group_rows {
            let note = match (row.source, (&row.value, &row.masked)) {
                (Source::Unset, _) => "not set".to_string(),
                (_, (Some(value), _)) => format!("{value} · {}", row.source.label()),
                (_, (_, Some(tail))) => format!("{tail} · {}", row.source.label()),
                (other, _) => format!("set · {}", other.label()),
            };
            println!("  {:<width$}  {}", row.entry.var, paint(DIM, &note));
        }
        println!();
    }
    Ok(())
}

fn set(
    repo_root: &Path,
    var: &str,
    value: Option<&str>,
    local: bool,
    stdin: bool,
) -> Result<()> {
    let var = key::normalize(var)?;
    ensure_aster(&var)?;
    let secret = VARS.iter().find(|v| v.var == var).is_some_and(|v| v.secret);
    let value = match (stdin, value) {
        (true, _) => {
            let mut line = String::new();
            std::io::stdin()
                .read_line(&mut line)
                .context("could not read the value from stdin")?;
            line.trim().to_string()
        }
        (false, Some(v)) => v.trim().to_string(),
        (false, None) => ask(&var, secret)?,
    };
    if value.is_empty() {
        bail!("no value given. Pass it as `aster env set {var} <value>`, or leave it off to be asked for it");
    }
    if value.contains(['\n', '\r']) {
        bail!("that value contains a line break, so it cannot be stored in .env. Check what you pasted");
    }

    let before = key::source(&var, repo_root);
    let path = match local {
        true => key::local_env(repo_root).context("this directory has no repo to write .env into")?,
        false => key::global_env().context("could not determine your home directory")?,
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    let replaced = key::file_value(Some(&path), &var).is_some();
    crate::init::set_env_key(&path, &var, &value)
        .with_context(|| format!("writing {}", path.display()))?;
    if local && let Some(dir) = path.parent() {
        crate::init::ensure_gitignored(dir, ".env")?;
    }

    if crate::json_mode() {
        println!(
            "{}",
            json!({ "ok": true, "var": var, "path": path.display().to_string(), "replaced": replaced })
        );
        return Ok(());
    }

    let verb = match replaced {
        true => "Replaced",
        false => "Stored",
    };
    println!("{verb} {var} in {}", key::display(&path));
    if let Some(note) = shadow_note(&var, before, local) {
        println!("{}", paint(DIM, &note));
    }
    if !VARS.iter().any(|v| v.var == var) {
        println!(
            "{}",
            paint(
                DIM,
                &format!("Nothing in Aster reads {var}; `aster env list` shows the ones that are read."),
            )
        );
    }
    Ok(())
}

fn get(repo_root: &Path, var: &str) -> Result<()> {
    let var = key::normalize(var)?;
    let source = key::source(&var, repo_root);
    let value = env_non_empty(&var);
    if crate::json_mode() {
        println!(
            "{}",
            json!({
                "ok": true,
                "var": var,
                "set": value.is_some(),
                "source": source.as_str(),
                "value": value,
            })
        );
        return Ok(());
    }
    match value {
        Some(value) => {
            println!("{value}");
            Ok(())
        }
        None => bail!("{var} is not set. `aster env list` spells every variable Aster reads"),
    }
}

fn unset(repo_root: &Path, var: &str, global: bool, local: bool) -> Result<()> {
    let var = key::normalize(var)?;
    let before = key::source(&var, repo_root);
    let both = !global && !local;
    let mut cleared: Vec<std::path::PathBuf> = Vec::new();

    if (global || both)
        && let Some(path) = key::global_env()
        && crate::init::remove_env_key(&path, &var)?
    {
        cleared.push(path);
    }
    if (local || both)
        && let Some(path) = key::local_env(repo_root)
        && crate::init::remove_env_key(&path, &var)?
    {
        cleared.push(path);
    }

    if crate::json_mode() {
        let paths: Vec<String> = cleared.iter().map(|p| p.display().to_string()).collect();
        println!("{}", json!({ "ok": true, "var": var, "cleared": paths }));
        return Ok(());
    }

    match cleared.is_empty() {
        true => println!("{var} was not in any .env Aster writes, so nothing changed."),
        false => {
            for path in &cleared {
                println!("Cleared {var} from {}", key::display(path));
            }
        }
    }
    if before == Source::Shell {
        println!(
            "{}",
            paint(
                DIM,
                &format!("Your shell still exports {var}. Run `unset {var}` to drop it here too."),
            )
        );
    }
    Ok(())
}

/// The settings page edits Aster's own behavior, so a foreign variable has no
/// business being written here; `aster key set` is the door for those.
fn ensure_aster(var: &str) -> Result<()> {
    if var.starts_with("ASTER_") {
        return Ok(());
    }
    bail!(
        "{var} is not an Aster variable. `aster env` manages ASTER_* names; use `aster key set` for API keys"
    );
}

fn shadow_note(var: &str, before: Source, local: bool) -> Option<String> {
    match before {
        Source::Shell => Some(format!(
            "Your shell already exports {var}, and it wins over any .env. Run `unset {var}` in the shells you use."
        )),
        Source::Local if !local => Some(format!(
            "This repo's .env also sets {var}, and it wins here. Clear it with `aster env unset --local {var}`."
        )),
        _ => None,
    }
}

fn ask(var: &str, secret: bool) -> Result<String> {
    if !crate::picker::is_tty() {
        bail!("no value given. Pass it as `aster env set {var} <value>`, or run this in a terminal to be asked for it");
    }
    let entered = or_cancel(match secret {
        true => {
            use cliclack::password;
            password(format!("{var} (nothing is echoed)"))
                .mask('•')
                .allow_empty()
                .interact()
        }
        false => input(format!("{var}")).interact(),
    })?;
    Ok(entered.unwrap_or_default().trim().to_string())
}

#[cfg(test)]
#[path = "../tests/env_test.rs"]
mod tests;