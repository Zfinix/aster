//! Which env var holds the key for an endpoint. Every surface that builds a
//! client resolves through here, so pointing Aster somewhere new picks up that
//! endpoint's key instead of the last one's.

use std::collections::HashMap;
use std::env;
use std::sync::OnceLock;

use serde::Deserialize;

/// The shared catalog, embedded at build time. Every endpoint Aster ships with
/// names the var holding its key, so a key set once survives switching away and
/// back.
const PROVIDERS_JSON: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../providers.json"));

/// The var used when the endpoint has no var of its own, or that var is unset.
pub const SHARED_KEY_VAR: &str = "ASTER_API_KEY";

/// Where the key for an endpoint came from, so a caller can say which one it
/// would use before spending a turn finding out.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KeySource {
    /// A var named for this endpoint, e.g. `ANTHROPIC_API_KEY`.
    Provider,
    /// The shared [`SHARED_KEY_VAR`].
    Shared,
}

/// The env vars that may hold `base_url`'s own key, in the order they are
/// tried. Empty when the endpoint has only the shared var to fall back on,
/// which is the case for self-hosted servers and anything off the catalog.
pub fn provider_key_vars(base_url: &str) -> &'static [&'static str] {
    let host = host_only(base_url.trim_end_matches('/'));
    let table = table();
    if let Some(vars) = table.exact.get(host) {
        return vars;
    }
    table
        .templated
        .iter()
        .find(|(segments, _)| segments.iter().all(|s| host.contains(s.as_str())))
        .map(|(_, vars)| *vars)
        .unwrap_or(&[])
}

#[derive(Deserialize)]
struct Catalog {
    providers: Vec<CatalogEntry>,
}

#[derive(Deserialize)]
struct CatalogEntry {
    base_url: String,
    /// Absent for servers that take no key, e.g. Ollama.
    #[serde(default)]
    key_env: Vec<String>,
}

/// Hosts resolved once and kept for the process. Templated hosts such as
/// `{resource}.openai.azure.com` cannot be matched whole, so their literal
/// parts are kept and every one has to appear.
struct KeyTable {
    exact: HashMap<String, &'static [&'static str]>,
    templated: Vec<(Vec<String>, &'static [&'static str])>,
}

fn table() -> &'static KeyTable {
    static TABLE: OnceLock<KeyTable> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut exact = HashMap::new();
        let mut templated = Vec::new();
        let Ok(catalog) = serde_json::from_str::<Catalog>(PROVIDERS_JSON) else {
            return KeyTable { exact, templated };
        };
        for entry in catalog.providers {
            if entry.key_env.is_empty() {
                continue;
            }
            let vars: &'static [&'static str] = Box::leak(
                entry
                    .key_env
                    .into_iter()
                    .map(|var| &*Box::leak(var.into_boxed_str()))
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
            );
            let host = host_only(entry.base_url.trim_end_matches('/'));
            if !host.contains('{') {
                exact.insert(host.to_string(), vars);
                continue;
            }
            let segments = literal_segments(host);
            if !segments.is_empty() {
                templated.push((segments, vars));
            }
        }
        KeyTable { exact, templated }
    })
}

/// The parts of a templated host either side of its `{placeholder}`, so
/// `bedrock-runtime.{region}.amazonaws.com` still matches a real region.
fn literal_segments(host: &str) -> Vec<String> {
    let mut segments = Vec::new();
    let mut rest = host;
    while let Some(open) = rest.find('{') {
        let (before, after) = rest.split_at(open);
        if !before.is_empty() {
            segments.push(before.to_string());
        }
        rest = match after.find('}') {
            Some(close) => &after[close + 1..],
            None => "",
        };
    }
    if !rest.is_empty() {
        segments.push(rest.to_string());
    }
    segments
}

/// The key held by `base_url`'s own var, when one is set.
pub fn provider_key(base_url: &str) -> Option<String> {
    provider_key_vars(base_url)
        .iter()
        .copied()
        .find_map(env_non_empty)
}

/// The key for `base_url`: the endpoint's own var first, then the shared one.
/// The specific var wins so a saved provider switch picks up a key that is
/// already exported. Only the shared var crosses endpoints: a var named for one
/// vendor is never offered to another, which would fail as a bare 401.
pub fn resolve_key(base_url: &str) -> Option<(String, KeySource)> {
    match provider_key(base_url) {
        Some(key) => Some((key, KeySource::Provider)),
        None => Some((env_non_empty(SHARED_KEY_VAR)?, KeySource::Shared)),
    }
}

/// Every var a key for `base_url` could come from, most specific first, for an
/// error that names them all.
pub fn key_vars(base_url: &str) -> Vec<&'static str> {
    let mut vars = provider_key_vars(base_url).to_vec();
    vars.push(SHARED_KEY_VAR);
    vars
}

/// An exported but blank var is a half-finished setup, not a key, so it counts
/// as unset and the next candidate gets a turn.
pub fn env_non_empty(var: &str) -> Option<String> {
    env::var(var).ok().filter(|v| !v.trim().is_empty())
}

fn host_only(url: &str) -> &str {
    url.split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(url)
        .split('/')
        .next()
        .unwrap_or(url)
}

#[cfg(test)]
#[path = "tests/keys_test.rs"]
mod tests;
