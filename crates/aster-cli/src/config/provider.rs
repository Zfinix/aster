//! Shared LLM provider resolution, and the `aster provider` command that writes
//! the choice down. Precedence: non-empty shell env, then `aster.yaml`, then
//! defaults. API keys never come from the yaml.

use std::env;
use std::fmt;
use std::path::Path;

use anyhow::{Context, Result, bail};
use aster_ai::keys::{self, env_non_empty};
use aster_ai::{AiClient, DEFAULT_BASE_URL, Effort};
use clap::{Args, Subcommand};
use serde::Serialize;

use crate::settings::{Review, Saved, Settings};

/// What a front-end needs to get the user signed in: the browser login that
/// applies, if any, and the env vars a key could go in. Travels as `setup` on
/// a stream error and in `aster config key --json`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Setup {
    pub provider: String,
    pub base_url: String,
    pub login: Option<&'static str>,
    pub key_vars: Vec<&'static str>,
}

impl Setup {
    pub fn for_endpoint(base_url: &str) -> Self {
        let codex = aster_ai::codex_api::is_codex(base_url);
        let login = if codex {
            Some("codex")
        } else if is_openrouter(base_url) {
            Some("openrouter")
        } else if base_url.contains("z.ai") {
            Some("zai")
        } else {
            None
        };
        Self {
            provider: crate::init::provider_label(base_url),
            base_url: base_url.to_string(),
            login,
            key_vars: if codex {
                Vec::new()
            } else {
                keys::key_vars(base_url)
            },
        }
    }

    /// None when the endpoint already has a key or a login.
    pub fn needed(base_url: &str) -> Option<Self> {
        resolve_key(base_url)
            .is_none()
            .then(|| Self::for_endpoint(base_url))
    }
}

/// No key and no login for the endpoint. Typed so `--stream` can hand the
/// front-end the setup it needs instead of a bare exit.
#[derive(Debug)]
pub struct MissingCredentials(pub Setup);

impl fmt::Display for MissingCredentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let setup = &self.0;
        if setup.login == Some("codex") {
            return write!(
                f,
                "not signed in to ChatGPT. Run `aster login codex` to link your subscription, then try again"
            );
        }
        let sign_in = match setup.login {
            Some(target) => format!(", or run `aster login {target}`"),
            None => String::new(),
        };
        write!(
            f,
            "no API key found for {}. Run `aster init` to set one up globally{sign_in}, or set {} in your shell environment",
            setup.provider,
            setup.key_vars.join(" or ")
        )
    }
}

impl std::error::Error for MissingCredentials {}

pub struct LlmConfig {
    pub api_key: String,
    pub base_url: String,
    pub model: String,
    pub effort: Effort,
    pub web_search: bool,
}

const DEFAULT_MODEL: &str = "openai/gpt-4o-mini";

pub use aster_ai::keys::{KeySource, resolve_key};

/// Endpoint and model alone, on the same precedence as [`resolve`] but without
/// needing a key, so a saved choice can be read back before one is set.
pub fn resolve_endpoint(review: &Review, model_flag: Option<&str>) -> (String, String) {
    let base_url = env_or("ASTER_BASE_URL", review.base_url.as_deref())
        .unwrap_or_else(|| DEFAULT_BASE_URL.to_string());
    let model = model_flag
        .map(str::to_string)
        .or_else(|| env_or("ASTER_MODEL", review.model.as_deref()))
        .unwrap_or_else(|| DEFAULT_MODEL.to_string());
    (base_url, model)
}

/// Resolve endpoint, key, and model. `model_flag` wins over env and yaml; empty env counts as unset.
/// The value `auto` routes through OpenRouter's live benchmark data instead of a pinned id.
pub fn resolve(review: &Review, model_flag: Option<&str>) -> Result<LlmConfig> {
    let (base_url, model) = resolve_endpoint(review, model_flag);
    let Some((api_key, _)) = resolve_key(&base_url) else {
        return Err(MissingCredentials(Setup::for_endpoint(&base_url)).into());
    };
    let model = if model == aster_ai::router::AUTO_MODEL && is_openrouter(&base_url) {
        resolve_auto_model(&api_key)
    } else {
        model
    };
    Ok(LlmConfig {
        api_key,
        base_url,
        model,
        effort: resolve_effort(review),
        web_search: resolve_web_search(review),
    })
}

/// `auto` resolves through OpenRouter's benchmark rankings. The tier comes from
/// `ASTER_ROUTER_TIER` (cheap | balanced | strong, default balanced). Any
/// failure falls back to the built-in default with a note, never a hard error.
fn resolve_auto_model(api_key: &str) -> String {
    use aster_ai::router::{self, Tier};
    let tier = env_or("ASTER_ROUTER_TIER", None)
        .and_then(|raw| {
            let parsed = Tier::parse(&raw);
            if parsed.is_none() {
                eprintln!(
                    "note: ignoring ASTER_ROUTER_TIER={raw}; expected cheap, balanced, or strong"
                );
            }
            parsed
        })
        .unwrap_or(Tier::Balanced);
    let cache = match aster_ai::home_dir() {
        Ok(home) => router::cache_path(&home),
        Err(_) => std::env::temp_dir().join("aster-model-rankings.json"),
    };
    match router::resolve_auto(api_key, tier, &cache, DEFAULT_MODEL) {
        Ok(pick) if pick.model != DEFAULT_MODEL || pick.from_cache => {
            eprintln!(
                "note: router picked {} ({}, coding {}, ${:.2}/M{})",
                pick.model,
                tier.as_str(),
                pick.coding_index,
                pick.blended_price_per_m,
                if pick.from_cache { ", cached" } else { "" }
            );
            pick.model
        }
        Ok(pick) => {
            eprintln!(
                "note: model router unavailable, using the default {}; set a pinned model to avoid this",
                pick.model
            );
            pick.model
        }
        Err(e) => {
            eprintln!("note: model router failed ({e:#}); using {DEFAULT_MODEL}");
            DEFAULT_MODEL.to_string()
        }
    }
}

/// `--effort` wins, then `ASTER_EFFORT`/`ASTER_REASONING_EFFORT`, then
/// aster.yaml, then the client default. An unparseable value is ignored rather
/// than fatal: a typo should not stop a run, but it is said out loud.
fn resolve_effort(review: &Review) -> Effort {
    crate::effort_flag()
        .or_else(|| {
            let raw =
                env_or("ASTER_EFFORT", None).or_else(|| env_or("ASTER_REASONING_EFFORT", None))?;
            match raw.parse() {
                Ok(effort) => Some(effort),
                Err(_) => {
                    let expected = Effort::ALL
                        .iter()
                        .copied()
                        .map(Effort::as_str)
                        .collect::<Vec<_>>()
                        .join(", ");
                    eprintln!(
                        "note: ignoring effort {raw:?} from the environment; expected {expected}"
                    );
                    None
                }
            }
        })
        .or(review.effort)
        .unwrap_or_default()
}

/// `ASTER_WEB_SEARCH` wins, then aster.yaml, then off: a search the turn did not
/// ask for spends money and drags unrelated pages into the context.
fn resolve_web_search(review: &Review) -> bool {
    env_or("ASTER_WEB_SEARCH", None)
        .map(|v| matches!(v.trim(), "1" | "true" | "yes" | "on"))
        .unwrap_or(review.web_search.unwrap_or(false))
}

// Attribution headers for OpenRouter analytics and display name.
const ASTER_HTTP_REFERER: &str = "https://withaster.dev";
const ASTER_TITLE: &str = "Aster";

pub fn resolve_client(settings: &Settings, model_override: Option<&str>) -> Result<AiClient> {
    let llm = resolve(&settings.review, model_override)?;
    let client = AiClient::new(llm.base_url, llm.api_key, llm.model)
        .with_effort(llm.effort)
        .with_web_search(llm.web_search);
    // Only attribute on OpenRouter-routed traffic; other providers ignore or
    // reject unknown headers, and the referer would leak aster's origin for no
    // gain.
    if is_openrouter(client.base_url()) {
        return Ok(client.with_attribution_headers([
            ("HTTP-Referer".to_string(), ASTER_HTTP_REFERER.to_string()),
            ("X-OpenRouter-Title".to_string(), ASTER_TITLE.to_string()),
        ]));
    }
    Ok(client)
}

pub(crate) fn is_openrouter(base_url: &str) -> bool {
    base_url
        .trim_end_matches('/')
        .split_once("//")
        .map(|(_, host)| host)
        .unwrap_or(base_url)
        .contains("openrouter")
}

/// Shell env wins, then the aster.yaml value; None when neither is set.
pub fn env_or(key: &str, file: Option<&str>) -> Option<String> {
    env::var(key)
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| file.map(str::to_string))
}

#[derive(Args)]
pub struct ProviderArgs {
    #[command(subcommand)]
    command: ProviderCmd,
}

#[derive(Subcommand)]
enum ProviderCmd {
    /// List the endpoints Aster knows, marking the one in use.
    List,
    /// Point Aster at an endpoint and adopt a model it serves. Saved to
    /// aster.yaml, so every surface picks it up.
    Use(UseProviderArgs),
}

#[derive(Args)]
pub struct UseProviderArgs {
    /// Provider id, name, or base URL, as shown by `aster provider list`.
    #[arg(value_name = "PROVIDER")]
    pub(crate) target: String,

    /// Model to adopt with it. Defaults to the provider's example model,
    /// since an endpoint kept with a model it does not serve fails next turn.
    #[arg(long, value_name = "ID")]
    pub(crate) model: Option<String>,
}

pub fn run(args: ProviderArgs) -> Result<()> {
    match args.command {
        ProviderCmd::List => super::models::list_providers_command(),
        ProviderCmd::Use(args) => use_provider(args),
    }
}

/// Repoint the endpoint and its model together, the way the TUI's `/provider`
/// does, then report what the next turn resolves to.
pub(crate) fn use_provider(args: UseProviderArgs) -> Result<()> {
    let repo_root = env::current_dir().context("could not determine the current directory")?;
    let (name, base_url, example_model) = crate::init::find_provider(&args.target)?;
    let model = args.model.unwrap_or(example_model);
    if model.is_empty() {
        bail!("{name} has no example model in the catalog; pass --model <ID>");
    }

    let saved = crate::settings::persist_user_review(
        Some(&repo_root),
        &[("base_url", &base_url), ("model", &model)],
    )?;
    report(&repo_root, &saved, &["ASTER_BASE_URL", "ASTER_MODEL"])
}

/// What the next turn would run with, read back through the same resolution
/// every command uses, plus the env vars that would override what was just
/// written. Silence there would be the bug this command exists to fix.
pub(crate) fn report(repo_root: &Path, saved: &Saved, watch: &[&str]) -> Result<()> {
    let settings = Settings::load(Some(repo_root))?;
    let (base_url, model) = resolve_endpoint(&settings.review, None);
    let shadowed: Vec<&str> = watch
        .iter()
        .copied()
        .filter(|key| env_non_empty(key).is_some())
        .collect();
    let key_env = crate::init::provider_key_vars(&base_url);
    let source = resolve_key(&base_url).map(|(_, source)| source);
    let key_source = match source {
        Some(KeySource::Provider) => "provider",
        Some(KeySource::Shared) => "shared",
        None => "none",
    };

    if crate::json_mode() {
        println!(
            "{}",
            serde_json::json!({
                "ok": true,
                "model": model,
                "provider": crate::init::provider_label(&base_url),
                "base_url": base_url,
                "config": saved.path.display().to_string(),
                "also": saved.also.as_ref().map(|p| p.display().to_string()),
                "key_env": key_env,
                "has_key": source.is_some(),
                "key_source": key_source,
                "shadowed_by_env": shadowed,
            })
        );
        return Ok(());
    }

    println!("provider {}", crate::init::provider_label(&base_url));
    println!("model    {model}");
    println!("saved to {}", saved.path.display());
    if let Some(also) = &saved.also {
        println!("         {} (this repo pinned it too)", also.display());
    }
    match source {
        None if aster_ai::codex_api::is_codex(&base_url) => {
            eprintln!("note: not signed in to ChatGPT; run `aster login codex`");
        }
        None => {
            eprintln!(
                "note: no key for this endpoint; set {}, or run `aster init`",
                keys::key_vars(&base_url).join(" or ")
            );
        }
        // A key meant for the last endpoint is usually rejected by this one, so
        // the fallback is worth naming before the next turn fails on it.
        Some(KeySource::Shared) if !key_env.is_empty() => {
            eprintln!(
                "note: no {} set; using {} for this endpoint",
                key_env.join(" or "),
                keys::SHARED_KEY_VAR
            );
        }
        Some(_) => {}
    }
    for key in shadowed {
        eprintln!("note: {key} is set in this shell and outranks the saved value");
    }
    Ok(())
}
