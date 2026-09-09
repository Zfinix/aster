//! Minimal JSON-RPC framing over a language server's stdio: Content-Length
//! headers, a reader thread that answers the server's own requests, and a
//! shared inbox for responses, diagnostics, and progress.

use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

const POLL: Duration = Duration::from_millis(20);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

struct Batch {
    seq: u64,
    at: Instant,
    payload: Value,
}

#[derive(Default)]
pub struct Inbox {
    responses: Vec<(u64, Value)>,
    diagnostics: HashMap<PathBuf, Batch>,
    progress: HashSet<String>,
    quiescent: Option<bool>,
    seq: u64,
    closed: bool,
}

impl Inbox {
    /// Whether the server is still indexing or running a check, so its
    /// current answers may be incomplete.
    fn busy(&self) -> bool {
        !self.progress.is_empty() || self.quiescent == Some(false)
    }
}

pub struct Transport {
    child: Child,
    stdin: Arc<Mutex<ChildStdin>>,
    inbox: Arc<Mutex<Inbox>>,
    next_id: AtomicU64,
}

impl Transport {
    pub fn spawn(mut child: Child, root: &Path) -> Result<Self> {
        let stdin = Arc::new(Mutex::new(child.stdin.take().context("server stdin")?));
        let stdout = child.stdout.take().context("server stdout")?;
        let inbox = Arc::new(Mutex::new(Inbox::default()));
        let reader_inbox = Arc::clone(&inbox);
        let reader_stdin = Arc::clone(&stdin);
        let root = root.to_path_buf();
        std::thread::spawn(move || reader(stdout, reader_inbox, reader_stdin, root));
        Ok(Self {
            child,
            stdin,
            inbox,
            next_id: AtomicU64::new(1),
        })
    }

    pub fn notify(&self, method: &str, params: Value) -> Result<()> {
        write_message(
            &self.stdin,
            &json!({ "jsonrpc": "2.0", "method": method, "params": params }),
        )
    }

    pub fn request(&self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        write_message(
            &self.stdin,
            &json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": method,
                "params": params,
            }),
        )?;
        let deadline = Instant::now() + REQUEST_TIMEOUT;
        loop {
            {
                let mut inbox = self.inbox.lock().expect("inbox");
                if let Some(pos) = inbox.responses.iter().position(|(rid, _)| *rid == id) {
                    let (_, value) = inbox.responses.remove(pos);
                    return match value.get("error") {
                        Some(err) => bail!("server error: {err}"),
                        None => Ok(value["result"].clone()),
                    };
                }
                if inbox.closed {
                    bail!("the language server exited before answering {method}");
                }
            }
            if Instant::now() > deadline {
                bail!("timed out waiting for {method} response");
            }
            std::thread::sleep(POLL);
        }
    }

    pub fn is_closed(&self) -> bool {
        self.inbox.lock().expect("inbox").closed
    }

    /// A mark for the diagnostics published so far. Batches published after
    /// it are the ones that saw the edit we are about to send.
    pub fn mark(&self) -> u64 {
        self.inbox.lock().expect("inbox").seq
    }

    /// Blocks until the server reports it is idle, or `wait` expires.
    /// Returns whether it went idle.
    pub fn wait_idle(&self, wait: Duration) -> bool {
        let deadline = Instant::now() + wait;
        loop {
            {
                let inbox = self.inbox.lock().expect("inbox");
                if inbox.closed || !inbox.busy() {
                    return !inbox.closed;
                }
            }
            if Instant::now() > deadline {
                return false;
            }
            std::thread::sleep(POLL);
        }
    }

    /// The diagnostics for `path` published after `mark`, so the answer
    /// always describes the current contents rather than an earlier read.
    /// Servers publish in several rounds (syntax first, then the type check),
    /// so this keeps the newest batch until they stop arriving.
    pub fn wait_diagnostics(
        &self,
        path: &Path,
        mark: u64,
        wait: Duration,
        settle: Duration,
    ) -> Option<Value> {
        let deadline = Instant::now() + wait;
        loop {
            {
                let inbox = self.inbox.lock().expect("inbox");
                let fresh = inbox.diagnostics.get(path).filter(|batch| batch.seq > mark);
                if let Some(batch) = fresh
                    && (!inbox.busy() && batch.at.elapsed() >= settle || Instant::now() > deadline)
                {
                    return Some(batch.payload.clone());
                }
                if inbox.closed || (fresh.is_none() && Instant::now() > deadline) {
                    return None;
                }
            }
            std::thread::sleep(POLL);
        }
    }
}

impl Drop for Transport {
    fn drop(&mut self) {
        let _ = self.notify("shutdown", json!(null));
        let _ = self.notify("exit", json!(null));
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn write_message(stdin: &Mutex<ChildStdin>, body: &Value) -> Result<()> {
    let text = serde_json::to_string(body).context("encoding message")?;
    let mut stdin = stdin.lock().expect("server stdin");
    write!(stdin, "Content-Length: {}\r\n\r\n{}", text.len(), text)
        .and_then(|_| stdin.flush())
        .context("writing to server stdin")
}

fn reader(
    stdout: ChildStdout,
    inbox: Arc<Mutex<Inbox>>,
    stdin: Arc<Mutex<ChildStdin>>,
    root: PathBuf,
) {
    let mut reader = BufReader::new(stdout);
    loop {
        let body = match read_message(&mut reader) {
            Ok(Some(body)) => body,
            _ => {
                inbox.lock().expect("inbox").closed = true;
                return;
            }
        };
        let Ok(msg) = serde_json::from_str::<Value>(&body) else {
            continue;
        };
        // A message carrying both an id and a method is the server asking us
        // something; only the id-without-method case answers our own request.
        match (msg.get("id"), msg["method"].as_str()) {
            (Some(id), Some(method)) => {
                let _ = write_message(&stdin, &answer(method, id.clone(), &msg["params"], &root));
            }
            (Some(id), None) => {
                if let Some(id) = id.as_u64() {
                    inbox.lock().expect("inbox").responses.push((id, msg));
                }
            }
            (None, Some(method)) => note(method, &msg["params"], &inbox),
            (None, None) => {}
        }
    }
}

fn answer(method: &str, id: Value, params: &Value, root: &Path) -> Value {
    let result = match method {
        // No stored settings: an empty object leaves every server default in
        // place, which is what an unattended client wants.
        "workspace/configuration" => {
            let items = params["items"].as_array().map_or(1, Vec::len);
            Some(Value::Array(vec![json!({}); items]))
        }
        "workspace/workspaceFolders" => Some(json!([{
            "uri": crate::path_to_uri(root),
            "name": root.file_name().unwrap_or_default().to_string_lossy(),
        }])),
        "client/registerCapability"
        | "client/unregisterCapability"
        | "window/workDoneProgress/create"
        | "window/showMessageRequest"
        | "workspace/applyEdit"
        | "workspace/semanticTokens/refresh"
        | "workspace/codeLens/refresh"
        | "workspace/diagnostic/refresh"
        | "workspace/inlayHint/refresh" => Some(Value::Null),
        _ => None,
    };
    match result {
        Some(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
        None => json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": -32601, "message": format!("{method} is not supported") },
        }),
    }
}

fn note(method: &str, params: &Value, inbox: &Mutex<Inbox>) {
    let mut inbox = inbox.lock().expect("inbox");
    match method {
        "textDocument/publishDiagnostics" => {
            let Some(path) = crate::uri_to_path(&params["uri"]) else {
                return;
            };
            inbox.seq += 1;
            let batch = Batch {
                seq: inbox.seq,
                at: Instant::now(),
                payload: params.clone(),
            };
            inbox.diagnostics.insert(path, batch);
        }
        "$/progress" => {
            let token = params["token"].to_string();
            match params["value"]["kind"].as_str() {
                Some("begin") => {
                    inbox.progress.insert(token);
                }
                Some("end") => {
                    inbox.progress.remove(&token);
                }
                _ => {}
            }
        }
        // rust-analyzer's own readiness signal, which covers the gap between
        // the end of indexing and the first check run.
        "experimental/serverStatus" => {
            inbox.quiescent = params["quiescent"].as_bool();
        }
        _ => {}
    }
}

fn read_message(reader: &mut BufReader<ChildStdout>) -> Result<Option<String>> {
    let mut length = 0usize;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some(value) = line.strip_prefix("Content-Length: ") {
            length = value.trim().parse().context("content length")?;
        }
    }
    let mut buf = vec![0u8; length];
    reader.read_exact(&mut buf)?;
    Ok(Some(String::from_utf8_lossy(&buf).into_owned()))
}
