use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

use anyhow::{Context, Result, bail};

use aster_lsp::{Client, ServerKind, supported};

/// Which navigation query an lsp_locations call runs.
#[derive(Clone, Copy)]
pub enum Query {
    References,
    Definitions,
}

/// How long an edit is willing to wait for the server. An edit happens many
/// times a turn, so this is a glance, not the full check the tool waits for.
const AFTER_EDIT_WAIT: Duration = Duration::from_secs(3);
const AFTER_EDIT_MAX: usize = 10;

type ServerKey = (ServerKind, PathBuf);

static CLIENTS: LazyLock<Mutex<HashMap<ServerKey, Client>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static WARMING: LazyLock<Mutex<HashSet<ServerKey>>> = LazyLock::new(|| Mutex::new(HashSet::new()));

fn with_client<T>(
    root: &Path,
    kind: ServerKind,
    query: impl FnOnce(&mut Client) -> Result<T>,
) -> Result<T> {
    let mut clients = CLIENTS.lock().unwrap();
    let key = (kind, root.to_path_buf());
    let client = match clients.get_mut(&key) {
        Some(c) => c,
        None => {
            let c = Client::start(kind, root)?;
            clients.entry(key.clone()).or_insert(c)
        }
    };
    // A slow or unhappy query is not a reason to throw away a warm server:
    // restarting one means indexing the project again.
    let result = query(&mut *client);
    let alive = client.is_alive();
    if result.is_err() && !alive {
        clients.remove(&key);
    }
    result
}

pub fn diagnostics(root: &Path, path: &str) -> Result<String> {
    let file = resolve(root, path)?;
    let Some(kind) = server_for(&file) else {
        return Ok(String::new());
    };
    let lines = with_client(root, kind, |c| c.diagnostics(&file))?;
    Ok(if lines.is_empty() {
        "no diagnostics".to_string()
    } else {
        lines.join("\n")
    })
}

pub fn locations(
    root: &Path,
    path: &str,
    line: u32,
    character: u32,
    query: Query,
) -> Result<String> {
    let file = resolve(root, path)?;
    let Some(kind) = server_for(&file) else {
        return Ok(String::new());
    };
    let hits = with_client(root, kind, |c| match query {
        Query::References => c.references(&file, line, character),
        Query::Definitions => c.definitions(&file, line, character),
    })?;
    Ok(if hits.is_empty() {
        "no locations".to_string()
    } else {
        hits.join("\n")
    })
}

/// Dispatch helper for the lsp_references/lsp_definitions tools: pulls path,
/// line, and character out of the tool arguments.
pub fn nav_from_args(root: &Path, args: &serde_json::Value, query: Query) -> Result<String> {
    let path = args["path"].as_str().context("needs a `path`")?;
    let line = args["line"].as_u64().context("needs a `line`")? as u32;
    let character = args["character"].as_u64().context("needs a `character`")? as u32;
    locations(root, path, line, character, query)
}

fn resolve(root: &Path, path: &str) -> Result<PathBuf> {
    // `Path::join` replaces the base on an absolute argument, so confine the
    // canonicalized result to the repo before handing the server anything.
    let file = root.join(path);
    if !file.is_file() {
        bail!("{path} is not a file");
    }
    let file = file.canonicalize().unwrap_or(file);
    let root = root.canonicalize().unwrap_or_default();
    if !file.starts_with(&root) {
        bail!("{path} is outside the repository");
    }
    Ok(file)
}

/// None when no server applies: the caller stays quiet instead of
/// surfacing an unavailable-server message in the UI.
fn server_for(file: &Path) -> Option<ServerKind> {
    let kind = supported(file)?;
    aster_lsp::installed(kind).then_some(kind)
}

/// The problems a just-applied edit left in the file, for appending to the
/// edit's own result. Answers only from an already-running server: the first
/// edit starts one in the background instead, so no edit ever pays for a
/// cold index.
pub fn after_edit(root: &Path, path: &str) -> Option<String> {
    let file = resolve(root, path).ok()?;
    let kind = supported(&file).filter(|kind| aster_lsp::installed(*kind))?;
    let key = (kind, root.to_path_buf());
    let mut clients = CLIENTS.lock().unwrap();
    let Some(client) = clients.get_mut(&key) else {
        drop(clients);
        warm_up(key);
        return None;
    };
    let lines = client
        .diagnostics_within(&file, Duration::ZERO, AFTER_EDIT_WAIT)
        .ok()??;
    let problems: Vec<&String> = lines
        .iter()
        .filter(|line| line.starts_with("error ") || line.starts_with("warning "))
        .collect();
    if problems.is_empty() {
        return None;
    }
    // Say the edit landed first: a tool result that opens with errors reads
    // as a failed edit, and the model retries an edit that already applied.
    let mut out = format!(
        "The edit was applied. {} then reported {} in {path}:",
        kind.binary(),
        count(problems.len())
    );
    for line in problems.iter().take(AFTER_EDIT_MAX) {
        out.push_str(&format!("\n{line}"));
    }
    if problems.len() > AFTER_EDIT_MAX {
        out.push_str(&format!(
            "\n(showing the first {AFTER_EDIT_MAX}; run lsp_diagnostics for the rest)"
        ));
    }
    Some(out)
}

fn count(problems: usize) -> String {
    match problems {
        1 => "1 problem".to_string(),
        n => format!("{n} problems"),
    }
}

/// Starts a server off the edit's path so later edits in the same session
/// have one to ask.
fn warm_up(key: ServerKey) {
    if !WARMING.lock().unwrap().insert(key.clone()) {
        return;
    }
    std::thread::spawn(move || {
        if let Ok(client) = Client::start(key.0, &key.1) {
            CLIENTS.lock().unwrap().entry(key.clone()).or_insert(client);
        }
        WARMING.lock().unwrap().remove(&key);
    });
}

#[cfg(test)]
#[path = "tests/lsp_tools_test.rs"]
mod tests;
