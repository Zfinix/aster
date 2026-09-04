use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

use anyhow::{Context, Result, bail};

use aster_lsp::{Client, ServerKind, supported};

/// Which navigation query an lsp_locations call runs.
#[derive(Clone, Copy)]
pub enum Query {
    References,
    Definitions,
}

static CLIENTS: LazyLock<Mutex<HashMap<(ServerKind, PathBuf), Client>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

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
    match query(client) {
        Ok(v) => Ok(v),
        Err(e) => {
            clients.remove(&key);
            Err(e)
        }
    }
}

pub fn diagnostics(root: &Path, path: &str) -> Result<String> {
    let file = resolve(root, path)?;
    let kind = match server_for(&file) {
        Ok(kind) => kind,
        Err(unavailable) => return Ok(unavailable),
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
    let kind = match server_for(&file) {
        Ok(kind) => kind,
        Err(unavailable) => return Ok(unavailable),
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

fn server_for(file: &Path) -> std::result::Result<ServerKind, String> {
    match supported(file) {
        Some(kind) if aster_lsp::installed(kind) => Ok(kind),
        Some(kind) => Err(format!(
            "{} is not installed, so there are no language server checks for this file",
            kind.binary()
        )),
        None => Err(format!("no language server for {}", file.display())),
    }
}
