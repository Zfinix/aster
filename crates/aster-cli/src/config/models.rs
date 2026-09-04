//! `aster model` — list what the configured provider serves, and switch which
//! one every surface uses. `aster models` is the older spelling of the list.

use std::env;

use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand};

use crate::settings::Settings;

#[derive(Args)]
pub struct KeysArgs {
    /// Include every model endpoint, not only the ones holding a key.
    #[arg(long)]
    pub(crate) all: bool,
}

#[derive(Args)]
pub struct ModelsArgs {
    /// Override the model in aster.yaml; only affects which provider is resolved
    /// when the config picks one by model name.
    #[arg(long, value_name = "ID")]
    model: Option<String>,

    /// List the endpoints Aster knows instead of one endpoint's models, marking
    /// the one this repo is pointed at.
    #[arg(long)]
    providers: bool,

    /// Report what each model accepts rather than its ID alone. Kept behind a
    /// flag so the plain list stays the array of strings callers parse.
    #[arg(long)]
    capabilities: bool,
}

#[derive(Args)]
pub struct ModelArgs {
    #[command(subcommand)]
    command: ModelCmd,
}

#[derive(Subcommand)]
enum ModelCmd {
    /// What the configured endpoint serves.
    List(ModelsArgs),
    /// Switch the model, saved to aster.yaml so every surface picks it up.
    Use(UseModelArgs),
    /// The catalog's shortlist for the endpoint in use, for a picker to show
    /// before the endpoint has been asked.
    Recommended,
    /// What the live benchmark router would pick per tier (cheap, balanced,
    /// strong) from OpenRouter's rankings. This is what `model: auto` uses.
    Router(RouterArgs),
}

#[derive(Args)]
pub struct RouterArgs {
    /// Only show one tier's pick: cheap, balanced, or strong.
    #[arg(long, value_name = "TIER")]
    pub(crate) tier: Option<String>,
}

#[derive(Args)]
pub struct UseModelArgs {
    /// Model id, as shown by `aster model list`.
    #[arg(value_name = "ID")]
    pub(crate) id: String,
}

pub async fn run_model(args: ModelArgs) -> Result<()> {
    match args.command {
        ModelCmd::List(args) => run(args).await,
        ModelCmd::Use(args) => use_model(args),
        ModelCmd::Recommended => recommended(),
        ModelCmd::Router(args) => router_command(args),
    }
}

/// Live picks from OpenRouter's benchmark data, the same resolution
/// `model: auto` runs. One fetch serves every tier and warms the cache.
fn router_command(args: RouterArgs) -> Result<()> {
    use aster_ai::router::{self, Tier};
    let repo_root = env::current_dir().context("could not determine the current directory")?;
    let settings = Settings::load(Some(&repo_root))?;
    let (base_url, _) = super::provider::resolve_endpoint(&settings.review, None);
    if !super::provider::is_openrouter(&base_url) {
        bail!(
            "the model router reads OpenRouter's rankings; point base_url at OpenRouter to use it"
        );
    }
    let Some((api_key, _)) = aster_ai::keys::resolve_key(&base_url) else {
        bail!("no OpenRouter key found. Run `aster login openrouter`, then try again");
    };
    let cache = match aster_ai::home_dir() {
        Ok(home) => router::cache_path(&home),
        Err(_) => std::env::temp_dir().join("aster-model-rankings.json"),
    };
    let picks = router::recommend(&api_key, &cache)?;
    let wanted: Option<Tier> = args.tier.as_deref().and_then(Tier::parse);
    if args.tier.is_some() && wanted.is_none() {
        bail!(
            "unknown tier {:?}; expected cheap, balanced, or strong",
            args.tier
        );
    }
    if crate::json_mode() {
        let shown: Vec<_> = picks
            .iter()
            .filter(|p| wanted.is_none_or(|t| p.tier == t))
            .collect();
        println!("{}", serde_json::to_string(&shown)?);
        return Ok(());
    }
    for pick in picks.iter().filter(|p| wanted.is_none_or(|t| p.tier == t)) {
        println!(
            "{:<9} {}  coding {}  agentic {}  ${:.2}/M",
            pick.tier.as_str(),
            pick.model,
            pick.coding_index,
            pick.agentic_index,
            pick.blended_price_per_m
        );
    }
    Ok(())
}

/// Write the choice where every surface reads it, then report what the next
/// turn resolves to: the point is that this answer is the same everywhere.
pub(crate) fn use_model(args: UseModelArgs) -> Result<()> {
    let repo_root = env::current_dir().context("could not determine the current directory")?;
    let saved = crate::settings::persist_user_review(Some(&repo_root), &[("model", &args.id)])?;
    super::provider::report(&repo_root, &saved, &["ASTER_MODEL"])
}

/// OpenRouter answers from the live benchmark router, so the shortlist tracks
/// the rankings instead of a hand-written snapshot. Other endpoints keep the
/// catalog's shortlist, which costs nothing and stays right when they move.
fn recommended() -> Result<()> {
    let repo_root = env::current_dir().context("could not determine the current directory")?;
    let settings = Settings::load(Some(&repo_root))?;
    let (base_url, _) = super::provider::resolve_endpoint(&settings.review, None);
    let models = match router_recommended(&base_url) {
        Some(models) => models,
        None => crate::init::provider_recommended(&base_url),
    };
    if crate::json_mode() {
        println!("{}", serde_json::to_string(&models)?);
        return Ok(());
    }
    for m in &models {
        println!("{m}");
    }
    Ok(())
}

/// The router's picks for `base_url`, or `None` when the endpoint is not
/// OpenRouter, no key is set, or the rankings cannot be fetched: callers fall
/// back to the catalog rather than failing the command.
fn router_recommended(base_url: &str) -> Option<Vec<String>> {
    use aster_ai::router;
    if !super::provider::is_openrouter(base_url) {
        return None;
    }
    let (api_key, _) = aster_ai::keys::resolve_key(base_url)?;
    let cache = match aster_ai::home_dir() {
        Ok(home) => router::cache_path(&home),
        Err(_) => std::env::temp_dir().join("aster-model-rankings.json"),
    };
    let picks = router::recommend(&api_key, &cache).ok()?;
    Some(picks.into_iter().map(|p| p.model).collect())
}

/// `aster provider list`, sharing this module's renderer so the two spellings
/// cannot disagree about what the catalog holds.
pub fn list_providers_command() -> Result<()> {
    let repo_root = env::current_dir().context("could not determine the current directory")?;
    let settings = Settings::load(Some(&repo_root))?;
    let (base_url, _) = super::provider::resolve_endpoint(&settings.review, None);
    list_providers(&base_url)
}

pub async fn run(args: ModelsArgs) -> Result<()> {
    // The catalog is embedded, so listing it is answered before a key is
    // resolved: `aster init` itself needs this list to run.
    if args.providers {
        return list_providers_command();
    }
    let repo_root = env::current_dir().context("could not determine the current directory")?;
    let settings = Settings::load(Some(&repo_root))?;
    let client = super::provider::resolve_client(&settings, args.model.as_deref())?;
    if args.capabilities {
        return list_capabilities(&client).await;
    }
    let models = client.fetch_models().await?;
    if crate::json_mode() {
        println!("{}", serde_json::to_string(&models)?);
    } else {
        for m in &models {
            println!("{m}");
        }
    }
    Ok(())
}

/// `images: null` is the endpoint saying nothing about its modalities, which
/// Aster reads as "try it"; `false` is the endpoint ruling images out.
async fn list_capabilities(client: &aster_ai::AiClient) -> Result<()> {
    let mut catalog = client.fetch_model_catalog().await?;
    catalog.sort_by(|a, b| a.id.cmp(&b.id));
    if crate::json_mode() {
        let rows: Vec<_> = catalog
            .iter()
            .map(|m| serde_json::json!({ "id": m.id, "images": m.takes_images }))
            .collect();
        println!("{}", serde_json::to_string(&rows)?);
    } else {
        for m in &catalog {
            let images = match m.takes_images {
                Some(true) => "images",
                Some(false) => "text only",
                None => "unknown",
            };
            println!("{}\t{images}", m.id);
        }
    }
    Ok(())
}

/// The provider catalog, with the endpoint currently in use marked. Templated
/// entries are left out: they name a host the user still has to fill in.
fn list_providers(current: &str) -> Result<()> {
    let current = current.trim_end_matches('/');
    let providers: Vec<_> = crate::init::provider_choices()
        .into_iter()
        .map(|(name, base_url, example_model)| {
            let is_current = base_url.trim_end_matches('/') == current;
            serde_json::json!({
                "name": name,
                // The vars a front-end should look in before falling back to
                // the shared key, so the mapping lives in one place.
                "key_env": crate::init::provider_key_vars(&base_url),
                "base_url": base_url,
                "example_model": example_model,
                // The shortlist a picker can show before this endpoint has been
                // asked what it serves, so no front-end has to hardcode one.
                "recommended": crate::init::provider_recommended(&base_url),
                "current": is_current,
            })
        })
        .collect();

    if crate::json_mode() {
        println!(
            "{}",
            serde_json::json!({ "ok": true, "providers": providers })
        );
        return Ok(());
    }
    for p in &providers {
        let mark = if p["current"] == true { "*" } else { " " };
        println!(
            "{mark} {:<16} {}",
            p["name"].as_str().unwrap_or_default(),
            p["base_url"].as_str().unwrap_or_default()
        );
    }
    Ok(())
}
