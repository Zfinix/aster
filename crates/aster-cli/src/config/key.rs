//! `aster key`: store the API keys Aster reads from the environment. Keys are
//! never read from `aster.yaml`, so `.env` is the file this command owns.

use std::path::{Path, PathBuf};
use std::{env, fs};

use anyhow::{Context, Result, bail};
use aster_ai::keys::{SHARED_KEY_VAR, catalog_key_vars, env_non_empty};
use clap::{Args, Subcommand};
use cliclack::password;
use serde_json::json;

use crate::term::{BOLD, DIM, paint};
use crate::util::or_cancel;

#[derive(Args)]
pub struct KeyArgs {
    #[command(subcommand)]
    command: Option<KeyCmd>,
}

#[derive(Subcommand)]
enum KeyCmd {
    /// Every key Aster reads, whether it is set, and which file it came from.
    List(ListArgs),
    /// Print one key's live value, for scripts and the editors' reveal button.
    Get(GetArgs),
    /// Store a key in `.env`, asking for the value when it is not given.
    Set(SetArgs),
    /// Take a key back out of `.env`.
    Unset(UnsetArgs),
    /// Which `.env` files a key would be written to and read from.
    Path,
}

#[derive(Args)]
struct ListArgs {
    /// Include every model endpoint, not only the ones holding a key.
    #[arg(long)]
    all: bool,
}

#[derive(Args)]
struct GetArgs {
    /// The variable to read, e.g. `OPENROUTER_API_KEY`.
    #[arg(value_name = "VAR")]
    var: String,
}

#[derive(Args)]
struct SetArgs {
    /// The variable, e.g. `FIRECRAWL_API_KEY`. `aster key list` spells them all.
    #[arg(value_name = "VAR")]
    var: String,

    /// The key itself. Left out, it is asked for without echoing, which keeps
    /// it out of your shell history.
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

pub fn run(args: KeyArgs) -> Result<()> {
    let repo_root = env::current_dir().context("could not determine the current directory")?;
    match args.command {
        None => list(&repo_root, false),
        Some(KeyCmd::List(a)) => list(&repo_root, a.all),
        Some(KeyCmd::Get(a)) => get(&repo_root, &a.var),
        Some(KeyCmd::Set(a)) => set(&repo_root, &a.var, a.value.as_deref(), a.local, a.stdin),
        Some(KeyCmd::Unset(a)) => unset(&repo_root, &a.var, a.global, a.local),
        Some(KeyCmd::Path) => paths(&repo_root),
    }
}

/// Where a key that is in effect came from. The shell outranks both files, so a
/// key written to one can still be shadowed by an export.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Source {
    Shell,
    Local,
    Global,
    Unset,
}

impl Source {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Source::Shell => "your shell",
            Source::Local => "this repo's .env",
            Source::Global => "~/.aster/.env",
            Source::Unset => "not set",
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Source::Shell => "shell",
            Source::Local => "local",
            Source::Global => "global",
            Source::Unset => "unset",
        }
    }
}

/// Which layer supplies `var` right now. Startup loads both files into the
/// environment, so the live value is matched back against each file rather than
/// read as proof of an export: what neither file holds came from the shell.
pub(crate) fn source(var: &str, repo_root: &Path) -> Source {
    let Some(live) = env_non_empty(var) else {
        return Source::Unset;
    };
    if file_value(local_env(repo_root).as_deref(), var).as_deref() == Some(live.as_str()) {
        return Source::Local;
    }
    if file_value(global_env().as_deref(), var).as_deref() == Some(live.as_str()) {
        return Source::Global;
    }
    Source::Shell
}

fn file_value(path: Option<&Path>, var: &str) -> Option<String> {
    let text = fs::read_to_string(path?).ok()?;
    text.lines()
        .filter_map(|line| assignment(line, var))
        .next_back()
        .map(str::to_string)
}

fn assignment<'a>(line: &'a str, var: &str) -> Option<&'a str> {
    let rest = line.trim_start().strip_prefix(var)?.strip_prefix('=')?;
    let rest = rest.trim();
    Some(
        match rest.strip_prefix('"').and_then(|r| r.strip_suffix('"')) {
            Some(inner) => inner,
            None => match rest.strip_prefix('\'').and_then(|r| r.strip_suffix('\'')) {
                Some(inner) => inner,
                None => rest,
            },
        },
    )
}

fn global_env() -> Option<PathBuf> {
    crate::persist::global_env_path()
}

fn local_env(repo_root: &Path) -> Option<PathBuf> {
    let beside = crate::settings::project_config(Some(repo_root))
        .map(|yaml| yaml.with_file_name(".env"))
        .unwrap_or_else(|| repo_root.join(".env"));
    Some(beside)
}

/// The web providers, each with the vars it needs, in the order aster-web
/// dispatches them. Cloudflare takes two, so a provider is a group rather than
/// a single var; `aster init` prompts from this so it cannot fall behind.
pub(crate) fn web_providers() -> Vec<(&'static str, Vec<(&'static str, &'static str)>)> {
    let mut out: Vec<(&'static str, Vec<(&'static str, &'static str)>)> = Vec::new();
    for (provider, var, buys) in aster_web::KEY_VARS {
        match out.last_mut() {
            Some((name, vars)) if name == provider => vars.push((var, buys)),
            _ => out.push((provider, vec![(var, buys)])),
        }
    }
    out
}

/// True when every var a provider needs is set, so a half-configured
/// Cloudflare does not read as done.
pub(crate) fn provider_is_set(vars: &[(&'static str, &'static str)]) -> bool {
    vars.iter().all(|(var, _)| env_non_empty(var).is_some())
}

pub(crate) struct Row {
    pub(crate) group: &'static str,
    pub(crate) label: String,
    pub(crate) var: &'static str,
    pub(crate) help: String,
    pub(crate) source: Source,
    pub(crate) masked: Option<String>,
}

/// The tail of a stored key, enough to tell two apart without showing one.
pub(crate) fn masked(var: &str) -> Option<String> {
    env_non_empty(var).map(|live| mask_tail(&live))
}

fn mask_tail(live: &str) -> String {
    let chars: Vec<char> = live.chars().collect();
    if chars.len() <= 8 {
        return "••••".to_string();
    }
    let tail: String = chars[chars.len() - 4..].iter().collect();
    format!("…{tail}")
}

pub(crate) fn rows(repo_root: &Path, all: bool) -> Vec<Row> {
    let mut out: Vec<Row> = aster_web::KEY_VARS
        .iter()
        .map(|(provider, var, buys)| Row {
            group: "Web tools",
            label: (*provider).to_string(),
            var,
            help: (*buys).to_string(),
            source: source(var, repo_root),
            masked: masked(var),
        })
        .collect();

    out.push(Row {
        group: "Model",
        label: "Any endpoint".to_string(),
        var: SHARED_KEY_VAR,
        help: "used by any endpoint without a var of its own".to_string(),
        source: source(SHARED_KEY_VAR, repo_root),
        masked: masked(SHARED_KEY_VAR),
    });
    for (provider, var) in catalog_key_vars() {
        let source = source(var, repo_root);
        // The catalog is long and mostly irrelevant to any one user, so a model
        // endpoint earns a row by holding a key unless every one was asked for.
        if !all && source == Source::Unset {
            continue;
        }
        out.push(Row {
            group: "Model",
            label: (*provider).to_string(),
            var,
            help: String::new(),
            source,
            masked: masked(var),
        });
    }
    out
}

pub(crate) fn list(repo_root: &Path, all: bool) -> Result<()> {
    let rows = rows(repo_root, all);

    if crate::json_mode() {
        let keys: Vec<_> = rows
            .iter()
            .map(|r| {
                json!({
                    "var": r.var,
                    "provider": r.label,
                    "group": r.group,
                    "set": r.source != Source::Unset,
                    "source": r.source.as_str(),
                    "masked": r.masked,
                    "help": r.help,
                })
            })
            .collect();
        println!("{}", json!({ "ok": true, "keys": keys }));
        return Ok(());
    }

    for group in ["Web tools", "Model"] {
        let rows: Vec<&Row> = rows.iter().filter(|r| r.group == group).collect();
        if rows.is_empty() {
            continue;
        }
        let width = rows.iter().map(|r| r.var.len()).max().unwrap_or(0);
        println!("{}", paint(BOLD, group));
        for row in rows {
            let note = match (row.source, &row.masked) {
                (Source::Unset, _) if row.help.is_empty() => "not set".to_string(),
                (Source::Unset, _) => format!("not set · {}", row.help),
                (other, Some(tail)) => format!("{tail} · {}", other.label()),
                (other, None) => format!("set · {}", other.label()),
            };
            println!("  {:<width$}  {}", row.var, paint(DIM, &note));
        }
        println!();
    }
    if !all {
        println!(
            "{}",
            paint(
                DIM,
                "Model endpoints without a key are hidden; --all shows them."
            )
        );
    }
    Ok(())
}

fn get(repo_root: &Path, var: &str) -> Result<()> {
    let var = normalize(var)?;
    let source = source(&var, repo_root);
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
        None => bail!("{var} is not set. `aster key list --all` spells every key Aster reads"),
    }
}

pub(crate) fn set(
    repo_root: &Path,
    var: &str,
    value: Option<&str>,
    local: bool,
    stdin: bool,
) -> Result<()> {
    let var = normalize(var)?;
    let value = match (stdin, value) {
        (true, _) => {
            let mut line = String::new();
            std::io::stdin()
                .read_line(&mut line)
                .context("could not read the key from stdin")?;
            line.trim().to_string()
        }
        (false, Some(v)) => v.trim().to_string(),
        (false, None) => ask(&var)?,
    };
    if value.is_empty() {
        bail!(
            "no key given. Pass it as `aster key set {var} <value>`, or leave it off to be asked for it"
        );
    }
    if value.contains(['\n', '\r']) {
        bail!(
            "that key contains a line break, so it cannot be stored in .env. Check what you pasted"
        );
    }

    // Read before writing: startup loaded both files into the environment, so
    // once the file changes the live value no longer traces back to its layer.
    let before = source(&var, repo_root);
    let path = match local {
        true => local_env(repo_root).context("this directory has no repo to write .env into")?,
        false => global_env().context("could not determine your home directory")?,
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    let replaced = file_value(Some(&path), &var).is_some();
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
    println!("{verb} {var} in {}", display(&path));
    if let Some(note) = shadow_note(&var, before, local) {
        println!("{}", paint(DIM, &note));
    }
    if !known(&var) {
        println!(
            "{}",
            paint(
                DIM,
                &format!(
                    "Nothing in Aster reads {var}; `aster key list` shows the ones that are read."
                ),
            )
        );
    }
    Ok(())
}

fn shadow_note(var: &str, before: Source, local: bool) -> Option<String> {
    match before {
        Source::Shell => Some(format!(
            "Your shell already exports {var}, and it wins over any .env. Run `unset {var}` in the shells you use."
        )),
        Source::Local if !local => Some(format!(
            "This repo's .env also sets {var}, and it wins here. Clear it with `aster key unset --local {var}`."
        )),
        _ => None,
    }
}

pub(crate) fn unset(repo_root: &Path, var: &str, global: bool, local: bool) -> Result<()> {
    let var = normalize(var)?;
    let before = source(&var, repo_root);
    let both = !global && !local;
    let mut cleared: Vec<PathBuf> = Vec::new();

    if (global || both)
        && let Some(path) = global_env()
        && crate::init::remove_env_key(&path, &var)?
    {
        cleared.push(path);
    }
    if (local || both)
        && let Some(path) = local_env(repo_root)
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
                println!("Cleared {var} from {}", display(path));
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

fn paths(repo_root: &Path) -> Result<()> {
    let rows = [("global", global_env()), ("local", local_env(repo_root))];
    if crate::json_mode() {
        let files: Vec<_> = rows
            .iter()
            .map(|(scope, path)| {
                json!({
                    "scope": scope,
                    "path": path.as_ref().map(|p| p.display().to_string()),
                    "exists": path.as_ref().is_some_and(|p| p.exists()),
                })
            })
            .collect();
        println!("{}", json!({ "ok": true, "files": files }));
        return Ok(());
    }
    for (scope, path) in rows {
        let Some(path) = path else { continue };
        let state = match path.exists() {
            true => "exists",
            false => "not created yet",
        };
        println!("{scope:<7} {}  {}", display(&path), paint(DIM, state));
    }
    println!(
        "{}",
        paint(
            DIM,
            "Your shell outranks both, then the repo file, then the global one."
        )
    );
    Ok(())
}

fn normalize(var: &str) -> Result<String> {
    let var = var.trim().to_ascii_uppercase();
    if var.is_empty() {
        bail!("no variable given. `aster key list` spells the ones Aster reads");
    }
    let shaped = var.starts_with(|c: char| c.is_ascii_alphabetic())
        && var.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if !shaped {
        bail!(
            "{var} is not a usable variable name: use letters, digits, and underscores, e.g. FIRECRAWL_API_KEY"
        );
    }
    Ok(var)
}

fn known(var: &str) -> bool {
    var == SHARED_KEY_VAR
        || aster_web::KEY_VARS
            .iter()
            .any(|(_, known, _)| *known == var)
        || catalog_key_vars().iter().any(|(_, known)| *known == var)
}

fn ask(var: &str) -> Result<String> {
    if !crate::picker::is_tty() {
        bail!(
            "no key given. Pass it as `aster key set {var} <value>`, or run this in a terminal to be asked for it"
        );
    }
    let entered = or_cancel(
        password(format!("{var} (nothing is echoed)"))
            .mask('•')
            .allow_empty()
            .interact(),
    )?;
    Ok(entered.unwrap_or_default().trim().to_string())
}

fn display(path: &Path) -> String {
    let Some(home) = dirs::home_dir() else {
        return path.display().to_string();
    };
    match path.strip_prefix(&home) {
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.display().to_string(),
    }
}

#[cfg(test)]
#[path = "../tests/key_test.rs"]
mod tests;
