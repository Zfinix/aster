//! `aster models` — list the model IDs the configured provider serves.

use std::env;

use anyhow::{Context, Result};
use clap::Args;

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

pub async fn run(args: ModelsArgs) -> Result<()> {
    let repo_root = env::current_dir().context("could not determine the current directory")?;
    let settings = Settings::load(Some(&repo_root))?;
    let client = crate::provider::resolve_client(&settings, args.model.as_deref())?;
    if args.providers {
        return list_providers(client.base_url());
    }
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
