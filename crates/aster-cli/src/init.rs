//! `aster init`: first-run onboarding. Inline clack-style prompts via `cliclack`
//! pick a provider, model, and API key, then scaffold `aster.yaml` and wire the
//! key into `.env` — so a fresh clone goes from "no API key found" to a working
//! `aster review` in one step. Falls back to a default config when stdout isn't
//! a terminal (CI, pipes) or `--yes` is passed.

use std::io::{self, IsTerminal};
use std::path::Path;
use std::{env, fs};

use anyhow::{Context, Result};
use clap::Args;
use cliclack::{intro, log, outro, outro_cancel, password, select, set_theme};
use console::Style;
use serde::Deserialize;

const DIM: &str = "\x1b[2m";
const GREEN: &str = "\x1b[32m";
const RESET: &str = "\x1b[0m";
/// 256-color orange, the closest palette match to the Aster accent.
const ORANGE_256: u8 = 208;

#[derive(Args)]
pub struct InitArgs {
    /// Write the global config (~/.config/aster/aster.yaml) instead of the repo root.
    #[arg(long, short = 'g')]
    global: bool,

    /// Overwrite an existing aster.yaml instead of leaving it in place.
    #[arg(long)]
    force: bool,

    /// Skip the wizard: write a default aster.yaml and touch nothing else.
    #[arg(long, short = 'y')]
    yes: bool,
}

/// One provider from the shared `providers.json` catalog at the repo root. That
/// file is the single source of truth so the desktop app and web can grab the
/// same list; fields we don't use here (notes) are ignored.
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
    /// Whether this endpoint needs a real API key. Local/self-hosted servers
    /// advertise "none" or "optional" auth and get to skip the key prompt.
    fn needs_key(&self) -> bool {
        let a = self.auth.to_ascii_lowercase();
        !(a.contains("none") || a.contains("optional"))
    }

    /// A base URL with a `{placeholder}` (Azure resource, Bedrock region, …) the
    /// user must fill in before it will resolve.
    fn templated(&self) -> bool {
        self.base_url.contains('{')
    }
}

/// The catalog is embedded at build time, so the binary carries it and there's
/// no runtime file to locate.
const PROVIDERS_JSON: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../providers.json"));

fn load_providers() -> Result<Vec<Provider>> {
    let catalog: Catalog =
        serde_json::from_str(PROVIDERS_JSON).context("parsing embedded providers.json")?;
    Ok(catalog.providers)
}

/// Human provider name for a base URL (e.g. `OpenRouter`), matched against the
/// catalog. Falls back to the bare host when nothing matches.
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

fn host_only(url: &str) -> &str {
    url.split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(url)
        .split('/')
        .next()
        .unwrap_or(url)
}

/// The provider to prefill for the non-interactive path and the "Skip" row.
/// OpenRouter is the friendliest default (aggregator, one key, any model).
fn default_provider(providers: &[Provider]) -> &Provider {
    providers
        .iter()
        .find(|p| p.id == "openrouter")
        .unwrap_or(&providers[0])
}

/// The clack theme, recolored from the default cyan to the Aster orange.
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
    let yaml_path = if args.global {
        dirs::config_dir()
            .context("could not determine a config directory for this platform")?
            .join("aster/aster.yaml")
    } else {
        repo_root.join("aster.yaml")
    };

    let providers = load_providers()?;

    let interactive = !args.yes && io::stdin().is_terminal() && io::stdout().is_terminal();
    if !interactive {
        let d = default_provider(&providers);
        emit(
            write_yaml(&yaml_path, &d.base_url, &d.example_model, args.force)?,
            false,
        )?;
        finish_plain(args.global);
        return Ok(());
    }

    set_theme(AsterTheme);
    intro(
        Style::new()
            .color256(ORANGE_256)
            .bold()
            .apply_to(" ✳ aster init "),
    )?;

    let Some((base_url, model, key)) = wizard(&providers)? else {
        outro_cancel("Cancelled. Nothing was written.")?;
        return Ok(());
    };

    emit(write_yaml(&yaml_path, &base_url, &model, args.force)?, true)?;
    if let Some(key) = key.filter(|k| !k.trim().is_empty()) {
        // The repo `.env` is git-ignored on our behalf; the global one lives
        // beside the global config and is loaded on every run (see main.rs).
        let env_path = yaml_path.with_file_name(".env");
        emit(store_key(&env_path, key.trim(), !args.global)?, true)?;
    }

    let next = if args.global {
        "cd into a repo, then run: aster review"
    } else {
        "You're set. Next: aster review"
    };
    outro(next)?;
    Ok(())
}

type Configured = (String, String, Option<String>);

/// The prompt sequence: provider → (base URL) → model → key. Returns `None`
/// when the user cancels (Esc / Ctrl+C).
fn wizard(providers: &[Provider]) -> Result<Option<Configured>> {
    let skip = providers.len();
    let mut menu = select::<usize>("Which model provider?")
        .initial_value(0)
        .max_rows(8);
    for (i, p) in providers.iter().enumerate() {
        menu = menu.item(i, &p.name, &p.base_url);
    }
    menu = menu.item(skip, "Skip · set env vars myself", "");
    let Some(idx) = or_cancel(menu.interact())? else {
        return Ok(None);
    };

    let Some(provider) = providers.get(idx) else {
        // The trailing "Skip" row: write defaults, wire nothing.
        let d = default_provider(providers);
        return Ok(Some((d.base_url.clone(), d.example_model.clone(), None)));
    };

    // Only prompt for the base URL when it has a `{placeholder}` to fill in;
    // every other provider's URL is ready to use, so we skip the step.
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
    let Some(key) = or_cancel(password(key_prompt).mask('•').interact())? else {
        return Ok(None);
    };

    Ok(Some((
        base_url.trim().to_string(),
        model.trim().to_string(),
        Some(key),
    )))
}

/// Map a cliclack prompt result to `Option`: `Interrupted` (Esc / Ctrl+C) means
/// the user cancelled; any other error propagates.
fn or_cancel<T>(result: io::Result<T>) -> Result<Option<T>> {
    match result {
        Ok(v) => Ok(Some(v)),
        Err(e) if e.kind() == io::ErrorKind::Interrupted => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// A one-line status, emitted inside the clack frame (interactive) or as a plain
/// line (CI / piped).
enum Note {
    Success(String),
    Info(String),
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

fn finish_plain(global: bool) {
    if global {
        println!("  {DIM}cd into a repo, then run:{RESET} aster review");
    } else {
        println!("  {DIM}Next:{RESET} aster review");
    }
}

/// Store the API key in `.env`. When `gitignore` is set, also keep the file out
/// of git (true for a repo `.env`, false for the global config dir).
fn store_key(env_path: &Path, key: &str, gitignore: bool) -> Result<Note> {
    if env_has_key(env_path, "ASTER_API_KEY") {
        return Ok(Note::Info(
            "ASTER_API_KEY already set in .env · leaving it".into(),
        ));
    }
    if let Some(parent) = env_path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    append_line(env_path, &format!("ASTER_API_KEY={key}"))
        .with_context(|| format!("writing {}", env_path.display()))?;
    if gitignore && let Some(dir) = env_path.parent() {
        ensure_gitignored(dir, ".env")?;
    }
    Ok(Note::Success(format!(
        "Stored key in {}",
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
         \x20   - \"node_modules/**\"\n"
    )
}

/// True if `.env` already defines `key` (so we never clobber an existing secret).
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

/// Add `entry` to the repo's `.gitignore` if it isn't ignored already.
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
mod tests {
    use super::*;

    #[test]
    fn catalog_parses_and_has_openrouter() {
        let providers = load_providers().expect("embedded providers.json parses");
        assert!(providers.len() > 10);
        assert_eq!(default_provider(&providers).id, "openrouter");
    }

    #[test]
    fn needs_key_follows_auth() {
        let cloud = Provider {
            id: "x".into(),
            name: "X".into(),
            base_url: "https://x/v1".into(),
            example_model: "m".into(),
            auth: "Bearer".into(),
        };
        let local = Provider {
            auth: "none".into(),
            ..Provider {
                id: "o".into(),
                name: "O".into(),
                base_url: "http://localhost/v1".into(),
                example_model: "m".into(),
                auth: String::new(),
            }
        };
        assert!(cloud.needs_key());
        assert!(!local.needs_key());
    }

    #[test]
    fn templated_detects_placeholder() {
        let providers = load_providers().unwrap();
        let azure = providers.iter().find(|p| p.id == "azure_openai").unwrap();
        let groq = providers.iter().find(|p| p.id == "groq").unwrap();
        assert!(azure.templated());
        assert!(!groq.templated());
    }

    #[test]
    fn yaml_contains_selected_provider() {
        let y = yaml_contents("http://localhost:11434/v1", "qwen2.5-coder");
        assert!(y.contains("base_url: http://localhost:11434/v1"));
        assert!(y.contains("model: qwen2.5-coder"));
    }

    #[test]
    fn env_has_key_matches_only_exact_key() {
        let dir = tempfile::tempdir().unwrap();
        let env = dir.path().join(".env");
        fs::write(&env, "ASTER_API_KEY=sk-123\nOTHER=1\n").unwrap();
        assert!(env_has_key(&env, "ASTER_API_KEY"));
        assert!(!env_has_key(&env, "ASTER_BASE_URL"));
    }

    #[test]
    fn append_line_adds_trailing_newline_when_missing() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join(".env");
        fs::write(&f, "A=1").unwrap();
        append_line(&f, "B=2").unwrap();
        assert_eq!(fs::read_to_string(&f).unwrap(), "A=1\nB=2\n");
    }
}
