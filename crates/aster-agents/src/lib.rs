#![forbid(unsafe_code)]
//! Sub-agent definitions and discovery. An agent is a directory holding an
//! `AGENT.md`: YAML frontmatter (`name`, `description`, and optional `model`,
//! `tools`, `max_rounds`, `verify`) followed by a markdown body that becomes the
//! agent's system prompt.
//!
//! Layout mirrors `aster-skills`: a project root (`.aster/agents`) overrides a
//! user-global root (`<config>/aster/agents`), and both override the compiled-in
//! built-ins (`explorer`, `reviewer`, `fixer`).

mod def;
mod registry;

pub use def::{AGENT_FILE, AgentDef, AgentSource, DEFAULT_TOOLS};
pub use registry::AgentRegistry;
