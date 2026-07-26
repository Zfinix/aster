use std::fs;
use std::path::{Path, PathBuf};

use crate::def::{AGENT_FILE, AgentDef, AgentSource, parse_agent_md};

/// Compiled-in agents, parsed through the same path as user files. A user
/// definition of the same name shadows the built-in.
const BUILTINS: &[(&str, &str)] = &[
    ("explorer", include_str!("../builtins/explorer/AGENT.md")),
    ("reviewer", include_str!("../builtins/reviewer/AGENT.md")),
    ("fixer", include_str!("../builtins/fixer/AGENT.md")),
];

/// Agents discovered across roots plus built-ins, deduped by name.
#[derive(Debug, Default, Clone)]
pub struct AgentRegistry {
    agents: Vec<AgentDef>,
}

impl AgentRegistry {
    /// Scan each root's immediate subdirectories for an `AGENT.md`. First name
    /// wins, so pass the project root before the global one; built-ins are
    /// merged last. Unreadable roots and malformed agents are skipped with a
    /// warning, not fatal.
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

    /// The system-prompt block listing every agent by name and description, or
    /// `None` when none exist. The model delegates with the `agent` tool.
    pub fn render_index(&self) -> Option<String> {
        if self.agents.is_empty() {
            return None;
        }
        let mut out = String::from(
            "## Agents\n\n\
            Agents are specialists you can delegate a self-contained task to with \
            the `agent` tool. Each starts with a fresh context and cannot see this \
            conversation, so the task you give it must contain everything it needs. \
            Delegate when a sub-task is substantial and separable; otherwise work \
            directly. Treat an agent's report as findings to weigh, not verified \
            truth.\n",
        );
        for agent in &self.agents {
            out.push_str(&format!("\n- **{}**: {}", agent.name, agent.description));
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

/// Read one root's subdirectories into agents. A missing or unreadable root is
/// an empty result, not an error.
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
mod tests {
    use super::*;

    fn write_agent(root: &Path, name: &str, body: &str) {
        let dir = root.join(name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(AGENT_FILE), body).unwrap();
    }

    #[test]
    fn builtins_present_and_valid() {
        let registry = AgentRegistry::discover(&[]);
        for name in ["explorer", "reviewer", "fixer"] {
            let agent = registry.get(name).unwrap();
            assert!(agent.is_builtin());
            assert!(!agent.load_body().unwrap().is_empty());
        }
        assert!(registry.get("reviewer").unwrap().verify);
        assert!(
            registry
                .get("fixer")
                .unwrap()
                .tools
                .as_deref()
                .unwrap()
                .contains(&"edit_file".to_string())
        );
    }

    #[test]
    fn user_agent_shadows_builtin() {
        let tmp = tempfile::tempdir().unwrap();
        write_agent(
            tmp.path(),
            "explorer",
            "---\ndescription: project override.\n---\nbe brief",
        );
        let registry = AgentRegistry::discover(&[tmp.path().to_path_buf()]);
        let explorer = registry.get("explorer").unwrap();
        assert!(!explorer.is_builtin());
        assert_eq!(explorer.description, "project override.");
    }

    #[test]
    fn project_root_shadows_global() {
        let project = tempfile::tempdir().unwrap();
        let global = tempfile::tempdir().unwrap();
        write_agent(
            project.path(),
            "dup",
            "---\ndescription: project version.\n---\np",
        );
        write_agent(
            global.path(),
            "dup",
            "---\ndescription: global version.\n---\ng",
        );
        let registry =
            AgentRegistry::discover(&[project.path().to_path_buf(), global.path().to_path_buf()]);
        assert_eq!(registry.get("dup").unwrap().description, "project version.");
    }

    #[test]
    fn malformed_agent_skipped() {
        let tmp = tempfile::tempdir().unwrap();
        write_agent(
            tmp.path(),
            "broken",
            "---\nname: broken\n---\nno description",
        );
        let registry = AgentRegistry::discover(&[tmp.path().to_path_buf()]);
        assert!(registry.get("broken").is_none());
    }

    #[test]
    fn index_lists_agents() {
        let registry = AgentRegistry::discover(&[]);
        let index = registry.render_index().unwrap();
        assert!(index.contains("**explorer**"));
        assert!(index.contains("**reviewer**"));
        assert!(index.contains("**fixer**"));
    }
}
