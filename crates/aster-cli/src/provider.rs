//! Shared LLM provider resolution for every subcommand that talks to a model.
//! One chain, applied identically to `review` and `chat`: shell env (non-empty)
//! wins, then `aster.yaml`, then defaults. API keys never come from the yaml.

use std::env;

use anyhow::{Result, bail};

use crate::settings::Review;

pub struct LlmConfig {
    pub api_key: String,
    pub base_url: String,
    pub model: String,
}

/// Resolve the endpoint, key, and model. `model_flag` (a CLI flag) wins over
/// env and yaml; an empty env var counts as unset.
pub fn resolve(review: &Review, model_flag: Option<&str>) -> Result<LlmConfig> {
    let Some(api_key) =
        env_non_empty("ASTER_API_KEY").or_else(|| env_non_empty("OPEN_ROUTER_API_KEY"))
    else {
        bail!(
            "no API key found; set ASTER_API_KEY (or OPEN_ROUTER_API_KEY) in your shell or the repo's .env (copy .env.example to get started)"
        );
    };
    let base_url = env_or("ASTER_BASE_URL", review.base_url.as_deref())
        .unwrap_or_else(|| "https://openrouter.ai/api/v1".to_string());
    let model = model_flag
        .map(str::to_string)
        .or_else(|| env_or("ASTER_MODEL", review.model.as_deref()))
        .unwrap_or_else(|| "openai/gpt-4o-mini".to_string());
    Ok(LlmConfig {
        api_key,
        base_url,
        model,
    })
}

fn env_non_empty(key: &str) -> Option<String> {
    env::var(key).ok().filter(|s| !s.trim().is_empty())
}

/// Resolve a setting without mutating the environment: shell env wins, then the
/// aster.yaml value. Returns None if neither is set, so callers apply defaults.
pub fn env_or(key: &str, file: Option<&str>) -> Option<String> {
    env::var(key)
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| file.map(str::to_string))
}
