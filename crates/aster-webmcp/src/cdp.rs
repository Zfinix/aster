//! Just enough of the Chrome DevTools Protocol to reach a page: find a tab
//! over HTTP, then speak JSON-RPC on its WebSocket. Connecting to a page
//! target's own debugger URL scopes every command to that tab, so no
//! `Target.attachToTarget` session plumbing is needed.

use std::time::Duration;

use anyhow::{Context, Result, bail};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::net::TcpStream;
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Target {
    #[serde(rename = "type")]
    kind: String,
    url: String,
    web_socket_debugger_url: String,
}

/// One WebSocket conversation with a single tab.
pub struct Tab {
    ws: WebSocketStream<MaybeTlsStream<TcpStream>>,
    next_id: i64,
    /// What the tab is showing, kept for error messages.
    url: String,
}

impl Tab {
    /// Attach to the first real page tab the browser at `cdp_url` reports.
    /// `about:blank` tabs are skipped: an agent wants the page the user is
    /// looking at, not a fresh new-tab page.
    pub async fn connect(cdp_url: &str, budget: Duration) -> Result<Self> {
        let base = cdp_url.trim_end_matches('/');
        let targets: Vec<Target> = timeout(budget, async {
            reqwest::Client::new()
                .get(format!("{base}/json/list"))
                .send()
                .await?
                .json()
                .await
        })
        .await
        .with_context(|| format!("no browser answered at {base}"))??;
        let page = targets
            .iter()
            .find(|t| t.kind == "page" && t.url != "about:blank")
            .or_else(|| targets.iter().find(|t| t.kind == "page"))
            .with_context(|| format!("the browser at {base} has no open tab to attach to"))?;
        let (ws, _) = timeout(budget, connect_async(&page.web_socket_debugger_url))
            .await
            .with_context(|| format!("the tab at {} did not accept a connection", page.url))??;
        Ok(Self {
            ws,
            next_id: 1,
            url: page.url.clone(),
        })
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    /// One command round trip. Events and replies to other ids arrive
    /// interleaved on the socket and are skipped, not mistaken for the reply.
    pub async fn command(
        &mut self,
        method: &str,
        params: Value,
        budget: Duration,
    ) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        let body = json!({ "id": id, "method": method, "params": params });
        self.ws
            .send(Message::Text(body.to_string().into()))
            .await
            .with_context(|| format!("the tab at {} dropped the connection", self.url))?;
        let reply = timeout(budget, async {
            loop {
                let Some(message) = self.ws.next().await else {
                    bail!("the tab at {} closed the connection", self.url);
                };
                let Message::Text(text) = message
                    .with_context(|| format!("reading from the tab at {} failed", self.url))?
                else {
                    continue;
                };
                let Ok(parsed) = serde_json::from_str::<Value>(&text) else {
                    continue;
                };
                if parsed.get("id").and_then(Value::as_i64) == Some(id) {
                    break Ok(parsed);
                }
            }
        })
        .await
        .with_context(|| format!("the tab at {} timed out on {method}", self.url))??;
        if let Some(error) = reply.get("error") {
            let message = error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("unknown error");
            bail!(
                "{method} was rejected by the tab at {}: {message}",
                self.url
            );
        }
        Ok(reply.get("result").cloned().unwrap_or(Value::Null))
    }
}
