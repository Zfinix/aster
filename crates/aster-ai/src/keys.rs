//! Which env var holds the key for an endpoint. Every surface that builds a
//! client resolves through here, so pointing Aster somewhere new picks up that
//! endpoint's key instead of the last one's.

use std::collections::HashMap;
use std::env;
use std::sync::OnceLock;

use serde::Deserialize;

const PROVIDERS_JSON: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../providers.json"));

pub const SHARED_KEY_VAR: &str = "ASTER_API_KEY";

/// Where the key for an endpoint came from, so a caller can say which one it
/// would use before spending a turn finding out.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KeySource {
    Provider,
    Shared,
    /// A server on this machine, which authenticates nothing.
    Local,
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
    #[serde(default)]
    name: String,
    base_url: String,
    #[serde(default)]
    key_env: Vec<String>,
}

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

/// Every var the catalog names, as `(provider name, var)`, first spelling
/// wins. For surfaces that list keys rather than resolve one endpoint's.
pub fn catalog_key_vars() -> &'static [(&'static str, &'static str)] {
    static VARS: OnceLock<Vec<(&'static str, &'static str)>> = OnceLock::new();
    VARS.get_or_init(|| {
        let Ok(catalog) = serde_json::from_str::<Catalog>(PROVIDERS_JSON) else {
            return Vec::new();
        };
        let mut out: Vec<(&'static str, &'static str)> = Vec::new();
        for entry in catalog.providers {
            let name: &'static str = Box::leak(entry.name.into_boxed_str());
            for var in entry.key_env {
                if out.iter().any(|(_, known)| *known == var) {
                    continue;
                }
                out.push((name, Box::leak(var.into_boxed_str())));
            }
        }
        out
    })
}

/// The key held by `base_url`'s own var, when one is set.
pub fn provider_key(base_url: &str) -> Option<String> {
    provider_key_vars(base_url)
        .iter()
        .copied()
        .find_map(env_non_empty)
}

/// The key for `base_url`: the endpoint's own var first, then the shared one.
/// Only the shared var crosses endpoints; a var named for one vendor is never
/// offered to another, which would fail as a bare 401.
pub fn resolve_key(base_url: &str) -> Option<(String, KeySource)> {
    // The Codex backend takes only the ChatGPT subscription login; an API key
    // sent there fails as an unexplained 401, so the shared var never crosses.
    if crate::codex_api::is_codex(base_url) {
        let home = crate::home_dir().ok()?;
        crate::codex::load(&home)?;
        return Some(("chatgpt-subscription".to_string(), KeySource::Provider));
    }
    if let Some(key) = provider_key(base_url) {
        return Some((key, KeySource::Provider));
    }
    if let Some(key) = env_non_empty(SHARED_KEY_VAR) {
        return Some((key, KeySource::Shared));
    }
    // A model server on this machine checks no credential, so asking for a key
    // would block the one setup that needs no account at all.
    is_loopback(base_url).then(|| (LOCAL_KEY.to_string(), KeySource::Local))
}

/// A placeholder bearer for endpoints that ignore it, so the request still has
/// the shape every other endpoint expects.
const LOCAL_KEY: &str = "local";

/// Whether `base_url` points at this machine, however it spells the host.
pub fn is_loopback(base_url: &str) -> bool {
    let host = host_only(base_url.trim_end_matches('/'));
    let host = match host.rsplit_once(':') {
        Some((before, port)) if !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) => {
            before
        }
        _ => host,
    };
    let host = host.trim_start_matches('[').trim_end_matches(']');
    host.eq_ignore_ascii_case("localhost")
        || host == "::1"
        || host
            .parse::<std::net::Ipv4Addr>()
            .is_ok_and(|ip| ip.is_loopback() || ip.is_unspecified())
}

struct ModelRow {
    base_url: String,
    shortlist: Vec<String>,
    example: String,
}

/// Where a refreshed model list is cached. Written by `aster provider refresh`,
/// read here. Only model ids live in it: base URLs and key vars stay in the
/// embedded catalog, so a bad file can never repoint an endpoint at someone
/// else's server and collect the key.
pub fn overlay_path() -> Option<std::path::PathBuf> {
    Some(
        crate::home_dir()
            .ok()?
            .join(".aster")
            .join("model-catalog.json"),
    )
}

/// The refreshed ids, by provider id. Absent, unreadable, or malformed all mean
/// the same thing: use what shipped in the binary.
fn overlay() -> HashMap<String, CatalogModels> {
    let Some(text) = overlay_path().and_then(|p| std::fs::read_to_string(p).ok()) else {
        return HashMap::new();
    };
    parse_overlay(&text)
}

/// Read a refreshed catalog. Anything the file carries beyond model ids is
/// dropped on the floor here: there is no field to receive it.
pub fn parse_overlay(text: &str) -> HashMap<String, CatalogModels> {
    serde_json::from_str::<Overlay>(text)
        .map(|o| o.models)
        .unwrap_or_default()
}

/// The two fields a refresh may carry. There is deliberately nowhere here to
/// put a base URL or a key var.
#[derive(Deserialize, Default, Clone)]
pub struct CatalogModels {
    #[serde(default)]
    pub example_model: Option<String>,
    #[serde(default)]
    pub recommended: Vec<String>,
}

#[derive(Deserialize)]
struct Overlay {
    #[serde(default)]
    models: HashMap<String, CatalogModels>,
}

/// A model id worth writing down: printable, one line, and short enough that a
/// picker row stays a picker row.
pub fn sane_model_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 200 && id.chars().all(|c| !c.is_control()) && id.trim() == id
}

fn model_rows() -> &'static [ModelRow] {
    static ROWS: OnceLock<Vec<ModelRow>> = OnceLock::new();
    ROWS.get_or_init(|| {
        #[derive(Deserialize)]
        struct Row {
            id: String,
            base_url: String,
            #[serde(default)]
            example_model: String,
            #[serde(default)]
            recommended: Vec<String>,
        }
        #[derive(Deserialize)]
        struct Rows {
            providers: Vec<Row>,
        }
        let Ok(catalog) = serde_json::from_str::<Rows>(PROVIDERS_JSON) else {
            return Vec::new();
        };
        let fresh = overlay();
        catalog
            .providers
            .into_iter()
            .map(|row| {
                let update = fresh.get(&row.id);
                let shortlist = match update.map(|u| &u.recommended) {
                    Some(ids) if !ids.is_empty() => {
                        ids.iter().filter(|id| sane_model_id(id)).cloned().collect()
                    }
                    _ => row.recommended,
                };
                let example = update
                    .and_then(|u| u.example_model.clone())
                    .filter(|id| sane_model_id(id))
                    .unwrap_or(row.example_model);
                ModelRow {
                    base_url: row.base_url.trim_end_matches('/').to_string(),
                    shortlist,
                    example,
                }
            })
            .collect()
    })
}

/// The catalog's shortlist for `base_url`: the exact endpoint's row first, then
/// the host's, so endpoints sharing a host keep their own list. Empty
/// off-catalog, which reads as "ask the endpoint".
pub fn catalog_models(base_url: &str) -> Vec<String> {
    match catalog_row(base_url) {
        Some(row) if !row.shortlist.is_empty() => row.shortlist.clone(),
        Some(row) if !row.example.is_empty() => vec![row.example.clone()],
        _ => Vec::new(),
    }
}

/// The catalog's curated coding shortlist for `base_url`, with no fall back to
/// the example model: a row without one has no shortlist to show, rather than
/// a list of one that claims to be vetted.
pub fn catalog_shortlist(base_url: &str) -> Vec<String> {
    catalog_row(base_url)
        .map(|row| row.shortlist.clone())
        .unwrap_or_default()
}

/// The catalog's starting model for `base_url`, with no fall back to the
/// shortlist: callers that want either already have `catalog_models`.
pub fn catalog_example(base_url: &str) -> Option<String> {
    catalog_row(base_url)
        .map(|row| row.example.clone())
        .filter(|id| !id.is_empty())
}

fn catalog_row(base_url: &str) -> Option<&'static ModelRow> {
    let want = base_url.trim_end_matches('/');
    let rows = model_rows();
    rows.iter().find(|row| row.base_url == want).or_else(|| {
        let host = host_only(want);
        rows.iter().find(|row| host_only(&row.base_url) == host)
    })
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
