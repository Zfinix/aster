use std::fs;
use std::path::{Path, PathBuf};

use crate::def::{AGENT_FILE, AgentDef, AgentSource, parse_agent_md};

// Built-in agents. User definitions with the same name override these.
const BUILTINS: &[(&str, &str)] = &[
    ("scout", include_str!("../builtins/scout/AGENT.md")),
    (
        "cartographer",
        include_str!("../builtins/cartographer/AGENT.md"),
    ),
    ("sentinel", include_str!("../builtins/sentinel/AGENT.md")),
    ("forge", include_str!("../builtins/forge/AGENT.md")),
    ("scribe", include_str!("../builtins/scribe/AGENT.md")),
    ("prism", include_str!("../builtins/prism/AGENT.md")),
];

// Registry of agents, deduped by name.
#[derive(Debug, Default, Clone)]
pub struct AgentRegistry {
    agents: Vec<AgentDef>,
}

impl AgentRegistry {
    // Search each root for agents, merging built-ins last. Malformed or unreadable agents are skipped.
    pub fn discover(roots: &[PathBuf]) -> Self {
        let mut agents: Vec<AgentDef> = Vec::new();
        for root in roots {
            for agent in scan_root(root) {
                push_unless_shadowed(&mut agents, agent);
            }
        }
        for (dir_name, raw) in BUILTINS {
            match parse_agent_md(raw, dir_name, AgentSource::BuiltIn(raw)) {
                Ok(agent) => push_unless_shadowed(&mut agents, agent),
                Err(e) => tracing::error!(agent = %dir_name, "built-in agent is invalid: {e:#}"),
            }
        }
        agents.sort_by(|a, b| a.name.cmp(&b.name));
        Self { agents }
    }

    pub fn is_empty(&self) -> bool {
        self.agents.is_empty()
    }

    pub fn len(&self) -> usize {
        self.agents.len()
    }

    pub fn get(&self, name: &str) -> Option<&AgentDef> {
        self.agents.iter().find(|a| a.name == name)
    }

    pub fn iter(&self) -> impl Iterator<Item = &AgentDef> {
        self.agents.iter()
    }

    // Returns a markdown index of agents, grouped by category, or None if empty.
    pub fn render_index(&self) -> Option<String> {
        if self.agents.is_empty() {
            return None;
        }
        let mut out = String::from(
            "## Agents\n\n\
            Agents are named specialists you can delegate a self-contained task \
            to with the `agent` tool. Each starts with a fresh context and cannot \
            see this conversation, so the task you give it must contain everything \
            it needs. Delegate when a sub-task is substantial and separable; \
            otherwise work directly. When the work is divisible, split it into \
            distinct, non-overlapping slices and dispatch them as one `agent` call \
            with one task per slice, so the batch covers the whole; do not hand \
            the whole job to a single agent when it splits cleanly. When the user \
            asks you to spin up or use agents, always fan out this way, even for \
            work you could do yourself. For broad work, fan several cheap agents \
            out in one `agent` call, then pass their raw reports to `prism` in a \
            second call. Treat an agent's report as findings to weigh, not \
            verified truth.\n",
        );
        let mut categories: Vec<&str> = Vec::new();
        for agent in &self.agents {
            let cat = agent.category.as_deref().unwrap_or("other");
            if !categories.contains(&cat) {
                categories.push(cat);
            }
        }
        for cat in categories {
            out.push_str(&format!("\n### {cat}\n"));
            for agent in &self.agents {
                if agent.category.as_deref().unwrap_or("other") == cat {
                    out.push_str(&format!("\n- **{}**: {}", agent.name, agent.description));
                }
            }
        }
        Some(out)
    }
}

fn push_unless_shadowed(agents: &mut Vec<AgentDef>, agent: AgentDef) {
    if agents.iter().any(|a| a.name == agent.name) {
        tracing::debug!(
            agent = %agent.name,
            "shadowed by a higher-precedence agent of the same name"
        );
        return;
    }
    agents.push(agent);
}

// Load agents from a root's subdirectories. Returns empty if root is missing or unreadable.
fn scan_root(root: &Path) -> Vec<AgentDef> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    let mut agents = Vec::new();
    for entry in entries.filter_map(|e| e.ok()) {
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        let manifest = dir.join(AGENT_FILE);
        if !manifest.is_file() {
            continue;
        }
        let dir_name = dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let raw = match fs::read_to_string(&manifest) {
            Ok(raw) => raw,
            Err(e) => {
                tracing::warn!(path = %manifest.display(), "skipping agent: {e:#}");
                continue;
            }
        };
        match parse_agent_md(&raw, &dir_name, AgentSource::File(manifest.clone())) {
            Ok(agent) => agents.push(agent),
            Err(e) => tracing::warn!(path = %manifest.display(), "skipping agent: {e:#}"),
        }
    }
    agents
}

#[cfg(test)]
#[path = "tests/registry_test.rs"]
mod tests;
