//! Where other coding agents keep their skills, so Aster can import from them.
//! Mirrors the agent table in `vercel-labs/skills`, which is the registry both
//! sides install against.

use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Root {
    None,
    Home(&'static str),
    Config(&'static str),
    Env {
        var: &'static str,
        fallback: &'static str,
        sub: &'static str,
    },
}

/// One coding agent and the two roots it reads skills from.
#[derive(Clone, Copy, Debug)]
pub struct Agent {
    pub key: &'static str,
    pub display_name: &'static str,
    pub project_dir: &'static str,
    global: Root,
}

impl Agent {
    /// This agent's user-global skills root, absent for project-only agents and
    /// when the home directory cannot be determined.
    pub fn global_dir(&self) -> Option<PathBuf> {
        match self.global {
            Root::None => None,
            Root::Home(rel) => dirs::home_dir().map(|h| h.join(rel)),
            Root::Config(rel) => config_home().map(|c| c.join(rel)),
            Root::Env { var, fallback, sub } => {
                let base = match std::env::var(var) {
                    Ok(v) if !v.trim().is_empty() => PathBuf::from(v.trim()),
                    _ => dirs::home_dir()?.join(fallback),
                };
                Some(base.join(sub))
            }
        }
    }

    pub fn project_dir_in(&self, repo_root: &Path) -> PathBuf {
        repo_root.join(self.project_dir)
    }

    /// The roots worth importing from: those that exist right now, global first.
    pub fn existing_roots(&self, repo_root: Option<&Path>) -> Vec<PathBuf> {
        let global = self.global_dir().filter(|d| d.is_dir());
        let project = repo_root
            .map(|r| self.project_dir_in(r))
            .filter(|d| d.is_dir());
        global.into_iter().chain(project).collect()
    }
}

fn config_home() -> Option<PathBuf> {
    match std::env::var("XDG_CONFIG_HOME") {
        Ok(v) if !v.trim().is_empty() => Some(PathBuf::from(v.trim())),
        _ => dirs::home_dir().map(|h| h.join(".config")),
    }
}

pub const AGENTS: &[Agent] = &[
    agent("adal", "AdaL", ".adal/skills", Root::Home(".adal/skills")),
    agent(
        "aider-desk",
        "AiderDesk",
        ".aider-desk/skills",
        Root::Home(".aider-desk/skills"),
    ),
    agent(
        "amp",
        "Amp",
        ".agents/skills",
        Root::Config("agents/skills"),
    ),
    agent(
        "antigravity",
        "Antigravity",
        ".agents/skills",
        Root::Home(".gemini/antigravity/skills"),
    ),
    agent(
        "antigravity-cli",
        "Antigravity CLI",
        ".agents/skills",
        Root::Home(".gemini/antigravity-cli/skills"),
    ),
    agent(
        "astrbot",
        "AstrBot",
        "data/skills",
        Root::Home(".astrbot/data/skills"),
    ),
    agent(
        "augment",
        "Augment",
        ".augment/skills",
        Root::Home(".augment/skills"),
    ),
    agent(
        "autohand-code",
        "Autohand Code CLI",
        ".autohand/skills",
        Root::Env {
            var: "AUTOHAND_HOME",
            fallback: ".autohand",
            sub: "skills",
        },
    ),
    agent("bob", "IBM Bob", ".bob/skills", Root::Home(".bob/skills")),
    agent(
        "claude-code",
        "Claude Code",
        ".claude/skills",
        Root::Env {
            var: "CLAUDE_CONFIG_DIR",
            fallback: ".claude",
            sub: "skills",
        },
    ),
    agent(
        "cline",
        "Cline",
        ".agents/skills",
        Root::Home(".agents/skills"),
    ),
    agent(
        "codearts-agent",
        "CodeArts Agent",
        ".codeartsdoer/skills",
        Root::Home(".codeartsdoer/skills"),
    ),
    agent(
        "codebuddy",
        "CodeBuddy",
        ".codebuddy/skills",
        Root::Home(".codebuddy/skills"),
    ),
    agent(
        "codemaker",
        "Codemaker",
        ".codemaker/skills",
        Root::Home(".codemaker/skills"),
    ),
    agent(
        "codestudio",
        "Code Studio",
        ".codestudio/skills",
        Root::Home(".codestudio/skills"),
    ),
    agent(
        "codex",
        "Codex",
        ".agents/skills",
        Root::Env {
            var: "CODEX_HOME",
            fallback: ".codex",
            sub: "skills",
        },
    ),
    agent(
        "command-code",
        "Command Code",
        ".commandcode/skills",
        Root::Home(".commandcode/skills"),
    ),
    agent(
        "continue",
        "Continue",
        ".continue/skills",
        Root::Home(".continue/skills"),
    ),
    agent(
        "cortex",
        "Cortex Code",
        ".cortex/skills",
        Root::Home(".snowflake/cortex/skills"),
    ),
    agent(
        "crush",
        "Crush",
        ".crush/skills",
        Root::Config("crush/skills"),
    ),
    agent(
        "cursor",
        "Cursor",
        ".agents/skills",
        Root::Home(".cursor/skills"),
    ),
    agent(
        "deepagents",
        "Deep Agents",
        ".agents/skills",
        Root::Home(".deepagents/agent/skills"),
    ),
    agent(
        "devin",
        "Devin for Terminal",
        ".devin/skills",
        Root::Config("devin/skills"),
    ),
    agent(
        "dexto",
        "Dexto",
        ".agents/skills",
        Root::Home(".agents/skills"),
    ),
    agent(
        "droid",
        "Droid",
        ".factory/skills",
        Root::Home(".factory/skills"),
    ),
    agent("eve", "Eve", "agent/skills", Root::None),
    agent(
        "firebender",
        "Firebender",
        ".agents/skills",
        Root::Home(".firebender/skills"),
    ),
    agent(
        "forgecode",
        "ForgeCode",
        ".forge/skills",
        Root::Home(".forge/skills"),
    ),
    agent(
        "gemini-cli",
        "Gemini CLI",
        ".agents/skills",
        Root::Home(".gemini/skills"),
    ),
    agent(
        "github-copilot",
        "GitHub Copilot",
        ".agents/skills",
        Root::Home(".copilot/skills"),
    ),
    agent(
        "goose",
        "Goose",
        ".goose/skills",
        Root::Config("goose/skills"),
    ),
    agent(
        "grok",
        "Grok Build",
        ".grok/skills",
        Root::Env {
            var: "GROK_HOME",
            fallback: ".grok",
            sub: "skills",
        },
    ),
    agent(
        "hermes-agent",
        "Hermes Agent",
        ".hermes/skills",
        Root::Env {
            var: "HERMES_HOME",
            fallback: ".hermes",
            sub: "skills",
        },
    ),
    agent(
        "iflow-cli",
        "iFlow CLI",
        ".iflow/skills",
        Root::Home(".iflow/skills"),
    ),
    agent(
        "inference-sh",
        "inference.sh",
        ".inferencesh/skills",
        Root::Home(".inferencesh/skills"),
    ),
    agent("jazz", "Jazz", ".jazz/skills", Root::Home(".jazz/skills")),
    agent(
        "junie",
        "Junie",
        ".junie/skills",
        Root::Home(".junie/skills"),
    ),
    agent(
        "kilo",
        "Kilo Code",
        ".kilocode/skills",
        Root::Home(".kilocode/skills"),
    ),
    agent(
        "kimchi",
        "Kimchi",
        ".kimchi/skills",
        Root::Config("kimchi/harness/skills"),
    ),
    agent(
        "kimi-code-cli",
        "Kimi Code CLI",
        ".agents/skills",
        Root::Home(".agents/skills"),
    ),
    agent(
        "kiro-cli",
        "Kiro CLI",
        ".kiro/skills",
        Root::Home(".kiro/skills"),
    ),
    agent("kode", "Kode", ".kode/skills", Root::Home(".kode/skills")),
    agent(
        "lingma",
        "Lingma",
        ".lingma/skills",
        Root::Home(".lingma/skills"),
    ),
    agent(
        "loaf",
        "Loaf",
        ".agents/skills",
        Root::Home(".agents/skills"),
    ),
    agent(
        "mcpjam",
        "MCPJam",
        ".mcpjam/skills",
        Root::Home(".mcpjam/skills"),
    ),
    agent(
        "minimax-code",
        "MiniMax Code",
        ".minimax/skills",
        Root::Home(".minimax/skills"),
    ),
    agent(
        "mistral-vibe",
        "Mistral Vibe",
        ".vibe/skills",
        Root::Env {
            var: "VIBE_HOME",
            fallback: ".vibe",
            sub: "skills",
        },
    ),
    agent(
        "moxby",
        "Moxby",
        ".moxby/skills",
        Root::Home(".moxby/skills"),
    ),
    agent("mux", "Mux", ".mux/skills", Root::Home(".mux/skills")),
    agent(
        "neovate",
        "Neovate",
        ".neovate/skills",
        Root::Home(".neovate/skills"),
    ),
    agent("ona", "Ona", ".ona/skills", Root::Home(".ona/skills")),
    agent(
        "openclaw",
        "OpenClaw",
        "skills",
        Root::Home(".openclaw/skills"),
    ),
    agent(
        "opencode",
        "OpenCode",
        ".agents/skills",
        Root::Config("opencode/skills"),
    ),
    agent(
        "openhands",
        "OpenHands",
        ".openhands/skills",
        Root::Home(".openhands/skills"),
    ),
    agent("pi", "Pi", ".pi/skills", Root::Home(".pi/agent/skills")),
    agent(
        "pochi",
        "Pochi",
        ".pochi/skills",
        Root::Home(".pochi/skills"),
    ),
    agent("promptscript", "PromptScript", ".agents/skills", Root::None),
    agent(
        "qoder",
        "Qoder",
        ".qoder/skills",
        Root::Home(".qoder/skills"),
    ),
    agent(
        "qoder-cn",
        "Qoder CN",
        ".qoder/skills",
        Root::Home(".qoder-cn/skills"),
    ),
    agent(
        "qwen-code",
        "Qwen Code",
        ".qwen/skills",
        Root::Home(".qwen/skills"),
    ),
    agent(
        "reasonix",
        "Reasonix",
        ".reasonix/skills",
        Root::Home(".reasonix/skills"),
    ),
    agent(
        "replit",
        "Replit",
        ".agents/skills",
        Root::Config("agents/skills"),
    ),
    agent("roo", "Roo Code", ".roo/skills", Root::Home(".roo/skills")),
    agent(
        "rovodev",
        "Rovo Dev",
        ".rovodev/skills",
        Root::Home(".rovodev/skills"),
    ),
    agent(
        "tabnine-cli",
        "Tabnine CLI",
        ".tabnine/agent/skills",
        Root::Home(".tabnine/agent/skills"),
    ),
    agent(
        "terramind",
        "Terramind",
        ".terramind/skills",
        Root::Home(".terramind/skills"),
    ),
    agent(
        "tinycloud",
        "Tinycloud",
        ".tinycloud/skills",
        Root::Home(".tinycloud/skills"),
    ),
    agent("trae", "Trae", ".trae/skills", Root::Home(".trae/skills")),
    agent(
        "trae-cn",
        "Trae CN",
        ".trae/skills",
        Root::Home(".trae-cn/skills"),
    ),
    agent(
        "universal",
        "Universal",
        ".agents/skills",
        Root::Config("agents/skills"),
    ),
    agent(
        "warp",
        "Warp",
        ".agents/skills",
        Root::Home(".agents/skills"),
    ),
    agent(
        "windsurf",
        "Windsurf",
        ".windsurf/skills",
        Root::Home(".codeium/windsurf/skills"),
    ),
    agent(
        "zcode",
        "ZCode",
        ".zcode/skills",
        Root::Home(".zcode/skills"),
    ),
    agent("zed", "Zed", ".agents/skills", Root::Home(".agents/skills")),
    agent(
        "zencoder",
        "Zencoder",
        ".zencoder/skills",
        Root::Home(".zencoder/skills"),
    ),
    agent(
        "zenflow",
        "Zenflow",
        ".zencoder/skills",
        Root::Home(".zencoder/skills"),
    ),
];

const fn agent(
    key: &'static str,
    display_name: &'static str,
    project_dir: &'static str,
    global: Root,
) -> Agent {
    Agent {
        key,
        display_name,
        project_dir,
        global,
    }
}

pub fn agent_by_key(key: &str) -> Option<&'static Agent> {
    let key = key.trim().to_ascii_lowercase();
    AGENTS.iter().find(|a| a.key == key)
}

/// Agents with at least one skills root on disk, in registry order.
pub fn installed_agents(repo_root: Option<&Path>) -> Vec<&'static Agent> {
    AGENTS
        .iter()
        .filter(|a| !a.existing_roots(repo_root).is_empty())
        .collect()
}

#[cfg(test)]
#[path = "tests/agents_test.rs"]
mod tests;
