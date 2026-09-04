//! Minimal JSON-RPC framing over a language server's stdio: Content-Length
//! headers, a reader thread, and a shared inbox for responses and
//! publishDiagnostics notifications.

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, ChildStdout};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

const POLL: Duration = Duration::from_millis(100);

#[derive(Default)]
pub struct Inbox {
    pub responses: Vec<(u64, Value)>,
    pub diagnostics: Vec<(String, Value)>,
}

pub struct Transport {
    child: Child,
    stdin: ChildStdin,
    inbox: Arc<Mutex<Inbox>>,
    next_id: AtomicU64,
}

impl Transport {
    pub fn spawn(mut child: Child) -> Result<Self> {
        let stdin = child.stdin.take().context("server stdin")?;
        let stdout = child.stdout.take().context("server stdout")?;
        let inbox = Arc::new(Mutex::new(Inbox::default()));
        let reader_inbox = Arc::clone(&inbox);
        std::thread::spawn(move || reader(stdout, reader_inbox));
        Ok(Self {
            child,
            stdin,
            inbox,
            next_id: AtomicU64::new(1),
        })
    }

    pub fn notify(&mut self, method: &str, params: Value) -> Result<()> {
        let body = json!({ "jsonrpc": "2.0", "method": method, "params": params });
        self.send(&body)
    }

    pub fn request(&mut self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let body = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        self.send(&body)?;
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let found = {
                let mut inbox = self.inbox.lock().expect("inbox");
                if let Some(pos) = inbox.responses.iter().position(|(rid, _)| *rid == id) {
                    let (_, value) = inbox.responses.remove(pos);
                    return match value.get("error") {
                        Some(err) => bail!("server error: {err}"),
                        None => Ok(value["result"].clone()),
                    };
                }
                false
            };
            if found || Instant::now() > deadline {
                bail!("timed out waiting for {method} response");
            }
            std::thread::sleep(POLL);
        }
    }

    /// The latest publishDiagnostics payload for `uri`, preferring a non-empty
    /// batch: servers publish an empty one before analysis finishes. Falls
    /// back to the latest batch at `wait` expiry, or None if nothing arrived.
    pub fn wait_diagnostics(&self, uri: &str, wait: Duration) -> Option<Value> {
        let deadline = Instant::now() + wait;
        loop {
            let inbox = self.inbox.lock().expect("inbox");
            if let Some((_, payload)) = inbox.diagnostics.iter().rev().find(|(u, _)| u == uri) {
                let empty = payload["diagnostics"]
                    .as_array()
                    .is_none_or(|a| a.is_empty());
                if !empty || Instant::now() > deadline {
                    return Some(payload.clone());
                }
            } else if Instant::now() > deadline {
                return None;
            }
            drop(inbox);
            std::thread::sleep(POLL);
        }
    }

    fn send(&mut self, body: &Value) -> Result<()> {
        let text = serde_json::to_string(body).context("encoding message")?;
        write!(self.stdin, "Content-Length: {}\r\n\r\n{}", text.len(), text)
            .and_then(|_| self.stdin.flush())
            .context("writing to server stdin")
    }
}

impl Drop for Transport {
    fn drop(&mut self) {
        let _ = self.notify("shutdown", json!(null));
        let _ = self.notify("exit", json!(null));
        let _ = self.stdin.flush();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn reader(stdout: ChildStdout, inbox: Arc<Mutex<Inbox>>) {
    let mut reader = BufReader::new(stdout);
    loop {
        let body = match read_message(&mut reader) {
            Ok(Some(body)) => body,
            _ => return,
        };
        let Ok(msg) = serde_json::from_str::<Value>(&body) else {
            continue;
        };
        let mut inbox = inbox.lock().expect("inbox");
        if let Some(id) = msg.get("id").and_then(Value::as_u64) {
            inbox.responses.push((id, msg));
        } else if msg["method"] == "textDocument/publishDiagnostics" {
            let uri = msg["params"]["uri"]
                .as_str()
                .unwrap_or_default()
                .to_string();
            inbox.diagnostics.push((uri, msg["params"].clone()));
        }
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
