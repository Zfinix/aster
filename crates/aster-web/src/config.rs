//! Web provider configuration: environment variables and optional `aster.yaml` overrides.

use std::env;

use serde::{Deserialize, Serialize};

/// Every env var the providers read, as `(provider, var, what it buys)`, in
/// dispatch order. Kept beside the `resolve_*` methods so a provider cannot
/// gain a key without `aster key` listing it.
pub const KEY_VARS: &[(&str, &str, &str)] = &[
    ("Exa", "EXA_API_KEY", "leads web/search"),
    ("Perplexity", "PERPLEXITY_API_KEY", "web/search, behind Exa"),
    (
        "Context.dev",
        "CONTEXT_DEV_API_KEY",
        "web/sitemap and web/screenshot, and leads extract and crawl",
    ),
    (
        "Firecrawl",
        "FIRECRAWL_API_KEY",
        "web/crawl, and lifts the keyless limit on search and extract",
    ),
    (
        "Browserbase",
        "BROWSERBASE_API_KEY",
        "browser-backed web/extract and web/search",
    ),
    (
        "Cloudflare Browser Rendering",
        "CLOUDFLARE_BR_ACCOUNT_ID",
        "web/extract and web/crawl, with the API token",
    ),
    (
        "Cloudflare Browser Rendering",
        "CLOUDFLARE_BR_API_TOKEN",
        "web/extract and web/crawl, with the account id",
    ),
    (
        "Jina Reader",
        "JINA_API_KEY",
        "a higher rate limit on keyless web/extract",
    ),
];

/// Configuration loaded from env vars at startup. `aster.yaml` overrides
/// can be layered on top when available.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WebConfig {
    #[serde(default)]
    pub defaults: WebDefaults,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebDefaults {
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,
}

fn default_timeout_ms() -> u64 {
    120_000
}

impl Default for WebDefaults {
    fn default() -> Self {
        Self {
            timeout_ms: default_timeout_ms(),
        }
    }
}

impl WebConfig {
    /// Callers may layer `aster.yaml` values on top of these afterward.
    pub fn from_env() -> Self {
        Self::default()
    }

    pub fn resolve_context_dev_key(&self) -> Option<String> {
        resolve_key(
            |name| env::var(name).ok(),
            &["CONTEXT_DEV_API_KEY", "CONTEXT_API_KEY"],
        )
    }

    pub fn resolve_firecrawl_key(&self) -> Option<String> {
        resolve_key(|name| env::var(name).ok(), &["FIRECRAWL_API_KEY"])
    }

    pub fn resolve_exa_key(&self) -> Option<String> {
        resolve_key(|name| env::var(name).ok(), &["EXA_API_KEY"])
    }

    pub fn resolve_perplexity_key(&self) -> Option<String> {
        resolve_key(|name| env::var(name).ok(), &["PERPLEXITY_API_KEY"])
    }

    pub fn resolve_jina_key(&self) -> Option<String> {
        resolve_key(|name| env::var(name).ok(), &["JINA_API_KEY"])
    }

    pub fn resolve_browserbase_key(&self) -> Option<String> {
        resolve_key(|name| env::var(name).ok(), &["BROWSERBASE_API_KEY"])
    }

    pub fn resolve_cloudflare_br_keys(&self) -> Option<(String, String)> {
        let account = env::var("CLOUDFLARE_BR_ACCOUNT_ID")
            .ok()
            .filter(|v| !v.trim().is_empty())?;
        let token = env::var("CLOUDFLARE_BR_API_TOKEN")
            .ok()
            .filter(|v| !v.trim().is_empty())?;
        Some((account, token))
    }
}

/// First non-blank value among `names`, in order. Split from the environment
/// lookup so tests exercise the precedence without mutating process-wide state.
fn resolve_key(lookup: impl Fn(&str) -> Option<String>, names: &[&str]) -> Option<String> {
    names
        .iter()
        .filter_map(|name| lookup(name))
        .find(|value| !value.trim().is_empty())
}

#[cfg(test)]
#[path = "tests/config_test.rs"]
mod tests;
