//! `aster model` — list what the configured provider serves, and switch which
//! one every surface uses. `aster models` is the older spelling of the list.

use std::env;

use anyhow::{Context, Result};
use clap::{Args, Subcommand};

use crate::settings::Settings;

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
}

#[derive(Args)]
pub struct UseModelArgs {
    /// Model id, as shown by `aster model list`.
    #[arg(value_name = "ID")]
    id: String,
}

pub async fn run_model(args: ModelArgs) -> Result<()> {
    match args.command {
        ModelCmd::List(args) => run(args).await,
        ModelCmd::Use(args) => use_model(args),
        ModelCmd::Recommended => recommended(),
    }
}

/// Write the choice where every surface reads it, then report what the next
/// turn resolves to: the point is that this answer is the same everywhere.
fn use_model(args: UseModelArgs) -> Result<()> {
    let repo_root = env::current_dir().context("could not determine the current directory")?;
    let saved = crate::settings::persist_user_review(Some(&repo_root), &[("model", &args.id)])?;
    crate::provider::report(&repo_root, &saved, &["ASTER_MODEL"])
}

/// Catalog-backed, so it costs nothing and stays right when the endpoint moves.
fn recommended() -> Result<()> {
    let repo_root = env::current_dir().context("could not determine the current directory")?;
    let settings = Settings::load(Some(&repo_root))?;
    let (base_url, _) = crate::provider::resolve_endpoint(&settings.review, None);
    let models = crate::init::provider_recommended(&base_url);
    if crate::json_mode() {
        println!("{}", serde_json::to_string(&models)?);
        return Ok(());
    }
    for m in &models {
        println!("{m}");
    }
    Ok(())
}

/// `aster provider list`, sharing this module's renderer so the two spellings
/// cannot disagree about what the catalog holds.
pub fn list_providers_command() -> Result<()> {
    let repo_root = env::current_dir().context("could not determine the current directory")?;
    let settings = Settings::load(Some(&repo_root))?;
    let (base_url, _) = crate::provider::resolve_endpoint(&settings.review, None);
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
    let client = crate::provider::resolve_client(&settings, args.model.as_deref())?;
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
