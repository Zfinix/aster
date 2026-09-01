//! Remote MCP transports: Streamable HTTP, and the HTTP+SSE binding it
//! replaced. Both carry the same JSON-RPC messages the stdio wire does, so only
//! the send and receive halves live here.

use std::collections::BTreeMap;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use futures_util::StreamExt;
use reqwest::header::{ACCEPT, CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue};
use reqwest::{Client, Response, StatusCode, Url, redirect};
use serde_json::Value;
use tokio::sync::{mpsc, oneshot};
use tokio::time::timeout;

const SESSION_HEADER: &str = "mcp-session-id";
const PROTOCOL_HEADER: &str = "mcp-protocol-version";
/// A remote round trip is not a local pipe, so the era probe waits longer than
/// the stdio one before deciding a server predates per-request metadata.
pub(super) const PROBE_TIMEOUT: Duration = Duration::from_secs(3);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// Redirects followed inside one origin. Crossing origins would leak the
/// configured headers to a host the user never named.
const MAX_REDIRECTS: usize = 5;

pub(super) enum Remote {
    Streamable(Streamable),
    Sse(Sse),
}

impl Remote {
    pub(super) async fn connect_streamable(
        url: &str,
        headers: &BTreeMap<String, String>,
    ) -> Result<Self> {
        Ok(Self::Streamable(Streamable {
            client: client()?,
            url: url.to_string(),
            headers: header_map(headers)?,
            session: None,
            protocol: None,
        }))
    }

    pub(super) async fn connect_sse(
        name: &str,
        url: &str,
        headers: &BTreeMap<String, String>,
        budget: Duration,
    ) -> Result<Self> {
        Ok(Self::Sse(
            Sse::open(name, url, &header_map(headers)?, budget).await?,
        ))
    }

    pub(super) async fn exchange(
        &mut self,
        name: &str,
        method: &str,
        body: &Value,
        id: i64,
        budget: Duration,
    ) -> Result<Value> {
        match self {
            Remote::Streamable(wire) => wire.exchange(name, method, body, id, budget).await,
            Remote::Sse(wire) => wire.exchange(name, method, body, id, budget).await,
        }
    }

    pub(super) async fn notify(&mut self, name: &str, body: &Value) -> Result<()> {
        match self {
            Remote::Streamable(wire) => wire.notify(name, body).await,
            Remote::Sse(wire) => wire.notify(name, body).await,
        }
    }

    /// Record the negotiated revision, which every later request must carry.
    pub(super) fn set_protocol(&mut self, version: &str) {
        if let Remote::Streamable(wire) = self {
            wire.protocol = HeaderValue::from_str(version).ok();
        }
    }

    pub(super) async fn shutdown(&mut self) {
        match self {
            Remote::Streamable(wire) => wire.end_session().await,
            Remote::Sse(wire) => wire.reader.abort(),
        }
    }
}

pub(super) struct Streamable {
    client: Client,
    url: String,
    headers: HeaderMap,
    /// Assigned by the server on the first response, then echoed back.
    session: Option<HeaderValue>,
    protocol: Option<HeaderValue>,
}

impl Streamable {
    async fn exchange(
        &mut self,
        name: &str,
        method: &str,
        body: &Value,
        id: i64,
        budget: Duration,
    ) -> Result<Value> {
        timeout(budget, async {
            let response = self.post(name, body).await?;
            let kind = content_type(&response);
            if response.status() == StatusCode::ACCEPTED {
                bail!("MCP server `{name}` accepted the request without answering it");
            }
            if kind.starts_with("text/event-stream") {
                return read_stream(name, response, id).await;
            }
            let text = response
                .text()
                .await
                .with_context(|| format!("reading from MCP server `{name}`"))?;
            match matching(&text, id) {
                Some(message) => Ok(message),
                None => bail!("MCP server `{name}` returned no reply to request {id}"),
            }
        })
        .await
        .with_context(|| format!("MCP server `{name}` timed out on {method}"))?
    }

    async fn notify(&mut self, name: &str, body: &Value) -> Result<()> {
        self.post(name, body).await?;
        Ok(())
    }

    async fn post(&mut self, name: &str, body: &Value) -> Result<Response> {
        let mut request = self
            .client
            .post(&self.url)
            .headers(self.headers.clone())
            .header(ACCEPT, "application/json, text/event-stream")
            .header(CONTENT_TYPE, "application/json");
        if let Some(session) = &self.session {
            request = request.header(SESSION_HEADER, session);
        }
        if let Some(protocol) = &self.protocol {
            request = request.header(PROTOCOL_HEADER, protocol);
        }
        let response = request
            .json(body)
            .send()
            .await
            .with_context(|| format!("MCP server `{name}` is not reachable"))?;
        if let Some(session) = response.headers().get(SESSION_HEADER) {
            self.session = Some(session.clone());
        }
        check(name, response).await
    }

    /// Best-effort session teardown. A server that does not implement it says
    /// so with 405, which is not a failure worth reporting.
    async fn end_session(&mut self) {
        let Some(session) = self.session.take() else {
            return;
        };
        let request = self
            .client
            .delete(&self.url)
            .headers(self.headers.clone())
            .header(SESSION_HEADER, session);
        let _ = timeout(CONNECT_TIMEOUT, request.send()).await;
    }
}

/// The 2024-11-05 binding: one long-lived GET stream carries every reply, and
/// messages go out to the endpoint that stream names.
pub(super) struct Sse {
    client: Client,
    endpoint: Url,
    headers: HeaderMap,
    incoming: mpsc::UnboundedReceiver<Value>,
    reader: tokio::task::JoinHandle<()>,
}

impl Sse {
    async fn open(name: &str, url: &str, headers: &HeaderMap, budget: Duration) -> Result<Self> {
        let client = client()?;
        let base =
            Url::parse(url).with_context(|| format!("MCP server `{name}` has an invalid url"))?;
        let response = timeout(
            budget,
            client
                .get(url)
                .headers(headers.clone())
                .header(ACCEPT, "text/event-stream")
                .send(),
        )
        .await
        .with_context(|| format!("MCP server `{name}` timed out opening its event stream"))?
        .with_context(|| format!("MCP server `{name}` is not reachable"))?;
        let response = check(name, response).await?;

        let (messages, incoming) = mpsc::unbounded_channel();
        let (found, endpoint) = oneshot::channel();
        let server = name.to_string();
        let reader = tokio::spawn(read_events(server, base, response, messages, found));

        let endpoint = timeout(budget, endpoint)
            .await
            .with_context(|| format!("MCP server `{name}` never sent its endpoint event"))?
            .with_context(|| format!("MCP server `{name}` closed its event stream at once"))?;
        Ok(Self {
            client,
            endpoint,
            headers: headers.clone(),
            incoming,
            reader,
        })
    }

    async fn exchange(
        &mut self,
        name: &str,
        method: &str,
        body: &Value,
        id: i64,
        budget: Duration,
    ) -> Result<Value> {
        timeout(budget, async {
            // A few servers answer inline instead of over the stream.
            if let Some(message) = self.post(name, body).await?
                && matches_id(&message, id)
            {
                return Ok(message);
            }
            while let Some(message) = self.incoming.recv().await {
                if matches_id(&message, id) {
                    return Ok(message);
                }
            }
            bail!("MCP server `{name}` closed the event stream without answering")
        })
        .await
        .with_context(|| format!("MCP server `{name}` timed out on {method}"))?
    }

    async fn notify(&mut self, name: &str, body: &Value) -> Result<()> {
        self.post(name, body).await?;
        Ok(())
    }

    /// Returns a reply only when the server answered the POST directly.
    async fn post(&mut self, name: &str, body: &Value) -> Result<Option<Value>> {
        let response = self
            .client
            .post(self.endpoint.clone())
            .headers(self.headers.clone())
            .header(CONTENT_TYPE, "application/json")
            .json(body)
            .send()
            .await
            .with_context(|| format!("MCP server `{name}` is not reachable"))?;
        let response = check(name, response).await?;
        if response.status() == StatusCode::ACCEPTED {
            return Ok(None);
        }
        let text = response.text().await.unwrap_or_default();
        Ok(serde_json::from_str(&text).ok())
    }
}

/// Drain the event stream for the life of the connection: the first `endpoint`
/// event names the POST target, and every message event is a JSON-RPC frame.
async fn read_events(
    name: String,
    base: Url,
    response: Response,
    messages: mpsc::UnboundedSender<Value>,
    found: oneshot::Sender<Url>,
) {
    let mut found = Some(found);
    let mut parser = Parser::default();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let Ok(chunk) = chunk else {
            tracing::debug!(server = %name, "MCP event stream ended");
            return;
        };
        for event in parser.feed(&String::from_utf8_lossy(&chunk)) {
            match event.name.as_str() {
                "endpoint" => {
                    let Some(sender) = found.take() else { continue };
                    match base.join(event.data.trim()) {
                        Ok(url) => {
                            let _ = sender.send(url);
                        }
                        Err(e) => {
                            tracing::debug!(server = %name, "bad endpoint event: {e}");
                            return;
                        }
                    }
                }
                "message" | "" => {
                    if let Ok(value) = serde_json::from_str::<Value>(&event.data)
                        && messages.send(value).is_err()
                    {
                        return;
                    }
                }
                _ => {}
            }
        }
    }
}

/// Read a POST's `text/event-stream` reply until the answer to `id` arrives.
async fn read_stream(name: &str, response: Response, id: i64) -> Result<Value> {
    let mut parser = Parser::default();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.with_context(|| format!("reading from MCP server `{name}`"))?;
        for event in parser.feed(&String::from_utf8_lossy(&chunk)) {
            if event.name != "message" && !event.name.is_empty() {
                continue;
            }
            if let Some(message) = matching(&event.data, id) {
                return Ok(message);
            }
        }
    }
    bail!("MCP server `{name}` closed the stream without answering request {id}")
}

/// A non-2xx response, turned into a reason a person can act on. The body is
/// included because MCP servers explain refusals there.
async fn check(name: &str, response: Response) -> Result<Response> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let detail = response.text().await.unwrap_or_default();
    let detail = detail.trim();
    if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
        bail!("MCP server `{name}` needs auth: it answered {status}");
    }
    match detail.is_empty() {
        true => bail!("MCP server `{name}` answered {status}"),
        false => bail!(
            "MCP server `{name}` answered {status}: {}",
            detail.chars().take(200).collect::<String>()
        ),
    }
}

fn content_type(response: &Response) -> String {
    response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

/// The JSON-RPC message answering `id`, from a single frame or a batch.
fn matching(text: &str, id: i64) -> Option<Value> {
    let value: Value = serde_json::from_str(text.trim()).ok()?;
    if let Some(batch) = value.as_array() {
        return batch.iter().find(|item| matches_id(item, id)).cloned();
    }
    matches_id(&value, id).then_some(value)
}

fn matches_id(message: &Value, id: i64) -> bool {
    message.get("id").and_then(Value::as_i64) == Some(id)
}

fn client() -> Result<Client> {
    Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .redirect(same_origin_only())
        .user_agent(concat!("aster/", env!("CARGO_PKG_VERSION")))
        .build()
        .context("building the HTTP client for MCP")
}

/// Configured headers may carry credentials, so a redirect that leaves the
/// origin is refused rather than followed.
fn same_origin_only() -> redirect::Policy {
    redirect::Policy::custom(move |attempt| {
        if attempt.previous().len() > MAX_REDIRECTS {
            return attempt.error("too many redirects");
        }
        match attempt.previous().last() {
            Some(from) if from.origin() == attempt.url().origin() => attempt.follow(),
            Some(_) => attempt.stop(),
            None => attempt.follow(),
        }
    })
}

fn header_map(headers: &BTreeMap<String, String>) -> Result<HeaderMap> {
    let mut map = HeaderMap::new();
    for (name, value) in headers {
        let name = HeaderName::from_bytes(name.as_bytes())
            .with_context(|| format!("{name:?} is not a valid HTTP header name"))?;
        let value = HeaderValue::from_str(value)
            .with_context(|| format!("the {name} header value is not valid"))?;
        map.insert(name, value);
    }
    Ok(map)
}

/// Incremental `text/event-stream` parser. Events are separated by a blank
/// line, and a frame can be split across chunks.
#[derive(Default)]
pub(super) struct Parser {
    buffer: String,
}

pub(super) struct Event {
    pub name: String,
    pub data: String,
}

impl Parser {
    pub(super) fn feed(&mut self, chunk: &str) -> Vec<Event> {
        self.buffer
            .push_str(&chunk.replace("\r\n", "\n").replace('\r', "\n"));
        let mut events = Vec::new();
        while let Some(end) = self.buffer.find("\n\n") {
            let frame: String = self.buffer.drain(..end + 2).collect();
            if let Some(event) = parse_frame(&frame) {
                events.push(event);
            }
        }
        events
    }
}

fn parse_frame(frame: &str) -> Option<Event> {
    let mut name = String::new();
    let mut data: Vec<&str> = Vec::new();
    for line in frame.lines() {
        if line.is_empty() || line.starts_with(':') {
            continue;
        }
        let (field, value) = match line.split_once(':') {
            Some((field, value)) => (field, value.strip_prefix(' ').unwrap_or(value)),
            None => (line, ""),
        };
        match field {
            "event" => name = value.to_string(),
            "data" => data.push(value),
            _ => {}
        }
    }
    if data.is_empty() {
        return None;
    }
    Some(Event {
        name,
        data: data.join("\n"),
    })
}
