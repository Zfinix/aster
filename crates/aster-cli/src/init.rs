//! `aster init`: first-run onboarding that scaffolds `aster.yaml` and wires a key into `.env`.
//! Writes to `~/.aster/` by default so the config applies to every repo.
//! Pass `--local` to write into the current directory instead.

use std::io::{self, IsTerminal};
use std::path::Path;
use std::{env, fs};

use anyhow::{Context, Result};
use clap::Args;
use cliclack::{log, multiselect, outro, outro_cancel, password, select, set_theme};
use console::Style;
use serde::Deserialize;

use crate::term::{DIM, GREEN, RESET};
use crate::util::or_cancel;

/// Closest 256-color palette match to the Aster accent.
const ORANGE_256: u8 = 208;

#[derive(Args)]
pub struct InitArgs {
    /// Write config to the current directory instead of ~/.aster/.
    #[arg(long, short = 'l')]
    local: bool,

    /// Overwrite an existing aster.yaml instead of leaving it in place.
    #[arg(long)]
    force: bool,

    /// Skip the wizard: write a default aster.yaml and touch nothing else.
    #[arg(long, short = 'y')]
    yes: bool,
}

/// One provider from the shared `providers.json` catalog, the single source of
/// truth across desktop and web; unused fields are ignored.
#[derive(Debug, Deserialize)]
struct Provider {
    id: String,
    name: String,
    base_url: String,
    #[serde(default)]
    example_model: String,
    #[serde(default)]
    auth: String,
}

#[derive(Deserialize)]
struct Catalog {
    providers: Vec<Provider>,
}

impl Provider {
    /// Local/self-hosted servers advertise "none" or "optional" auth and skip the key prompt.
    fn needs_key(&self) -> bool {
        let a = self.auth.to_ascii_lowercase();
        !(a.contains("none") || a.contains("optional"))
    }

    /// A base URL with a `{placeholder}` the user must fill in before it resolves.
    fn templated(&self) -> bool {
        self.base_url.contains('{')
    }
}

/// Embedded at build time so there's no runtime file to locate.
const PROVIDERS_JSON: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../providers.json"));

fn load_providers() -> Result<Vec<Provider>> {
    let catalog: Catalog =
        serde_json::from_str(PROVIDERS_JSON).context("parsing embedded providers.json")?;
    Ok(catalog.providers)
}

/// Human provider name for a base URL, matched against the catalog; falls back to the host.
pub fn provider_label(base_url: &str) -> String {
    let want = base_url.trim_end_matches('/');
    if let Ok(providers) = load_providers() {
        if let Some(p) = providers
            .iter()
            .find(|p| p.base_url.trim_end_matches('/') == want)
        {
            return p.name.clone();
        }
        let host = host_only(want);
        if let Some(p) = providers
            .iter()
            .find(|p| host_only(p.base_url.trim_end_matches('/')) == host)
        {
            return p.name.clone();
        }
    }
    host_only(want).to_string()
}

/// One row for the TUI's `/provider` picker: name, endpoint, and the model to
/// start from. Templated endpoints are dropped, since there is no one to fill
/// the placeholder in mid-session.
pub fn provider_choices() -> Vec<(String, String, String)> {
    load_providers()
        .unwrap_or_default()
        .into_iter()
        .filter(|p| !p.templated())
        .map(|p| (p.name, p.base_url, p.example_model))
        .collect()
}

/// The env var holding the key for `base_url`, when it is not the shared one.
/// Lets a mid-session provider switch pick up a key that is already exported.
pub fn provider_key(base_url: &str) -> Option<String> {
    let host = host_only(base_url.trim_end_matches('/'));
    let vars: &[&str] = match host {
        h if h.contains("openrouter") => &["OPEN_ROUTER_API_KEY", "OPENROUTER_API_KEY"],
        h if h.contains("anthropic") => &["ANTHROPIC_API_KEY"],
        h if h.contains("openai") => &["OPENAI_API_KEY"],
        h if h.contains("groq") => &["GROQ_API_KEY"],
        h if h.contains("mistral") => &["MISTRAL_API_KEY"],
        h if h.contains("deepseek") => &["DEEPSEEK_API_KEY"],
        h if h.contains("googleapis") => &["GEMINI_API_KEY", "GOOGLE_API_KEY"],
        _ => &[],
    };
    vars.iter()
        .filter_map(|v| env::var(v).ok())
        .find(|v| !v.trim().is_empty())
}

fn host_only(url: &str) -> &str {
    url.split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(url)
        .split('/')
        .next()
        .unwrap_or(url)
}

/// Prefill for the non-interactive path and "Skip" row; OpenRouter is the friendliest default.
fn default_provider(providers: &[Provider]) -> &Provider {
    providers
        .iter()
        .find(|p| p.id == "openrouter")
        .unwrap_or(&providers[0])
}

/// The clack theme, recolored to the Aster orange.
struct AsterTheme;

impl cliclack::Theme for AsterTheme {
    fn bar_color(&self, state: &cliclack::ThemeState) -> Style {
        match state {
            cliclack::ThemeState::Active => Style::new().color256(ORANGE_256),
            cliclack::ThemeState::Cancel => Style::new().red(),
            cliclack::ThemeState::Submit => Style::new().bright().black(),
            cliclack::ThemeState::Error(_) => Style::new().yellow(),
        }
    }

    fn state_symbol_color(&self, state: &cliclack::ThemeState) -> Style {
        match state {
            cliclack::ThemeState::Submit => Style::new().green(),
            _ => self.bar_color(state),
        }
    }
}

pub fn run(args: InitArgs) -> Result<()> {
    let repo_root = env::current_dir().context("resolving the current directory")?;
    let global_config = !args.local;
    let yaml_path = if global_config {
        dirs::home_dir()
            .context("could not determine home directory")?
            .join(".aster/aster.yaml")
    } else {
        repo_root.join("aster.yaml")
    };

    let providers = load_providers()?;

    let interactive =
        !args.yes && !crate::json_mode() && io::stdin().is_terminal() && io::stdout().is_terminal();
    if !interactive {
        let d = default_provider(&providers);
        let note = write_yaml(&yaml_path, &d.base_url, &d.example_model, args.force)?;
        if crate::json_mode() {
            println!(
                "{}",
                serde_json::json!({
                    "ok": true,
                    "path": yaml_path.display().to_string(),
                    "scope": if global_config { "global" } else { "project" },
                    "base_url": d.base_url,
                    "model": d.example_model,
                    "wrote": matches!(note, Note::Success(_)),
                    "message": note.message(),
                })
            );
            return Ok(());
        }
        emit(note, false)?;
        finish_plain(global_config);
        return Ok(());
    }

    set_theme(AsterTheme);
    print!("{}", crate::tui::mark_ansi());

    let Some(cfg) = wizard(&providers)? else {
        outro_cancel("Cancelled. Nothing was written.")?;
        return Ok(());
    };

    emit(
        write_yaml(&yaml_path, &cfg.base_url, &cfg.model, args.force)?,
        true,
    )?;
    let env_path = yaml_path.with_file_name(".env");
    let gitignore = args.local;
    let mut stored_key = false;
    if let Some(key) = cfg.api_key.filter(|k| !k.trim().is_empty()) {
        emit(
            store_key(&env_path, "ASTER_API_KEY", key.trim(), gitignore)?,
            true,
        )?;
        stored_key = true;
    }
    if let Some(key) = cfg.context_dev_key.filter(|k| !k.trim().is_empty()) {
        emit(
            store_key(&env_path, "CONTEXT_DEV_API_KEY", key.trim(), gitignore)?,
            true,
        )?;
    }
    if let Some(key) = cfg.jina_key.filter(|k| !k.trim().is_empty()) {
        emit(
            store_key(&env_path, "JINA_API_KEY", key.trim(), gitignore)?,
            true,
        )?;
    }

    let next = if !stored_key && !has_api_key() {
        NO_KEY_HINT
    } else if global_config {
        "You're set. cd into any repo and run: aster"
    } else {
        "You're set. Next: aster"
    };
    outro(next)?;
    Ok(())
}

struct Configured {
    base_url: String,
    model: String,
    api_key: Option<String>,
    context_dev_key: Option<String>,
    jina_key: Option<String>,
}

/// What the user opted into on the first screen; nothing is prompted for the rest.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Setup {
    Provider,
    ContextDev,
    Jina,
}

/// Pick what to set up first, then only prompt for what was picked.
/// `None` when the user cancels.
fn wizard(providers: &[Provider]) -> Result<Option<Configured>> {
    let Some(picked) = or_cancel(
        multiselect("What do you want to set up? (space to toggle · enter to confirm)")
            .initial_values(vec![Setup::Provider])
            .required(false)
            .item(Setup::Provider, "Model provider", "the model Aster runs on")
            .item(Setup::ContextDev, "Web crawl", "Context.dev API key")
            .item(Setup::Jina, "Web reading", "Jina API key")
            .interact(),
    )?
    else {
        return Ok(None);
    };

    let default = default_provider(providers);
    let mut cfg = Configured {
        base_url: default.base_url.clone(),
        model: default.example_model.clone(),
        api_key: None,
        context_dev_key: None,
        jina_key: None,
    };

    if picked.contains(&Setup::Provider) {
        let Some(chosen) = provider_setup(providers)? else {
            return Ok(None);
        };
        cfg.base_url = chosen.0;
        cfg.model = chosen.1;
        cfg.api_key = chosen.2;
    }

    // Escaping an optional key prompt skips that key; the answers already given
    // are kept rather than thrown away.
    if picked.contains(&Setup::ContextDev) {
        cfg.context_dev_key = or_cancel(
            password("Context.dev API key (enter to skip)")
                .mask('•')
                .interact(),
        )?;
    }

    if picked.contains(&Setup::Jina) {
        cfg.jina_key = or_cancel(
            password("Jina API key (enter to skip)")
                .mask('•')
                .interact(),
        )?;
    }

    Ok(Some(cfg))
}

/// Provider, base URL, model, key. `None` when the user cancels.
fn provider_setup(providers: &[Provider]) -> Result<Option<(String, String, Option<String>)>> {
    let mut menu = select::<usize>("Which model provider? (type to search)")
        .initial_value(0)
        .filter_mode()
        .max_rows(8);
    for (i, p) in providers.iter().enumerate() {
        menu = menu.item(i, &p.name, &p.base_url);
    }
    let Some(idx) = or_cancel(menu.interact())? else {
        return Ok(None);
    };
    let provider = &providers[idx];

    let base_url = if provider.templated() {
        let Some(url) = or_cancel(
            cliclack::input("Base URL")
                .default_input(&provider.base_url)
                .interact::<String>(),
        )?
        else {
            return Ok(None);
        };
        url
    } else {
        provider.base_url.clone()
    };

    let Some(model) = or_cancel(
        cliclack::input("Model")
            .default_input(&provider.example_model)
            .interact::<String>(),
    )?
    else {
        return Ok(None);
    };

    let key_prompt = if provider.needs_key() {
        "API key (enter to add later)"
    } else {
        "API key (usually none · enter to skip)"
    };
    // Escaping the key prompt keeps the provider and model already chosen.
    let key = or_cancel(password(key_prompt).mask('•').interact())?;

    Ok(Some((
        base_url.trim().to_string(),
        model.trim().to_string(),
        key,
    )))
}

/// A one-line status, emitted inside the clack frame or as a plain line.
enum Note {
    Success(String),
    Info(String),
}

impl Note {
    fn message(&self) -> &str {
        match self {
            Note::Success(m) | Note::Info(m) => m,
        }
    }
}

fn emit(note: Note, framed: bool) -> Result<()> {
    match (framed, note) {
        (true, Note::Success(m)) => log::success(m)?,
        (true, Note::Info(m)) => log::info(m)?,
        (false, Note::Success(m)) => println!("  {GREEN}✓{RESET} {m}"),
        (false, Note::Info(m)) => println!("  {DIM}{m}{RESET}"),
    }
    Ok(())
}

/// True when a key is already reachable, so "you're set" is not a lie.
fn has_api_key() -> bool {
    ["ASTER_API_KEY", "OPEN_ROUTER_API_KEY"]
        .iter()
        .any(|k| env::var(k).is_ok_and(|v| !v.trim().is_empty()))
}

const NO_KEY_HINT: &str =
    "No API key yet. Set ASTER_API_KEY in your shell, or run `aster init` again to store one.";

fn finish_plain(global: bool) {
    if !has_api_key() {
        println!("  {DIM}{NO_KEY_HINT}{RESET}");
        return;
    }
    if global {
        println!("  {DIM}You're set. cd into any repo and run:{RESET} aster");
    } else {
        println!("  {DIM}Next:{RESET} aster");
    }
}

/// Store the API key in `.env`. When `gitignore` is set, also keep the file out of git.
fn store_key(env_path: &Path, var_name: &str, key: &str, gitignore: bool) -> Result<Note> {
    if env_has_key(env_path, var_name) {
        return Ok(Note::Info(format!(
            "{var_name} already set in .env · leaving it"
        )));
    }
    if let Some(parent) = env_path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    append_line(env_path, &format!("{var_name}={key}"))
        .with_context(|| format!("writing {}", env_path.display()))?;
    if gitignore && let Some(dir) = env_path.parent() {
        ensure_gitignored(dir, ".env")?;
    }
    Ok(Note::Success(format!(
        "Stored {var_name} in {}",
        display(env_path)
    )))
}

fn write_yaml(path: &Path, base_url: &str, model: &str, force: bool) -> Result<Note> {
    if path.exists() && !force {
        return Ok(Note::Info(format!(
            "{} already exists · keeping it (use --force to overwrite)",
            display(path)
        )));
    }
    let rewrite = path.exists();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    fs::write(path, yaml_contents(base_url, model))
        .with_context(|| format!("writing {}", path.display()))?;
    let verb = if rewrite { "Rewrote" } else { "Wrote" };
    Ok(Note::Success(format!("{verb} {}", display(path))))
}

fn yaml_contents(base_url: &str, model: &str) -> String {
    format!(
        "# Aster review config. Precedence: CLI flags > shell env > this file > defaults.\n\
         # API keys are NEVER read from here. Use ASTER_API_KEY or `aster login`.\n\
         review:\n\
         \x20 model: {model}\n\
         \x20 base_url: {base_url}\n\n\
         \x20 # Drop findings below this confidence (0.0-1.0).\n\
         \x20 min_confidence: 0.6\n\n\
         \x20 # Bias the hypothesis pass toward these defect classes.\n\
         \x20 focus_areas:\n\
         \x20   - correctness\n\
         \x20   - security\n\n\
         \x20 # Which files to review. `include` empty = everything except `exclude`.\n\
         \x20 include: []\n\
         \x20 exclude:\n\
         \x20   - \"target/**\"\n\
         \x20   - \"node_modules/**\"\n\n\
         # MCP servers that give the agent extra tools. Disabled servers are\n\
         # kept in the config but never started.\n\
         mcp:\n\
         \x20 servers:\n\
         \x20   # Ships with Chrome. Let the agent browse, screenshot, and inspect pages.\n\
         \x20   # Set `disabled: false` or remove the line to enable it.\n\
         \x20   chrome:\n\
         \x20     command: npx\n\
         \x20     args:\n\
         \x20       - \"-y\"\n\
         \x20       - \"chrome-devtools-mcp@latest\"\n\
         \x20     disabled: true\n\
         \x20   # Headless browser via Playwright. Heavier but needs no Chrome flag tweaks.\n\
         \x20   playwright:\n\
         \x20     command: npx\n\
         \x20     args:\n\
         \x20       - \"-y\"\n\
         \x20       - \"@playwright/mcp@latest\"\n\
         \x20     disabled: true\n"
    )
}

/// True if `.env` already defines `key`, so we never clobber an existing secret.
fn env_has_key(env_path: &Path, key: &str) -> bool {
    let Ok(text) = fs::read_to_string(env_path) else {
        return false;
    };
    text.lines().any(|l| {
        let l = l.trim_start();
        l.strip_prefix(key)
            .is_some_and(|rest| rest.starts_with('='))
    })
}

fn append_line(path: &Path, line: &str) -> Result<()> {
    let mut content = fs::read_to_string(path).unwrap_or_default();
    if !content.is_empty() && !content.ends_with('\n') {
        content.push('\n');
    }
    content.push_str(line);
    content.push('\n');
    fs::write(path, content)?;
    Ok(())
}

fn ensure_gitignored(repo_root: &Path, entry: &str) -> Result<()> {
    let path = repo_root.join(".gitignore");
    let existing = fs::read_to_string(&path).unwrap_or_default();
    if existing.lines().any(|l| l.trim() == entry) {
        return Ok(());
    }
    append_line(&path, entry).with_context(|| format!("updating {}", path.display()))
}

/// Show a repo-relative path when we can, so output stays short.
fn display(path: &Path) -> String {
    env::current_dir()
        .ok()
        .and_then(|cwd| path.strip_prefix(&cwd).ok())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| path.to_path_buf())
        .display()
        .to_string()
}

#[cfg(test)]
#[path = "tests/init_test.rs"]
mod tests;
