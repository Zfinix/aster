//! Tool listings cached per folder, so a session's first prompt does not wait
//! for every MCP server to spawn, handshake, and list. An entry is keyed on a
//! fingerprint of the server configs, so editing aster.yaml invalidates it
//! without a TTL; a server that changes its own list is caught when the live
//! connect rewrites the cache.

use std::collections::BTreeMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};

use aster_mcp::McpTool;
use serde::{Deserialize, Serialize};

use crate::mcp::{McpSettings, ServerConfig, Transport};

/// The protocol era a server spoke last time. A cached `Legacy` skips the
/// startup probe, which pre-2026 servers pay in full every session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum CachedEra {
    Modern { version: String },
    Legacy,
}

pub(crate) struct CacheHit {
    pub tools: Vec<McpTool>,
    pub eras: BTreeMap<String, CachedEra>,
}

#[derive(Serialize, Deserialize)]
struct Entry {
    fingerprint: u64,
    tools: Vec<McpTool>,
    eras: BTreeMap<String, CachedEra>,
}

/// Everything about a server that decides its tool list: the command, its
/// arguments and environment, the endpoint, and whether it is enabled.
pub(crate) fn fingerprint(servers: &BTreeMap<String, ServerConfig>) -> u64 {
    let mut hasher = DefaultHasher::new();
    for (name, config) in servers {
        name.hash(&mut hasher);
        config.command.hash(&mut hasher);
        config.args.hash(&mut hasher);
        for (key, value) in &config.env {
            key.hash(&mut hasher);
            value.hash(&mut hasher);
        }
        config.cwd.hash(&mut hasher);
        config.url.hash(&mut hasher);
        for (key, value) in &config.headers {
            key.hash(&mut hasher);
            value.hash(&mut hasher);
        }
        config.kind.map(Transport::label).hash(&mut hasher);
        config.disabled.hash(&mut hasher);
    }
    hasher.finish()
}

/// One cache file per folder, named by the repo root so parallel sessions in
/// different projects never contend for the same entry.
fn cache_path(repo_root: &Path) -> Option<PathBuf> {
    let dir = crate::persist::home().ok()?.join("mcp-cache");
    let mut hasher = DefaultHasher::new();
    repo_root.hash(&mut hasher);
    Some(dir.join(format!("{:016x}.json", hasher.finish())))
}

pub(crate) fn load(repo_root: &Path, settings: &McpSettings) -> Option<CacheHit> {
    load_at(&cache_path(repo_root)?, settings)
}

pub(crate) fn save(
    repo_root: &Path,
    settings: &McpSettings,
    tools: Vec<McpTool>,
    eras: BTreeMap<String, CachedEra>,
) {
    if let Some(path) = cache_path(repo_root) {
        save_at(&path, settings, tools, eras);
    }
}

fn load_at(path: &Path, settings: &McpSettings) -> Option<CacheHit> {
    let raw = std::fs::read(path).ok()?;
    let entry: Entry = serde_json::from_slice(&raw).ok()?;
    if entry.fingerprint != fingerprint(&settings.servers) {
        return None;
    }
    Some(CacheHit {
        tools: entry.tools,
        eras: entry.eras,
    })
}

fn save_at(
    path: &Path,
    settings: &McpSettings,
    tools: Vec<McpTool>,
    eras: BTreeMap<String, CachedEra>,
) {
    let entry = Entry {
        fingerprint: fingerprint(&settings.servers),
        tools,
        eras,
    };
    let Ok(bytes) = serde_json::to_vec(&entry) else {
        return;
    };
    let write = || -> std::io::Result<()> {
        let dir = path
            .parent()
            .ok_or_else(|| std::io::Error::other("the cache path has no parent directory"))?;
        std::fs::create_dir_all(dir)?;
        std::fs::write(path, bytes)
    };
    if write().is_err() {
        tracing::debug!("could not write the MCP tool cache");
    }
}

#[cfg(test)]
#[path = "tests/mcp_cache_test.rs"]
mod tests;
