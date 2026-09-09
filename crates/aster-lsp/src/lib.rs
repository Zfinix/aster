//! A minimal LSP client: spawns a language server over stdio and answers
//! diagnostics, references, and definition queries. Positions are 0-based
//! LSP positions (line, UTF-16 character); callers must convert.

mod rpc;
mod servers;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

pub use servers::{ServerKind, installed, supported};

const INDEX_WAIT: Duration = Duration::from_secs(60);
const DIAGNOSTICS_WAIT: Duration = Duration::from_secs(15);
const DIAGNOSTICS_SETTLE: Duration = Duration::from_millis(400);
const REQUEST_WAIT: Duration = Duration::from_secs(10);
const MAX_DIAGNOSTICS: usize = 200;
const MAX_LOCATIONS: usize = 100;

pub struct Client {
    transport: rpc::Transport,
    kind: ServerKind,
    root: PathBuf,
    open: HashMap<PathBuf, i64>,
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
        let transport = rpc::Transport::spawn(child, root)?;
        transport.request("initialize", initialize_params(root))?;
        transport.notify("initialized", json!({}))?;
        Ok(Self {
            transport,
            kind,
            root: root.to_path_buf(),
            open: HashMap::new(),
        })
    }

    /// Whether the server is still running, so a failed query is worth
    /// retrying on this client instead of restarting from a cold index.
    pub fn is_alive(&self) -> bool {
        !self.transport.is_closed()
    }

    /// The server's diagnostics for `path`, as published for its current
    /// contents.
    pub fn diagnostics(&mut self, path: &Path) -> Result<Vec<String>> {
        match self.diagnostics_within(path, INDEX_WAIT, DIAGNOSTICS_WAIT)? {
            Some(lines) => Ok(lines),
            None => Ok(vec![format!(
                "{} has not finished analyzing {} yet; ask again in a moment",
                self.kind.binary(),
                path.display()
            )]),
        }
    }

    /// The same query under a caller's own budget. `None` means the server
    /// published nothing for the current contents in time, which a caller
    /// that cannot wait should treat as "no answer", never as "no problems".
    pub fn diagnostics_within(
        &mut self,
        path: &Path,
        index_wait: Duration,
        diagnostics_wait: Duration,
    ) -> Result<Option<Vec<String>>> {
        self.transport.wait_idle(index_wait);
        let mark = self.transport.mark();
        self.sync(path)?;
        let key = canonical(path);
        let Some(payload) =
            self.transport
                .wait_diagnostics(&key, mark, diagnostics_wait, DIAGNOSTICS_SETTLE)
        else {
            if self.transport.is_closed() {
                bail!(
                    "{} exited while checking {}",
                    self.kind.binary(),
                    path.display()
                );
            }
            return Ok(None);
        };
        let empty = Vec::new();
        let mut items: Vec<&Value> = payload["diagnostics"]
            .as_array()
            .unwrap_or(&empty)
            .iter()
            .collect();
        items.sort_by_key(|d| {
            (
                d["range"]["start"]["line"].as_u64().unwrap_or(0),
                d["range"]["start"]["character"].as_u64().unwrap_or(0),
            )
        });
        let mut out = Vec::new();
        for d in items {
            if out.len() == MAX_DIAGNOSTICS {
                out.push(format!(
                    "(more diagnostics exist; showing the first {MAX_DIAGNOSTICS})"
                ));
                break;
            }
            out.push(format_diagnostic(d));
        }
        Ok(Some(out))
    }

    /// Where the symbol at the 0-based position is referenced.
    pub fn references(&mut self, path: &Path, line: u32, character: u32) -> Result<Vec<String>> {
        self.locations("textDocument/references", path, line, character, false)
    }

    /// Where the symbol at the 0-based position is defined.
    pub fn definitions(&mut self, path: &Path, line: u32, character: u32) -> Result<Vec<String>> {
        self.locations("textDocument/definition", path, line, character, true)
    }

    /// Brings the server's copy of `path` up to date with what is on disk:
    /// an open for the first sight of a file, a versioned change after that,
    /// then a save so check-on-save servers rerun their build.
    fn sync(&mut self, path: &Path) -> Result<()> {
        let text = std::fs::read_to_string(path).context("reading file for the language server")?;
        let uri = path_to_uri(path);
        let key = canonical(path);
        match self.open.get_mut(&key) {
            Some(version) => {
                *version += 1;
                self.transport.notify(
                    "textDocument/didChange",
                    json!({
                        "textDocument": { "uri": uri, "version": *version },
                        "contentChanges": [{ "text": text }],
                    }),
                )?;
            }
            None => {
                self.open.insert(key, 1);
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
            }
        }
        self.transport.notify(
            "textDocument/didSave",
            json!({ "textDocument": { "uri": uri }, "text": text }),
        )
    }

    fn locations(
        &mut self,
        method: &str,
        path: &Path,
        line: u32,
        character: u32,
        include_declaration: bool,
    ) -> Result<Vec<String>> {
        self.sync(path)?;
        self.transport.wait_idle(INDEX_WAIT);
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
            None if result.is_object() => vec![result],
            None => vec![],
        };
        let mut out = Vec::new();
        for loc in &items {
            if out.len() == MAX_LOCATIONS {
                out.push(format!(
                    "(more locations exist; showing the first {MAX_LOCATIONS})"
                ));
                break;
            }
            if let Some(line) = format_location(loc, &self.root) {
                out.push(line);
            }
        }
        Ok(out)
    }
}

fn initialize_params(root: &Path) -> Value {
    let uri = path_to_uri(root);
    json!({
        "processId": std::process::id(),
        "rootUri": uri,
        "workspaceFolders": [{
            "uri": uri,
            "name": root.file_name().unwrap_or_default().to_string_lossy(),
        }],
        "capabilities": {
            "workspace": {
                "workspaceFolders": true,
                "configuration": true,
                "didChangeWatchedFiles": { "dynamicRegistration": true },
            },
            "textDocument": {
                "synchronization": {
                    "dynamicRegistration": false,
                    "didSave": true,
                    "willSave": false,
                },
                "publishDiagnostics": {
                    "relatedInformation": true,
                    "versionSupport": true,
                    "codeDescriptionSupport": true,
                },
                "definition": { "linkSupport": false },
                "references": { "dynamicRegistration": false },
            },
            "window": { "workDoneProgress": true },
        },
        // rust-analyzer only sends its readiness notification when asked,
        // and it is the signal that indexing and the first check are done.
        "experimental": { "serverStatusNotification": true },
    })
}

fn format_diagnostic(d: &Value) -> String {
    let line = d["range"]["start"]["line"].as_u64().unwrap_or(0) + 1;
    let column = d["range"]["start"]["character"].as_u64().unwrap_or(0) + 1;
    let severity = match d["severity"].as_u64() {
        Some(1) => "error",
        Some(2) => "warning",
        Some(4) => "hint",
        _ => "info",
    };
    let code = match (d["source"].as_str(), code_as_str(&d["code"])) {
        (Some(source), Some(code)) => format!(" ({source} {code})"),
        (Some(source), None) => format!(" ({source})"),
        (None, Some(code)) => format!(" ({code})"),
        (None, None) => String::new(),
    };
    let message = d["message"].as_str().unwrap_or("").trim();
    format!("{severity} {line}:{column}: {message}{code}")
}

fn code_as_str(code: &Value) -> Option<String> {
    match code {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// A `file:line:column` line for a Location, or a LocationLink from a server
/// that sends them anyway.
fn format_location(loc: &Value, root: &Path) -> Option<String> {
    let (uri, range) = match loc.get("targetUri") {
        Some(uri) => (uri, &loc["targetSelectionRange"]),
        None => (loc.get("uri")?, &loc["range"]),
    };
    let path = uri_to_path(uri)?;
    let shown = path.strip_prefix(root).unwrap_or(&path);
    Some(format!(
        "{}:{}:{}",
        shown.display(),
        range["start"]["line"].as_u64().unwrap_or(0) + 1,
        range["start"]["character"].as_u64().unwrap_or(0) + 1
    ))
}

fn canonical(path: &Path) -> PathBuf {
    let absolute = absolute_path(path);
    std::fs::canonicalize(&absolute).unwrap_or(absolute)
}

pub(crate) fn path_to_uri(path: &Path) -> String {
    let mut uri = String::from("file://");
    for byte in canonical(path).to_string_lossy().bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                uri.push(byte as char)
            }
            _ => uri.push_str(&format!("%{byte:02X}")),
        }
    }
    uri
}

fn absolute_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(path)
    }
}

/// The canonical path a `file://` uri names, so paths that differ only in
/// escaping or symlinks still match what we asked about.
pub(crate) fn uri_to_path(uri: &Value) -> Option<PathBuf> {
    let rest = uri.as_str()?.strip_prefix("file://")?;
    let bytes = rest.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok()?;
                match u8::from_str_radix(hex, 16) {
                    Ok(byte) => {
                        decoded.push(byte);
                        i += 3;
                    }
                    Err(_) => {
                        decoded.push(bytes[i]);
                        i += 1;
                    }
                }
            }
            byte => {
                decoded.push(byte);
                i += 1;
            }
        }
    }
    let path = PathBuf::from(String::from_utf8(decoded).ok()?);
    Some(canonical(&path))
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
