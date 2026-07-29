#![forbid(unsafe_code)]
//! Agent definitions and discovery.

mod def;
mod registry;

pub use def::{AGENT_FILE, AgentDef, AgentSource, DEFAULT_TOOLS};
pub use registry::AgentRegistry;
