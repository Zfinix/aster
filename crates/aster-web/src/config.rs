//! Web provider configuration: environment variables and optional `aster.yaml` overrides.

use std::env;

use serde::{Deserialize, Serialize};

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
    /// Read only the parts that come from environment variables.
    /// Callers may layer `aster.yaml` values on top afterward.
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
mod tests {
    use super::*;

    #[test]
    fn defaults_are_sensible() {
        let config = WebConfig::default();
        assert_eq!(config.defaults.timeout_ms, 120_000);
    }

    fn env_of(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> + use<> {
        let pairs: Vec<(String, String)> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |name| {
            pairs
                .iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.clone())
        }
    }

    #[test]
    fn resolve_key_prefers_first_var() {
        let lookup = env_of(&[
            ("CONTEXT_DEV_API_KEY", "ctxt_secret_dev"),
            ("CONTEXT_API_KEY", "ctxt_secret_fallback"),
        ]);
        assert_eq!(
            resolve_key(lookup, &["CONTEXT_DEV_API_KEY", "CONTEXT_API_KEY"]).as_deref(),
            Some("ctxt_secret_dev")
        );
    }

    #[test]
    fn resolve_key_falls_back_to_second_var() {
        let lookup = env_of(&[("CONTEXT_API_KEY", "ctxt_secret_fallback")]);
        assert_eq!(
            resolve_key(lookup, &["CONTEXT_DEV_API_KEY", "CONTEXT_API_KEY"]).as_deref(),
            Some("ctxt_secret_fallback")
        );
    }

    #[test]
    fn resolve_key_skips_blank() {
        let lookup = env_of(&[
            ("CONTEXT_DEV_API_KEY", "   "),
            ("CONTEXT_API_KEY", "ctxt_secret_fallback"),
        ]);
        assert_eq!(
            resolve_key(lookup, &["CONTEXT_DEV_API_KEY", "CONTEXT_API_KEY"]).as_deref(),
            Some("ctxt_secret_fallback")
        );
    }

    #[test]
    fn resolve_key_returns_none_when_none_set() {
        assert!(resolve_key(env_of(&[]), &["FIRECRAWL_API_KEY"]).is_none());
    }
}
