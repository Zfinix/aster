//! Shared LLM provider resolution. Precedence: non-empty shell env, then `aster.yaml`, then defaults.
//! API keys never come from the yaml.

use std::env;

use anyhow::{Result, bail};
use aster_ai::{AiClient, Effort};

use crate::settings::{Review, Settings};

pub struct LlmConfig {
    pub api_key: String,
    pub base_url: String,
    pub model: String,
    pub effort: Effort,
    pub web_search: bool,
}

/// Resolve endpoint, key, and model. `model_flag` wins over env and yaml; empty env counts as unset.
pub fn resolve(review: &Review, model_flag: Option<&str>) -> Result<LlmConfig> {
    let Some(api_key) =
        env_non_empty("ASTER_API_KEY").or_else(|| env_non_empty("OPEN_ROUTER_API_KEY"))
    else {
        bail!(
            "no API key found. Run `aster init` to set one up globally, or set ASTER_API_KEY (or OPEN_ROUTER_API_KEY) in your shell environment"
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
        effort: resolve_effort(review),
        web_search: resolve_web_search(review),
    })
}

/// `--effort` wins, then `ASTER_EFFORT`/`ASTER_REASONING_EFFORT`, then
/// aster.yaml, then the client default. An unparseable value is ignored rather
/// than fatal: a typo should not stop a run, but it is said out loud.
fn resolve_effort(review: &Review) -> Effort {
    crate::effort_flag()
        .or_else(|| {
            let raw = env_or("ASTER_EFFORT", None).or_else(|| env_or("ASTER_REASONING_EFFORT", None))?;
            match raw.parse() {
                Ok(effort) => Some(effort),
                Err(_) => {
                    eprintln!(
                        "note: ignoring effort {raw:?} from the environment; expected off, low, medium, or high"
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

fn env_non_empty(key: &str) -> Option<String> {
    env::var(key).ok().filter(|s| !s.trim().is_empty())
}

/// Build a configured `AiClient` from already-loaded settings.
pub fn resolve_client(settings: &Settings, model_override: Option<&str>) -> Result<AiClient> {
    let llm = resolve(&settings.review, model_override)?;
    Ok(AiClient::new(llm.base_url, llm.api_key, llm.model)
        .with_effort(llm.effort)
        .with_web_search(llm.web_search))
}

/// Shell env wins, then the aster.yaml value; None when neither is set.
pub fn env_or(key: &str, file: Option<&str>) -> Option<String> {
    env::var(key)
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| file.map(str::to_string))
}
