//! What the bot assumes, checked against this machine. An import that does not
//! say which assumptions fail here has not finished.

use std::collections::BTreeMap;
use std::path::Path;

use super::ir::{BotIr, Trigger};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Configured here already: an MCP server, or a binary on PATH.
    Available,
    /// Real and reachable, but it has to be connected or authenticated first.
    NeedsSetup,
    /// Nothing here provides it. Event triggers and desktop-only work land here.
    Unsupported,
}

impl Status {
    pub fn label(self) -> &'static str {
        match self {
            Status::Available => "available",
            Status::NeedsSetup => "needs setup",
            Status::Unsupported => "unsupported",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Checked {
    pub id: String,
    pub status: Status,
    pub detail: String,
}

/// Classify every requirement, then every routine Aster cannot schedule.
/// Runs against the machine it is called on, never a static table.
pub fn check(ir: &BotIr, servers: &BTreeMap<String, crate::mcp::ServerConfig>) -> Vec<Checked> {
    let mut out: Vec<Checked> = ir
        .requirements
        .iter()
        .map(|req| {
            let id = req.id.trim().to_lowercase();
            match matching_server(&id, servers) {
                Some(name) => Checked {
                    id: req.id.clone(),
                    status: Status::Available,
                    detail: format!("MCP server {name}"),
                },
                None if on_path(&id) => Checked {
                    id: req.id.clone(),
                    status: Status::Available,
                    detail: format!("{id} on PATH"),
                },
                None => Checked {
                    id: req.id.clone(),
                    status: Status::NeedsSetup,
                    detail: "no MCP server or command provides it: `aster mcp add`".to_string(),
                },
            }
        })
        .collect();

    for routine in ir.non_cron_routines() {
        let detail = match &routine.trigger {
            Trigger::Event { name } => {
                format!("{name} event listener, which Aster has no substrate for")
            }
            _ => "no schedule could be recovered from the routine text".to_string(),
        };
        out.push(Checked {
            id: format!("routine {}", routine.name),
            status: Status::Unsupported,
            detail,
        });
    }
    out
}

pub fn count(checked: &[Checked], status: Status) -> usize {
    checked.iter().filter(|c| c.status == status).count()
}

/// A plugin id matches a server when the names line up either way round, which
/// covers `github` against both `github` and `github-mcp`.
fn matching_server(
    id: &str,
    servers: &BTreeMap<String, crate::mcp::ServerConfig>,
) -> Option<String> {
    if id.is_empty() {
        return None;
    }
    servers
        .keys()
        .find(|name| {
            let name = name.to_lowercase();
            name == id || name.contains(id) || id.contains(&name)
        })
        .cloned()
}

fn on_path(name: &str) -> bool {
    if name.is_empty() || name.contains(['/', '\\']) {
        return false;
    }
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| is_executable(&dir.join(name)))
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}
