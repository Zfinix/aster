//! `aster init`: first-run onboarding that scaffolds `aster.yaml` and wires a key into `.env`.
//! Writes to `~/.aster/` by default so the config applies to every repo.
//! Pass `--local` to write into the current directory instead.

use std::io::{self, IsTerminal};
use std::path::Path;
use std::{env, fs};

use anyhow::{Context, Result, bail};
use aster_ai::AiClient;
use aster_ai::keys;
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
pub(crate) struct Provider {
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

pub(crate) fn load_providers() -> Result<Vec<Provider>> {
    let catalog: Catalog =
        serde_json::from_str(PROVIDERS_JSON).context("parsing embedded providers.json")?;
    Ok(catalog.providers)
}

/// Every non-templated catalog row as (id, base_url), for callers that map
/// provider ids to endpoints without the full picker rows.
pub fn provider_base_urls() -> Vec<(String, String)> {
    load_providers()
        .unwrap_or_default()
        .into_iter()
        .filter(|p| !p.templated())
        .map(|p| (p.id, p.base_url))
        .collect()
}

/// Human provider name for a base URL, matched against the catalog; falls back to the host.
pub fn provider_label(base_url: &str) -> String {
    let want = base_url.trim_end_matches('/');
    lookup(want)
        .map(|p| p.name)
        .unwrap_or_else(|| host_only(want).to_string())
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

/// The catalog's shortlist for `base_url`, falling back to its example model.
/// Empty when the endpoint is unknown, which reads as "ask the endpoint".
pub fn provider_recommended(base_url: &str) -> Vec<String> {
    aster_ai::keys::catalog_models(base_url)
}

/// Resolve what a user typed at `provider use` against the catalog: an id, a
/// name, or a base URL. A URL that matches nothing is taken at face value, so
/// self-hosted endpoints work without a catalog entry.
pub fn find_provider(target: &str) -> Result<(String, String, String)> {
    let want = target.trim().trim_end_matches('/');
    let providers = load_providers()?;
    let found = providers.into_iter().filter(|p| !p.templated()).find(|p| {
        p.id.eq_ignore_ascii_case(want)
            || p.name.eq_ignore_ascii_case(want)
            || p.base_url.trim_end_matches('/').eq_ignore_ascii_case(want)
    });
    if let Some(p) = found {
        return Ok((p.name, p.base_url, p.example_model));
    }
    if want.starts_with("http://") || want.starts_with("https://") {
        return Ok((provider_label(want), want.to_string(), String::new()));
    }
    bail!("no provider {target:?} in the catalog; run `aster provider list` to see the ids")
}

/// The catalog entry for a base URL: every exact match is considered before any
/// host match, so a shared host never shadows the endpoint actually named.
fn lookup(base_url: &str) -> Option<Provider> {
    let want = base_url.trim_end_matches('/');
    let providers = load_providers().ok()?;
    let exact = providers
        .iter()
        .position(|p| p.base_url.trim_end_matches('/') == want);
    let host = host_only(want);
    let by_host = || {
        providers
            .iter()
            .position(|p| host_only(p.base_url.trim_end_matches('/')) == host)
    };
    let at = exact.or_else(by_host)?;
    providers.into_iter().nth(at)
}

pub use aster_ai::keys::provider_key_vars;

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
pub(crate) struct AsterTheme;

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

/// The bare `aster config` entry when nothing is set up yet: the same
/// onboarding `aster init` runs, with its defaults.
pub(crate) async fn run_onboarding() -> Result<()> {
    run(InitArgs {
        local: false,
        force: false,
        yes: false,
    })
    .await
}

pub async fn run(args: InitArgs) -> Result<()> {
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
    let current = Current::read(&repo_root);

    let interactive =
        !args.yes && !crate::json_mode() && io::stdin().is_terminal() && io::stdout().is_terminal();
    if !interactive {
        let d = default_provider(&providers);
        let note = scaffold(&yaml_path, &d.base_url, &d.example_model, args.force)?;
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
        finish_plain(global_config, &current.base_url);
        return Ok(());
    }

    // A rerun with everything already set up is configuration, not onboarding:
    // the config form covers the provider, keys, and every setting in one place.
    if current.configured && !args.force && !args.local {
        return crate::config::menu(&repo_root).await;
    }

    set_theme(AsterTheme);
    print!("{}", crate::tui::mark_ansi());
    log::info(current.summary())?;

    let Some(cfg) = wizard(&providers, &current).await? else {
        outro_cancel("Cancelled. Nothing was written.")?;
        return Ok(());
    };

    let env_path = yaml_path.with_file_name(".env");
    let gitignore = args.local;

    let configured_provider = !cfg.base_url.is_empty();
    if configured_provider {
        emit(
            save_provider(&yaml_path, &cfg.base_url, &cfg.model, args.force)?,
            true,
        )?;
    }

    let mut stored_key = false;
    if let (Some(key), Some(var)) = (cfg.api_key.as_deref(), cfg.key_var) {
        emit(store_key(&env_path, var, key.trim(), gitignore)?, true)?;
        stored_key = true;
    }
    for (var, key) in &cfg.web_keys {
        if key.trim().is_empty() {
            continue;
        }
        emit(store_key(&env_path, var, key.trim(), gitignore)?, true)?;
    }

    let next = if !configured_provider {
        "Nothing to set up · run `aster init` again anytime".to_string()
    } else if !stored_key && key_status(&cfg.base_url).is_none() {
        no_key_hint(&cfg.base_url)
    } else if global_config {
        "You're set. cd into any repo and run: aster".to_string()
    } else {
        "You're set. Next: aster".to_string()
    };
    outro(next)?;
    Ok(())
}

/// What Aster resolves to right now. The wizard opens with it and starts the
/// cursor on it, so a second run is a change of mind rather than the whole
/// form typed again.
pub(crate) struct Current {
    base_url: String,
    model: String,
    /// False when nothing has been chosen yet and the values above are only
    /// the built-in defaults.
    pub(crate) configured: bool,
}

impl Current {
    pub(crate) fn read(repo_root: &Path) -> Self {
        // A malformed config is a thing init is here to fix, not a reason to
        // refuse to run.
        let settings = crate::settings::Settings::load(Some(repo_root)).unwrap_or_default();
        let configured = settings.review.base_url.is_some() || settings.review.model.is_some();
        let (base_url, model) = crate::config::provider::resolve_endpoint(&settings.review, None);
        Self {
            base_url,
            model,
            configured,
        }
    }

    /// The "here is what you already have" line the wizard opens with.
    fn summary(&self) -> String {
        if !self.configured {
            return "Nothing set up yet".to_string();
        }
        let key = match key_status(&self.base_url) {
            Some(var) => format!("key from {var}"),
            None => "no key yet".to_string(),
        };
        format!(
            "Now: {} · {} · {key}",
            provider_label(&self.base_url),
            self.model
        )
    }

    fn provider_hint(&self) -> String {
        match self.configured {
            true => format!("{} · {}", provider_label(&self.base_url), self.model),
            false => "the model Aster runs on".to_string(),
        }
    }

    /// True when `base_url` is the endpoint already in use, so its model is
    /// worth offering as a row rather than being a leftover from another one.
    fn serves(&self, base_url: &str) -> bool {
        self.configured && self.base_url.trim_end_matches('/') == base_url.trim_end_matches('/')
    }
}

fn env_set(var: &str) -> bool {
    env::var(var).is_ok_and(|v| !v.trim().is_empty())
}

/// The env var a key for this endpoint would be read from, when one is set.
/// Walks the same order as [`crate::config::provider::resolve_key`], so the wizard
/// reports the key the next turn would actually use.
fn key_status(base_url: &str) -> Option<&'static str> {
    keys::key_vars(base_url)
        .into_iter()
        .find(|var| env_set(var))
}

/// The var a key typed at the prompt belongs in. Endpoints with a var of their
/// own get it, so two providers each keep a key and switching back does not ask
/// for it again.
fn key_var_for(base_url: &str) -> &'static str {
    provider_key_vars(base_url)
        .first()
        .copied()
        .unwrap_or(keys::SHARED_KEY_VAR)
}

struct Configured {
    base_url: String,
    model: String,
    /// A key typed at the prompt; `None` leaves whatever is already stored.
    api_key: Option<String>,
    /// Where that key goes. `None` when the provider step was skipped.
    key_var: Option<&'static str>,
    /// Web tool keys typed at the prompt, as `(var, key)`.
    web_keys: Vec<(&'static str, String)>,
}

/// What the user opted into on the first screen; nothing is prompted for the rest.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Setup {
    Provider,
    WebTools,
}

/// Pick what to set up first, then only prompt for what was picked.
/// `None` when the user cancels.
async fn wizard(providers: &[Provider], current: &Current) -> Result<Option<Configured>> {
    let Some(picked) = or_cancel(
        multiselect("What do you want to set up? (space to toggle · enter to confirm)")
            .required(false)
            .initial_values(vec![Setup::Provider])
            .item(Setup::Provider, "Model provider", current.provider_hint())
            .item(Setup::WebTools, "Web tools", web_hint())
            .interact(),
    )?
    else {
        return Ok(None);
    };

    // Only what was picked is written: leaving the provider unticked must not
    // rewrite the endpoint already saved.
    let mut cfg = Configured {
        base_url: String::new(),
        model: String::new(),
        api_key: None,
        key_var: None,
        web_keys: Vec::new(),
    };
    if picked.is_empty() {
        return Ok(Some(cfg));
    }

    if picked.contains(&Setup::Provider) {
        let Some(chosen) = provider_setup(providers, current).await? else {
            return Ok(None);
        };
        cfg.base_url = chosen.base_url;
        cfg.model = chosen.model;
        cfg.api_key = chosen.api_key;
        cfg.key_var = Some(chosen.key_var);
    }

    if picked.contains(&Setup::WebTools) {
        cfg.web_keys = web_setup()?;
    }

    Ok(Some(cfg))
}

fn key_hint(set: bool, what: &str) -> String {
    match set {
        true => format!("{what} · key set"),
        false => what.to_string(),
    }
}

fn key_prompt(what: &str, set: bool) -> String {
    match set {
        true => format!("{what} (set · enter to keep, or type a new one)"),
        false => format!("{what} (enter to skip)"),
    }
}

/// What one web key is asked for. A provider needing two vars names each one,
/// since "Cloudflare API key" asked twice says nothing about which is which.
fn web_prompt_label(provider: &str, var: &str, vars: usize) -> String {
    match vars {
        1 => format!("{provider} API key"),
        _ => format!("{provider} · {var}"),
    }
}

/// How many web providers already hold a key, for the first screen.
fn web_hint() -> String {
    let providers = crate::config::key::web_providers();
    let set = providers
        .iter()
        .filter(|(_, vars)| crate::config::key::provider_is_set(vars))
        .count();
    match set {
        0 => "search and read the web".to_string(),
        n => format!("search and read the web · {n} of {} set", providers.len()),
    }
}

/// Pick which web providers to key, then ask for each one's vars. Driven by the
/// catalog aster-web resolves from, so a provider added there turns up here.
/// Escaping a prompt skips that key rather than discarding the answers given.
fn web_setup() -> Result<Vec<(&'static str, String)>> {
    let providers = crate::config::key::web_providers();
    let mut menu = multiselect::<usize>(
        "Which web tools? (space to toggle · enter to confirm · none is fine)",
    )
    .required(false);
    for (i, (name, vars)) in providers.iter().enumerate() {
        let buys = vars.first().map(|(_, buys)| *buys).unwrap_or_default();
        menu = menu.item(
            i,
            *name,
            key_hint(crate::config::key::provider_is_set(vars), buys),
        );
    }
    let Some(picked) = or_cancel(menu.interact())? else {
        return Ok(Vec::new());
    };

    let mut out = Vec::new();
    for i in picked {
        let (name, vars) = &providers[i];
        for (var, _) in vars {
            let what = web_prompt_label(name, var, vars.len());
            let set = env_set(var);
            let Some(key) = or_cancel(
                password(key_prompt(&what, set))
                    .mask('•')
                    .allow_empty()
                    .interact(),
            )?
            else {
                continue;
            };
            out.push((*var, key));
        }
    }
    Ok(out)
}

pub(crate) struct Chosen {
    pub(crate) base_url: String,
    pub(crate) model: String,
    pub(crate) api_key: Option<String>,
    pub(crate) key_var: &'static str,
}

/// Provider, base URL, key, model. The key is asked before the model so the
/// endpoint can be asked what it serves. `None` when the user cancels.
pub(crate) async fn provider_setup(
    providers: &[Provider],
    current: &Current,
) -> Result<Option<Chosen>> {
    // The catalog rows, then one for any endpoint the catalog does not know:
    // a self-hosted server, a proxy, anything OpenAI-compatible. An endpoint
    // in use that no row serves is a custom one, so the cursor starts there.
    let custom = providers.len();
    let catalog_at = providers.iter().position(|p| current.serves(&p.base_url));
    let at = catalog_at.unwrap_or(match current.configured {
        true => custom,
        false => 0,
    });
    let mut menu = select::<usize>("Which model provider? (type to search)")
        .initial_value(at)
        .filter_mode()
        .max_rows(8);
    for (i, p) in providers.iter().enumerate() {
        let hint = match i == at && current.configured {
            true => format!("{} · in use", p.base_url),
            false => p.base_url.clone(),
        };
        menu = menu.item(i, &p.name, hint);
    }
    let custom_hint = match at == custom && current.configured {
        true => format!("{} · in use", current.base_url),
        false => "any OpenAI-compatible base URL".to_string(),
    };
    menu = menu.item(custom, "Custom endpoint", custom_hint);
    let Some(idx) = or_cancel(menu.interact())? else {
        return Ok(None);
    };

    let custom_provider;
    let provider = match providers.get(idx) {
        Some(p) => p,
        None => {
            let Some(url) = custom_base_url(current, at == custom)? else {
                return Ok(None);
            };
            custom_provider = Provider {
                id: "custom".to_string(),
                name: "Custom endpoint".to_string(),
                base_url: url,
                example_model: String::new(),
                auth: String::new(),
            };
            &custom_provider
        }
    };

    let base_url = if provider.templated() {
        let Some(url) = or_cancel(
            cliclack::input("Base URL")
                .default_input(&provider.base_url)
                .required(false)
                .interact::<String>(),
        )?
        else {
            return Ok(None);
        };
        let url = url.trim();
        match url.is_empty() {
            true => provider.base_url.clone(),
            false => url.to_string(),
        }
    } else {
        provider.base_url.clone()
    };

    let key_var = key_var_for(&base_url);
    let has_key = key_status(&base_url).is_some();
    let prompt = match (has_key, provider.needs_key()) {
        (true, _) => format!("API key ({key_var} is set · enter to keep it)"),
        (false, true) => format!("API key, stored as {key_var} (enter to add later)"),
        (false, false) => "API key (usually none · enter to skip)".to_string(),
    };
    // Escaping the key prompt keeps the provider already chosen.
    let api_key = or_cancel(password(prompt).mask('•').allow_empty().interact())?
        .filter(|k| !k.trim().is_empty());

    let live = api_key
        .clone()
        .or_else(|| crate::config::provider::resolve_key(&base_url).map(|(key, _)| key));
    let Some(model) = pick_model(provider, &base_url, current, live.as_deref()).await? else {
        return Ok(None);
    };

    Ok(Some(Chosen {
        base_url,
        model: model.trim().to_string(),
        api_key,
        key_var,
    }))
}

/// The base URL for a custom endpoint. The one in use is offered back when it
/// is already a custom one, so re-running init edits it instead of retyping it.
fn custom_base_url(current: &Current, in_use: bool) -> Result<Option<String>> {
    let mut input = cliclack::input("Base URL, e.g. http://localhost:8080/v1");
    input = match in_use {
        true => input.default_input(&current.base_url).required(false),
        false => input.required(true),
    };
    let Some(url) = or_cancel(input.interact::<String>())? else {
        return Ok(None);
    };
    let url = url.trim();
    Ok(Some(match (url.is_empty(), in_use) {
        (true, true) => current.base_url.clone(),
        _ => url.to_string(),
    }))
}

/// The catalog's shortlist, the endpoint's own list, or a typed id. Both menus
/// filter as you type, and the model in use is where the cursor starts.
/// `None` when the user cancels.
async fn pick_model(
    provider: &Provider,
    base_url: &str,
    current: &Current,
    key: Option<&str>,
) -> Result<Option<String>> {
    let mut rows = provider_recommended(base_url);
    if current.serves(base_url) && !rows.contains(&current.model) {
        rows.insert(0, current.model.clone());
    }
    if rows.is_empty() && key.is_none() {
        return type_model(provider);
    }

    let search = rows.len();
    let typed = rows.len() + 1;
    let mut menu = select::<usize>("Model (type to search)")
        .initial_value(0)
        .filter_mode()
        .max_rows(10);
    for (i, m) in rows.iter().enumerate() {
        let hint = match current.serves(base_url) && *m == current.model {
            true => "in use",
            false => "",
        };
        menu = menu.item(i, m, hint);
    }
    if key.is_some() {
        menu = menu.item(
            search,
            "Search all models",
            "ask this endpoint what it serves",
        );
    }
    menu = menu.item(typed, "Something else", "type an id this endpoint serves");

    let Some(idx) = or_cancel(menu.interact())? else {
        return Ok(None);
    };
    if let Some(model) = rows.get(idx) {
        return Ok(Some(model.clone()));
    }
    if idx == search
        && let Some(key) = key
    {
        match search_models(base_url, key, current).await? {
            Search::Picked(model) => return Ok(Some(model)),
            Search::Cancelled => return Ok(None),
            // The endpoint could not be asked; the id can still be typed.
            Search::Unavailable => {}
        }
    }
    type_model(provider)
}

fn type_model(provider: &Provider) -> Result<Option<String>> {
    // No example model means nothing to fall back on, so empty is refused
    // rather than saved as a model id no endpoint serves.
    let Some(model) = or_cancel(
        cliclack::input("Model")
            .default_input(&provider.example_model)
            .required(provider.example_model.is_empty())
            .interact::<String>(),
    )?
    else {
        return Ok(None);
    };
    let model = model.trim();
    Ok(Some(match model.is_empty() {
        true => provider.example_model.clone(),
        false => model.to_string(),
    }))
}

enum Search {
    Picked(String),
    Cancelled,
    /// The endpoint could not be asked, which is not fatal: the shortlist and
    /// the free-text prompt are both still there.
    Unavailable,
}

/// Ask the endpoint for its whole catalog and filter it in place. This is the
/// only way to reach a model that is neither in the catalog's shortlist nor
/// already known by heart.
async fn search_models(base_url: &str, key: &str, current: &Current) -> Result<Search> {
    let spinner = cliclack::spinner();
    spinner.start("Asking the endpoint what it serves…");
    let client = AiClient::new(base_url.to_string(), key.to_string(), String::new());
    let models = match client.fetch_models().await {
        Ok(models) if !models.is_empty() => {
            spinner.stop(format!("{} models", models.len()));
            models
        }
        Ok(_) => {
            spinner.error("the endpoint listed no models");
            return Ok(Search::Unavailable);
        }
        Err(e) => {
            spinner.error(format!("could not list models: {e:#}"));
            return Ok(Search::Unavailable);
        }
    };

    let at = models
        .iter()
        .position(|m| current.serves(base_url) && *m == current.model)
        .unwrap_or(0);
    let mut menu = select::<usize>("Model (type to search)")
        .initial_value(at)
        .filter_mode()
        .max_rows(12);
    for (i, m) in models.iter().enumerate() {
        let hint = match i == at && current.serves(base_url) {
            true => "in use",
            false => "",
        };
        menu = menu.item(i, m, hint);
    }
    match or_cancel(menu.interact())? {
        Some(idx) => Ok(Search::Picked(models[idx].clone())),
        None => Ok(Search::Cancelled),
    }
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

fn no_key_hint(base_url: &str) -> String {
    format!(
        "No API key yet. Set {} in your shell, or run `aster init` again to store one.",
        key_var_for(base_url)
    )
}

fn finish_plain(global: bool, base_url: &str) {
    if key_status(base_url).is_none() {
        println!("  {DIM}{}{RESET}", no_key_hint(base_url));
        return;
    }
    if global {
        println!("  {DIM}You're set. cd into any repo and run:{RESET} aster");
    } else {
        println!("  {DIM}Next:{RESET} aster");
    }
}

/// Store the API key in `.env`, replacing any value already there: a key typed
/// at the prompt is the one the user wants from here on. When `gitignore` is
/// set, also keep the file out of git.
fn store_key(env_path: &Path, var_name: &str, key: &str, gitignore: bool) -> Result<Note> {
    let replaced = env_has_key(env_path, var_name);
    if let Some(parent) = env_path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    set_env_key(env_path, var_name, key)
        .with_context(|| format!("writing {}", env_path.display()))?;
    if gitignore && let Some(dir) = env_path.parent() {
        ensure_gitignored(dir, ".env")?;
    }
    let verb = if replaced { "Replaced" } else { "Stored" };
    Ok(Note::Success(format!(
        "{verb} {var_name} in {}",
        display(env_path)
    )))
}

/// Write the choice down. An existing config is edited in place, so re-running
/// init to switch provider switches it instead of being ignored; comments and
/// everything else in the file survive. `--force` rewrites the whole scaffold.
fn save_provider(path: &Path, base_url: &str, model: &str, force: bool) -> Result<Note> {
    if !path.exists() || force {
        return write_scaffold(path, base_url, model);
    }
    crate::settings::write_review(path, &[("base_url", base_url), ("model", model)])?;
    Ok(Note::Success(format!(
        "Updated {} · provider and model, nothing else touched",
        display(path)
    )))
}

/// The non-interactive path, which never overwrites: `-y` picks defaults the
/// user was never shown, and a config already there outranks a guess.
fn scaffold(path: &Path, base_url: &str, model: &str, force: bool) -> Result<Note> {
    if path.exists() && !force {
        return Ok(Note::Info(format!(
            "{} already exists · keeping it (--force rewrites it; `aster init` without -y switches provider in place)",
            display(path)
        )));
    }
    write_scaffold(path, base_url, model)
}

fn write_scaffold(path: &Path, base_url: &str, model: &str) -> Result<Note> {
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
         #\n\
         # Web search and page reading need no server and no key: they ship in\n\
         # the binary as `web/search` and `web/extract`.\n\
         mcp:\n\
         \x20 servers:\n\
         \x20   # Drive a real browser: navigate, click, type, and screenshot.\n\
         \x20   # Needs uv (https://docs.astral.sh/uv) and Python 3.11+, then\n\
         \x20   # `uvx browser-use install` once to fetch Chromium.\n\
         \x20   # It browses in its own profile under ~/.config/browseruse, not\n\
         \x20   # the Chrome you are signed into. Set `disabled: false` to enable.\n\
         \x20   browser:\n\
         \x20     command: uvx\n\
         \x20     args:\n\
         \x20       - \"--from\"\n\
         \x20       - \"browser-use\"\n\
         \x20       - \"browser-use\"\n\
         \x20       - \"--mcp\"\n\
         \x20     env:\n\
         \x20       # browser-use reports usage to its vendor unless this is set.\n\
         \x20       ANONYMIZED_TELEMETRY: \"False\"\n\
         \x20       # Without this the browser opens a window on your screen.\n\
         \x20       BROWSER_USE_HEADLESS: \"true\"\n\
         \x20       # Uncomment to confine the agent to named hosts.\n\
         \x20       # BROWSER_USE_ALLOWED_DOMAINS: \"example.com,docs.rs\"\n\
         \x20     disabled: true\n\n\
         \x20 # Turn individual tools off by their `server/tool` id. Globs work.\n\
         \x20 # Also settable with `aster mcp disable web/crawl`.\n\
         \x20 tools:\n\
         \x20   deny:\n\
         \x20     # These two need a second LLM API key, and the retry tool runs\n\
         \x20     # a whole agent loop inside one tool call.\n\
         \x20     - \"browser/browser_extract_content\"\n\
         \x20     - \"browser/retry_with_browser_use_agent\"\n"
    )
}

fn env_has_key(env_path: &Path, key: &str) -> bool {
    let Ok(text) = fs::read_to_string(env_path) else {
        return false;
    };
    text.lines().any(|l| assigns(l, key))
}

fn assigns(line: &str, key: &str) -> bool {
    line.trim_start()
        .strip_prefix(key)
        .is_some_and(|rest| rest.starts_with('='))
}

/// Set `KEY=value` in `.env`, dropping every earlier line that assigns the key:
/// the loader takes the last match, so a survivor would keep the old value in
/// effect.
pub(crate) fn set_env_key(path: &Path, key: &str, value: &str) -> Result<()> {
    let text = fs::read_to_string(path).unwrap_or_default();
    let mut lines: Vec<String> = text
        .lines()
        .map(str::to_string)
        .filter(|l| !assigns(l, key))
        .collect();
    lines.push(format!("{key}={value}"));
    let mut out = lines.join("\n");
    out.push('\n');
    write_secret(path, out.as_bytes())
}

/// Drop `KEY=...` from `.env`, keeping every other line. Returns whether the
/// key was there; a missing file is simply "no".
pub(crate) fn remove_env_key(path: &Path, key: &str) -> Result<bool> {
    let Ok(text) = fs::read_to_string(path) else {
        return Ok(false);
    };
    let kept: Vec<&str> = text.lines().filter(|l| !assigns(l, key)).collect();
    if kept.len() == text.lines().count() {
        return Ok(false);
    }
    let mut out = kept.join("\n");
    if !out.is_empty() {
        out.push('\n');
    }
    write_secret(path, out.as_bytes())?;
    Ok(true)
}

/// Write a file that holds a key, created 0o600 from the start so it is never
/// briefly world-readable; an existing wider file is narrowed the same way.
fn write_secret(path: &Path, bytes: &[u8]) -> Result<()> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
        file.write_all(bytes)?;
    }
    #[cfg(not(unix))]
    fs::write(path, bytes)?;
    Ok(())
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

pub(crate) fn ensure_gitignored(repo_root: &Path, entry: &str) -> Result<()> {
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
