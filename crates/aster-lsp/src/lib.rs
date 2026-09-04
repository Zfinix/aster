//! A minimal LSP client: spawns a language server over stdio and answers
//! diagnostics, references, and definition queries. Positions are 0-based
//! LSP positions (line, UTF-16 character); callers must convert.

mod rpc;
mod servers;

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

pub use servers::{ServerKind, installed, supported};

/// How long diagnostics() waits for the server's first publish.
const DIAGNOSTICS_WAIT: Duration = Duration::from_secs(10);
/// How long locations() retries while the project is still loading: the
/// server answers "file not found" or "content modified", or returns an
/// empty result before analysis finishes.
const REQUEST_WAIT: Duration = Duration::from_secs(10);
/// Diagnostics returned per file; past this the list is cut and counted.
const MAX_DIAGNOSTICS: usize = 200;
/// Locations returned per references/definitions query; past this the list is
/// cut and counted.
const MAX_LOCATIONS: usize = 100;

pub struct Client {
    transport: rpc::Transport,
    kind: ServerKind,
}

impl Client {
    pub fn start(kind: ServerKind, root: &Path) -> Result<Self> {
        if !installed(kind) {
            bail!("{} is not installed", kind.binary());
        }
        let child = Command::new(kind.binary())
            .args(kind.args())
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| format!("starting {}", kind.binary()))?;
        let mut transport = rpc::Transport::spawn(child)?;
        transport.request(
            "initialize",
            json!({
                "processId": std::process::id(),
                "rootUri": path_to_uri(root),
                "capabilities": {},
            }),
        )?;
        transport.notify("initialized", json!({}))?;
        Ok(Self { transport, kind })
    }

    /// The server's published diagnostics for `path` after opening it.
    pub fn diagnostics(&mut self, path: &Path) -> Result<Vec<String>> {
        let uri = path_to_uri(path);
        let text = std::fs::read_to_string(path).context("reading file for diagnostics")?;
        self.transport.notify(
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": self.kind.language_id(),
                    "version": 1,
                    "text": text,
                }
            }),
        )?;
        let Some(payload) = self.transport.wait_diagnostics(&uri, DIAGNOSTICS_WAIT) else {
            bail!("no diagnostics published for {}", path.display());
        };
        let mut out = Vec::new();
        for d in payload["diagnostics"].as_array().unwrap_or(&vec![]) {
            if out.len() == MAX_DIAGNOSTICS {
                out.push(format!(
                    "(more diagnostics exist; showing the first {MAX_DIAGNOSTICS})"
                ));
                break;
            }
            let line = d["range"]["start"]["line"].as_u64().unwrap_or(0) + 1;
            let severity = match d["severity"].as_u64() {
                Some(1) => "error",
                Some(2) => "warning",
                _ => "info",
            };
            out.push(format!(
                "{severity} {line}: {}",
                d["message"].as_str().unwrap_or("")
            ));
        }
        Ok(out)
    }

    /// Where the symbol at the 0-based position is referenced.
    pub fn references(&mut self, path: &Path, line: u32, character: u32) -> Result<Vec<String>> {
        self.locations("textDocument/references", path, line, character, false)
    }

    /// Where the symbol at the 0-based position is defined.
    pub fn definitions(&mut self, path: &Path, line: u32, character: u32) -> Result<Vec<String>> {
        self.locations("textDocument/definition", path, line, character, true)
    }

    fn locations(
        &mut self,
        method: &str,
        path: &Path,
        line: u32,
        character: u32,
        include_declaration: bool,
    ) -> Result<Vec<String>> {
        let params = json!({
            "textDocument": { "uri": path_to_uri(path) },
            "position": { "line": line, "character": character },
            "context": { "includeDeclaration": include_declaration },
        });
        let deadline = Instant::now() + REQUEST_WAIT;
        let result = loop {
            match self.transport.request(method, params.clone()) {
                Ok(result) => {
                    // An empty answer right after startup usually means the
                    // project is still loading, so give the server the window
                    // to reconsider before accepting it.
                    let empty = result.is_null() || result.as_array().is_some_and(|a| a.is_empty());
                    if !empty || Instant::now() >= deadline {
                        break result;
                    }
                }
                Err(e) => {
                    let transient = ["file not found", "content modified"]
                        .iter()
                        .any(|needle| e.to_string().contains(needle));
                    if !transient || Instant::now() >= deadline {
                        return Err(e);
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(300));
        };
        let items = match result.as_array() {
            Some(items) => items.clone(),
            None => match result.get("uri") {
                Some(_) => vec![result],
                None => vec![],
            },
        };
        let mut out = Vec::new();
        for loc in &items {
            if out.len() == MAX_LOCATIONS {
                out.push(format!(
                    "(more locations exist; showing the first {MAX_LOCATIONS})"
                ));
                break;
            }
            out.push(format!(
                "{}:{}",
                uri_to_path(&loc["uri"]).display(),
                loc["range"]["start"]["line"].as_u64().unwrap_or(0) + 1
            ));
        }
        Ok(out)
    }
}

/// rust-analyzer canonicalizes paths (/var/folders -> /private/var/folders on
/// macOS) when indexing, so every URI we send must be canonical or the server
/// reports "file not found".
fn path_to_uri(path: &Path) -> String {
    let absolute = absolute_path(path);
    let canonical = std::fs::canonicalize(&absolute).unwrap_or(absolute);
    format!("file://{}", canonical.display())
}

fn absolute_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(path)
    }
}
fn uri_to_path(uri: &Value) -> PathBuf {
    PathBuf::from(
        uri.as_str()
            .unwrap_or_default()
            .strip_prefix("file://")
            .unwrap_or_default(),
    )
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
