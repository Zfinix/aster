#![forbid(unsafe_code)]
//! Agent Plugins packages: a `plugin.json` manifest at the root, Agent Skills under
//! `skills/`, MCP servers in `mcp.json`. Only a bad manifest rejects the whole
//! plugin. <https://github.com/agentplugins/agent-plugins-spec>

pub mod manifest;
pub mod mcp;
mod path;

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

pub use manifest::{Author, Manifest, PLUGIN_SCHEMA};
pub use mcp::{Http, MCP_SCHEMA, Server, Stdio, Transport};

pub const SPEC_VERSION: &str = "1.0.0";

pub const MANIFEST_FILE: &str = "plugin.json";
pub const MCP_FILE: &str = "mcp.json";
pub const SKILLS_DIR: &str = "skills";
pub const SKILL_FILE: &str = "SKILL.md";

/// One loaded plugin. `warnings` holds everything that was reported and skipped
/// rather than fatal, so a caller can surface it without re-reading the package.
#[derive(Debug, Clone)]
pub struct Plugin {
    pub manifest: Manifest,
    pub root: PathBuf,
    pub data_dir: PathBuf,
    pub skills: Vec<PathBuf>,
    pub servers: Vec<Server>,
    pub warnings: Vec<String>,
}

impl Plugin {
    pub fn name(&self) -> &str {
        &self.manifest.name
    }

    pub fn stdio_servers(&self) -> impl Iterator<Item = (&str, &Stdio)> {
        self.servers
            .iter()
            .filter_map(|server| match &server.transport {
                Transport::Stdio(stdio) => Some((server.name.as_str(), stdio)),
                Transport::Http(_) => None,
            })
    }
}

/// Load the plugin rooted at `root`, with its data directory under `data_root`.
/// An invalid manifest is fatal; anything else is a warning on the result.
pub fn load(root: &Path, data_root: &Path) -> Result<Plugin> {
    let root =
        fs::canonicalize(root).with_context(|| format!("reading plugin at {}", root.display()))?;
    let manifest_path = root.join(MANIFEST_FILE);
    if !manifest_path.is_file() || !path::contained(&root, &manifest_path) {
        bail!("no {MANIFEST_FILE} at {}", root.display());
    }

    let mut warnings = Vec::new();
    let text = fs::read_to_string(&manifest_path)
        .with_context(|| format!("reading {}", manifest_path.display()))?;
    let manifest = manifest::parse(&text, &mut warnings)
        .with_context(|| format!("invalid {}", manifest_path.display()))?;

    let data_dir = data_root.join(&manifest.name);
    let skills = skills(&root, &mut warnings);
    let servers = servers(&root, &data_dir, &mut warnings);

    Ok(Plugin {
        manifest,
        root,
        data_dir,
        skills,
        servers,
        warnings,
    })
}

/// Load every plugin installed directly under `root`. A directory without a
/// manifest is not a plugin and is passed over silently.
pub fn discover(root: &Path, data_root: &Path) -> (Vec<Plugin>, Vec<String>) {
    let Ok(entries) = fs::read_dir(root) else {
        return (Vec::new(), Vec::new());
    };
    let mut dirs: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.join(MANIFEST_FILE).is_file())
        .collect();
    dirs.sort();

    let mut plugins = Vec::new();
    let mut problems = Vec::new();
    for dir in dirs {
        match load(&dir, data_root) {
            Ok(plugin) => plugins.push(plugin),
            Err(e) => problems.push(format!("{}: {e:#}", dir.display())),
        }
    }
    plugins.sort_by(|a, b| a.manifest.name.cmp(&b.manifest.name));
    (plugins, problems)
}

/// Plugin roots inside `dir`: the directory itself, its immediate children, or
/// the children of a `plugins/` directory, which is how repositories that carry
/// several plugins lay them out.
pub fn candidates(dir: &Path) -> Vec<PathBuf> {
    if dir.join(MANIFEST_FILE).is_file() {
        return vec![dir.to_path_buf()];
    }
    let mut found: Vec<PathBuf> = [dir.to_path_buf(), dir.join("plugins")]
        .iter()
        .filter_map(|parent| fs::read_dir(parent).ok())
        .flatten()
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.join(MANIFEST_FILE).is_file())
        .collect();
    found.sort();
    found.dedup();
    found
}

fn skills(root: &Path, warnings: &mut Vec<String>) -> Vec<PathBuf> {
    let dir = root.join(SKILLS_DIR);
    if !dir.exists() {
        return Vec::new();
    }
    if !dir.is_dir() {
        warnings.push(format!("ignoring `{SKILLS_DIR}`: it is not a directory"));
        return Vec::new();
    }
    let Ok(entries) = fs::read_dir(&dir) else {
        warnings.push(format!("could not read {}", dir.display()));
        return Vec::new();
    };
    let mut skills = Vec::new();
    for entry in entries.filter_map(|entry| entry.ok()) {
        let skill = entry.path();
        let manifest = skill.join(SKILL_FILE);
        if !skill.is_dir() || !manifest.is_file() {
            continue;
        }
        if !path::contained(root, &manifest) {
            warnings.push(format!(
                "skipping skill `{}`: its {SKILL_FILE} resolves outside the plugin",
                entry.file_name().to_string_lossy()
            ));
            continue;
        }
        skills.push(skill);
    }
    skills.sort();
    skills
}

fn servers(root: &Path, data_dir: &Path, warnings: &mut Vec<String>) -> Vec<Server> {
    let config = root.join(MCP_FILE);
    if !config.exists() {
        return Vec::new();
    }
    if !config.is_file() || !path::contained(root, &config) {
        warnings.push(format!("ignoring `{MCP_FILE}`: it is not a regular file"));
        return Vec::new();
    }
    let text = match fs::read_to_string(&config) {
        Ok(text) => text,
        Err(e) => {
            warnings.push(format!("MCP disabled: could not read {MCP_FILE} ({e})"));
            return Vec::new();
        }
    };
    match mcp::parse(&text, root, data_dir, warnings) {
        Ok(servers) => servers,
        Err(e) => {
            warnings.push(format!("MCP disabled: invalid {MCP_FILE} ({e:#})"));
            Vec::new()
        }
    }
}

#[cfg(test)]
#[path = "tests/lib_test.rs"]
mod tests;
