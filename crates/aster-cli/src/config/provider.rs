//! LLM provider resolution and the `aster provider` command. Precedence: shell env, then `aster.yaml`, then defaults.
//! API keys never come from yaml.

use std::env;
use std::fmt;
use std::path::Path;

use anyhow::{Context, Result, bail};
use aster_ai::keys::{self, env_non_empty};
use aster_ai::{AiClient, DEFAULT_BASE_URL, Effort};
use clap::{Args, Subcommand};
use serde::Serialize;

use crate::settings::{Agent, Review, Saved, Settings};

/// Data needed for user authentication, sent as `setup` on stream error or in `aster config key --json`.
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

    /// Returns Some(Setup) if credentials are needed for this endpoint.
    pub fn needed(base_url: &str) -> Option<Self> {
        resolve_key(base_url)
            .is_none()
            .then(|| Self::for_endpoint(base_url))
    }
}

/// Signals a missing key and no login for the endpoint.
#[derive(Debug)]
pub struct MissingCredentials(pub Setup);

/// List of supported browser logins.
pub const LOGINS: [(&str, &str); 2] = [
    ("openrouter", "one account, most models"),
    ("codex", "use a ChatGPT Plus or Pro subscription"),
];

impl fmt::Display for MissingCredentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let setup = &self.0;
        let mut lines = vec![match setup.login {
            Some("codex") => "not signed in to ChatGPT yet. Ways in:".to_string(),
            _ => format!("no key for {} yet. Ways in:", setup.provider),
        }];
        lines.push(
            "  aster init                 pick a provider, or a model on this machine".to_string(),
        );
        let mut logins = LOGINS;
        logins.sort_by_key(|(target, _)| Some(*target) != setup.login);
        for (target, what) in logins {
            lines.push(format!("  aster login {target:<14} {what}"));
        }
        if let Some(var) = setup.key_vars.first() {
            let others = match setup.key_vars.len() {
                1 => String::new(),
                _ => format!("   (or {})", setup.key_vars[1..].join(", ")),
            };
            lines.push(format!("  export {var}=…{others}"));
        }
        write!(f, "{}", lines.join("\n"))
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

/// Get chosen endpoint and model, no key required.
pub fn resolve_endpoint(review: &Review, model_flag: Option<&str>) -> (String, String) {
    let base_url = env_or("ASTER_BASE_URL", review.base_url.as_deref())
        .unwrap_or_else(|| DEFAULT_BASE_URL.to_string());
    let model = model_flag
        .map(str::to_string)
        .or_else(|| env_or("ASTER_MODEL", review.model.as_deref()))
        .unwrap_or_else(|| DEFAULT_MODEL.to_string());
    (base_url, model)
}

/// Get endpoint, key, model. `model_flag` takes highest priority.
/// If model is "auto" and endpoint is openrouter, call the router to resolve.
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

pub(crate) fn resolve_effort(review: &Review) -> Effort {
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

/// The output cap for one request. `0`, `none` or `off` lifts it and lets the
/// provider decide, which some accounts refuse outright, so it stays opt-in.
pub(crate) fn resolve_max_tokens(agent: &Agent) -> Option<u32> {
    max_tokens_from(agent, env_or("ASTER_MAX_TOKENS", None))
}

fn max_tokens_from(agent: &Agent, env: Option<String>) -> Option<u32> {
    let configured = match env {
        Some(raw) => match raw.trim() {
            "0" | "none" | "off" => return None,
            raw => match raw.parse() {
                Ok(cap) => Some(cap),
                Err(_) => {
                    eprintln!("note: ignoring max tokens {raw:?} from the environment");
                    agent.max_output_tokens
                }
            },
        },
        None => agent.max_output_tokens,
    };
    match configured {
        Some(0) => None,
        Some(cap) => Some(cap),
        None => Some(aster_ai::DEFAULT_MAX_TOKENS),
    }
}

fn resolve_web_search(review: &Review) -> bool {
    env_or("ASTER_WEB_SEARCH", None)
        .map(|v| matches!(v.trim(), "1" | "true" | "yes" | "on"))
        .unwrap_or(review.web_search.unwrap_or(false))
}

#[cfg(test)]
#[path = "../tests/provider_test.rs"]
mod tests;

const ASTER_HTTP_REFERER: &str = "https://withaster.dev";
const ASTER_TITLE: &str = "Aster";

pub fn resolve_client(settings: &Settings, model_override: Option<&str>) -> Result<AiClient> {
    let llm = resolve(&settings.review, model_override)?;
    let client = AiClient::new(llm.base_url, llm.api_key, llm.model)
        .with_effort(llm.effort)
        .with_web_search(llm.web_search)
        .with_max_tokens(resolve_max_tokens(&settings.agent));
    // Only attribute if endpoint is openrouter.
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

/// Returns shell value or config value for `key`, or None.
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
    /// List known endpoints.
    List,
    /// Use given provider and optionally set its model.
    Use(UseProviderArgs),
    /// Pull refreshed model ids from `providers.catalog_url`.
    Refresh,
    /// Ask every endpoint you hold a key for what it serves, and print a
    /// catalog file with the dead ids dropped.
    Probe(ProbeArgs),
}

#[derive(Args)]
pub struct ProbeArgs {
    /// Write the file here instead of printing it.
    #[arg(long, value_name = "PATH")]
    out: Option<std::path::PathBuf>,
}

#[derive(Args)]
pub struct UseProviderArgs {
    /// Provider id, name, or base URL.
    #[arg(value_name = "PROVIDER")]
    pub(crate) target: String,

    /// Model to use (if not given, the provider's example model is used).
    #[arg(long, value_name = "ID")]
    pub(crate) model: Option<String>,
}

pub async fn run(args: ProviderArgs) -> Result<()> {
    match args.command {
        ProviderCmd::List => super::models::list_providers_command(),
        ProviderCmd::Use(args) => use_provider(args),
        ProviderCmd::Refresh => refresh().await,
        ProviderCmd::Probe(args) => probe(args).await,
    }
}

/// Where the catalog cache lives, and the only thing a fetch may change: model
/// ids. Endpoints and key vars are compiled in and never read from here.
fn catalog_url(settings: &Settings) -> Option<String> {
    env_or(
        "ASTER_CATALOG_URL",
        settings.providers.catalog_url.as_deref(),
    )
}

async fn refresh() -> Result<()> {
    let repo_root = env::current_dir().context("could not determine the current directory")?;
    let settings = Settings::load(Some(&repo_root))?;
    let Some(url) = catalog_url(&settings) else {
        bail!(
            "no model list to pull from. Point one at a URL first:\n  aster config set providers.catalog_url <url>"
        );
    };
    let Some(path) = keys::overlay_path() else {
        bail!("could not resolve the home directory to cache the list in");
    };

    let client = reqwest::Client::builder()
        .user_agent(concat!("aster/", env!("CARGO_PKG_VERSION")))
        .build()?;
    let text = client
        .get(&url)
        .timeout(std::time::Duration::from_secs(20))
        .send()
        .await
        .with_context(|| format!("fetching {url}"))?
        .error_for_status()
        .with_context(|| format!("fetching {url}"))?
        .text()
        .await?;

    let fetched: Catalog = serde_json::from_str(&text)
        .with_context(|| format!("{url} is not a model list Aster understands"))?;
    let (rows, ids) = (fetched.models.len(), fetched.total_ids());
    if rows == 0 {
        bail!("{url} lists no providers; leaving the current list in place");
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&path, serde_json::to_string_pretty(&fetched)? + "\n")?;

    if crate::json_mode() {
        println!(
            "{}",
            serde_json::json!({ "ok": true, "providers": rows, "models": ids,
                               "url": url, "cached": path.display().to_string() })
        );
        return Ok(());
    }
    println!(
        "{ids} model ids across {rows} provider(s) · cached in {}",
        path.display()
    );
    Ok(())
}

/// The catalog file, both halves of the trip: what `probe` writes and what
/// `refresh` accepts. Model ids only, by provider id.
#[derive(Debug, Default, Serialize, serde::Deserialize)]
struct Catalog {
    #[serde(default)]
    generated: String,
    /// Carried in the file so a copy found on its own still explains itself.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    note: String,
    #[serde(default)]
    models: std::collections::BTreeMap<String, CatalogModels>,
}

const CATALOG_NOTE: &str = "Model ids only, by provider id. Endpoints and key vars live in \
     providers.json and are never read from here. Regenerate with `aster provider probe --out \
     model-catalog.json`; publish it where `providers.catalog_url` points.";

impl Catalog {
    fn total_ids(&self) -> usize {
        self.models.values().map(|m| m.recommended.len()).sum()
    }
}

#[derive(Debug, Default, Serialize, serde::Deserialize)]
struct CatalogModels {
    #[serde(skip_serializing_if = "Option::is_none")]
    example_model: Option<String>,
    #[serde(default)]
    recommended: Vec<String>,
}

/// Ask each endpoint holding its own key what it serves, and drop the ids that
/// have gone. Never invents a shortlist: an endpoint gaining a model is a
/// judgement call, an endpoint losing one is a fact, and only the fact is
/// automated.
async fn probe(args: ProbeArgs) -> Result<()> {
    let mut out = Catalog {
        generated: chrono::Utc::now().date_naive().to_string(),
        note: CATALOG_NOTE.to_string(),
        models: Default::default(),
    };
    let mut notes: Vec<String> = Vec::new();

    for (id, base_url) in crate::init::provider_base_urls() {
        if keys::is_loopback(&base_url) {
            continue;
        }
        // Only endpoints holding a key of their own. The shared key answers for
        // every provider, and probing on it would hand one key to thirty
        // different companies to learn nothing.
        let Some((key, KeySource::Provider)) = resolve_key(&base_url) else {
            continue;
        };
        let shortlist = keys::catalog_shortlist(&base_url);
        let example = keys::catalog_example(&base_url).unwrap_or_default();
        let client = AiClient::new(base_url.clone(), key, example.clone());
        let served = match client.fetch_models().await {
            Ok(models) if !models.is_empty() => models,
            Ok(_) => {
                notes.push(format!("{id:<16} lists nothing; left alone"));
                continue;
            }
            Err(e) => {
                notes.push(format!("{id:<16} {}", first_line(&format!("{e:#}"))));
                continue;
            }
        };
        let kept: Vec<String> = shortlist
            .iter()
            .filter(|id| served.contains(id))
            .cloned()
            .collect();
        let dropped = shortlist.len() - kept.len();
        notes.push(format!(
            "{id:<16} {} served · {} kept{}",
            served.len(),
            kept.len(),
            match dropped {
                0 => String::new(),
                n => format!(" · {n} gone"),
            }
        ));
        out.models.insert(
            id,
            CatalogModels {
                example_model: match served.contains(&example) {
                    true => Some(example),
                    false => served.first().cloned(),
                },
                recommended: kept,
            },
        );
    }

    let text = serde_json::to_string_pretty(&out)? + "\n";
    match &args.out {
        Some(path) => {
            std::fs::write(path, &text)?;
            eprintln!("{}", notes.join("\n"));
            eprintln!("\nwrote {}", path.display());
        }
        None => {
            eprintln!("{}", notes.join("\n"));
            println!("{text}");
        }
    }
    Ok(())
}

fn first_line(text: &str) -> String {
    text.lines().next().unwrap_or_default().to_string()
}

/// Point endpoint and model together, then show effect.
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

/// Print what will be used next, showing any shell overrides.
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
        Some(KeySource::Local) => "local",
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
