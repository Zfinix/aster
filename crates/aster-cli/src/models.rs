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
}

pub async fn run(args: ModelsArgs) -> Result<()> {
    let repo_root = env::current_dir().context("could not determine the current directory")?;
    let settings = Settings::load(Some(&repo_root))?;
    let client = crate::provider::resolve_client(&settings, args.model.as_deref())?;
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
