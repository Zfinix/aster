//! MCP transport and session runtime. `aster-mcp` owns discovery and routing
//! and deliberately has no transport, so the host supplies one: JSON-RPC 2.0
//! over a child process's stdio, the standard for local MCP servers.

use std::collections::{BTreeMap, VecDeque};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use aster_mcp::{Injection, Injector, McpCatalog, McpTool, ProgressiveConfig};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::Mutex;
use tokio::time::timeout;

/// A server that never answers must not hang the turn.
const CALL_TIMEOUT: Duration = Duration::from_secs(30);
/// Handshake and listing happen at startup, where a slow server delays the
/// whole session, so they get a tighter budget than a tool call.
const STARTUP_TIMEOUT: Duration = Duration::from_secs(10);
/// A legacy server may never answer `server/discover`, so the probe waits only
/// long enough to tell silence from slowness. A server that speaks the modern
/// protocol answers in single-digit milliseconds, so this is a hair-trigger:
/// every legacy server pays it in full, on every session.
const PROBE_TIMEOUT: Duration = Duration::from_millis(400);
/// Backstop against a server that paginates without ever ending.
const MAX_TOOLS_PER_SERVER: usize = 2_000;
/// How long a server gets to exit on its own after its stdin closes.
const SHUTDOWN_GRACE: Duration = Duration::from_millis(500);
/// Stderr lines kept per server: enough to hold a whole Node crash dump
/// (message plus stack frames), bounded so a chatty server cannot grow memory.
const STDERR_TAIL_LINES: usize = 40;
/// Longest stderr line kept; anything past this is a dump, not a reason.
const STDERR_LINE_CHARS: usize = 400;
/// Newest revision we speak. Modern revisions carry it per request instead of
/// negotiating once.
const PROTOCOL_VERSION: &str = "2026-07-28";
/// Sent in the `initialize` handshake when a server turns out to predate
/// per-request metadata.
const LEGACY_PROTOCOL_VERSION: &str = "2025-11-25";
/// `UnsupportedProtocolVersionError`. Identifies a modern server even when it
/// refuses our version, so it must not trigger the legacy fallback.
const UNSUPPORTED_PROTOCOL_VERSION: i64 = -32022;

/// Which era a server turned out to speak. Determined once per process and
/// then reused, as the spec requires.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Era {
    /// Per-request `_meta`, no handshake.
    Modern { version: String },
    /// `initialize` handshake, state held by the connection.
    Legacy,
}

/// One MCP server as declared in `aster.yaml`.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct ServerConfig {
    /// Executable to spawn, e.g. `npx`.
    pub command: String,
    pub args: Vec<String>,
    /// Extra environment for the child, merged over the inherited environment.
    pub env: BTreeMap<String, String>,
    /// Skip this server without deleting its configuration.
    pub disabled: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct McpSettings {
    pub servers: BTreeMap<String, ServerConfig>,
    /// Context the inventory is measured against.
    pub context_tokens: usize,
    /// Share of `context_tokens` the tool inventory may spend. Above it the
    /// prompt lists servers only and the model searches for what it needs.
    pub inventory_percent: f32,
    /// Matches returned by one `search`.
    pub search_limit: usize,
}

impl Default for McpSettings {
    fn default() -> Self {
        Self {
            servers: BTreeMap::new(),
            context_tokens: 100_000,
            // Deliberately below the crate default: an inventory is a standing
            // cost on every turn, and search recovers anything it hides.
            inventory_percent: 1.5,
            search_limit: 10,
        }
    }
}

impl McpSettings {
    fn progressive(&self) -> ProgressiveConfig {
        let default = ProgressiveConfig::default();
        ProgressiveConfig {
            available_context_tokens: self.context_tokens.max(1),
            inventory_threshold_percent: self.inventory_percent.clamp(0.01, 100.0),
            search_default_limit: self.search_limit.clamp(1, default.max_search_limit),
            max_search_limit: default.max_search_limit,
        }
    }
}

/// A live connection to one MCP server.
struct Connection {
    name: String,
    child: Child,
    /// Taken on shutdown: closing stdin is the portable signal telling the
    /// server to exit.
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
    next_id: i64,
    era: Era,
    /// Last stderr lines, kept so a dead server's reason survives it.
    stderr_tail: Arc<std::sync::Mutex<VecDeque<String>>>,
    /// Drains stderr for the life of the child; finishes at EOF.
    stderr_task: tokio::task::JoinHandle<()>,
}

/// One JSON-RPC failure, kept structured so the probe can tell an
/// `UnsupportedProtocolVersionError` (modern server) from anything else
/// (legacy server).
#[derive(Debug)]
struct RpcError {
    code: i64,
    message: String,
    data: Value,
}

impl std::fmt::Display for RpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} (code {})", self.message, self.code)
    }
}

impl Connection {
    async fn spawn(name: &str, config: &ServerConfig) -> Result<Self> {
        let mut command = Command::new(&config.command);
        command
            .args(&config.args)
            .envs(&config.env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // Servers log to stderr; a tail is kept for post-mortems, and
            // inheriting it would corrupt the TUI.
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = command.spawn().map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => {
                anyhow::anyhow!("is not installed (no `{}` on PATH)", config.command)
            }
            _ => anyhow::Error::new(e).context(format!("could not start ({})", config.command)),
        })?;
        let stdin = child
            .stdin
            .take()
            .context("MCP server stdin was not piped")?;
        let stdout = child
            .stdout
            .take()
            .context("MCP server stdout was not piped")?;
        let stderr = child
            .stderr
            .take()
            .context("MCP server stderr was not piped")?;
        let stderr_tail = Arc::new(std::sync::Mutex::new(VecDeque::new()));
        let tail = Arc::clone(&stderr_tail);
        let stderr_task = tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let Ok(mut tail) = tail.lock() else { return };
                if tail.len() >= STDERR_TAIL_LINES {
                    tail.pop_front();
                }
                tail.push_back(line.chars().take(STDERR_LINE_CHARS).collect());
            }
        });
        Ok(Self {
            name: name.to_string(),
            child,
            stdin: Some(stdin),
            stdout: BufReader::new(stdout),
            next_id: 1,
            era: Era::Modern {
                version: PROTOCOL_VERSION.to_string(),
            },
            stderr_tail,
            stderr_task,
        })
    }

    /// The per-request metadata a modern server needs to serve a request with
    /// no prior connection state. Legacy servers get no `_meta` at all.
    fn meta(&self) -> Option<Value> {
        let Era::Modern { version } = &self.era else {
            return None;
        };
        Some(json!({
            "io.modelcontextprotocol/protocolVersion": version,
            "io.modelcontextprotocol/clientCapabilities": {},
            "io.modelcontextprotocol/clientInfo": {
                "name": "aster",
                "version": env!("CARGO_PKG_VERSION"),
            },
        }))
    }

    async fn request(&mut self, method: &str, params: Value, budget: Duration) -> Result<Value> {
        self.try_request(method, params, budget)
            .await?
            .map_err(|e| anyhow::anyhow!("MCP server `{}` rejected {method}: {e}", self.name))
    }

    /// One JSON-RPC round trip. The outer `Result` is a transport failure; the
    /// inner one is the server's own error, which the probe inspects rather
    /// than treats as fatal. Notifications are skipped, not mistaken for the
    /// reply.
    async fn try_request(
        &mut self,
        method: &str,
        mut params: Value,
        budget: Duration,
    ) -> Result<std::result::Result<Value, RpcError>> {
        let id = self.next_id;
        self.next_id += 1;
        if let Some(meta) = self.meta()
            && let Some(object) = params.as_object_mut()
        {
            object.insert("_meta".into(), meta);
        }
        let body = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        self.send(&body).await?;

        timeout(budget, async {
            loop {
                let mut line = String::new();
                let read = self
                    .stdout
                    .read_line(&mut line)
                    .await
                    .with_context(|| format!("reading from MCP server `{}`", self.name))?;
                if read == 0 {
                    bail!("MCP server `{}` closed the connection", self.name);
                }
                let Ok(message) = serde_json::from_str::<Value>(line.trim()) else {
                    continue;
                };
                if message.get("id").and_then(Value::as_i64) != Some(id) {
                    continue;
                }
                if let Some(error) = message.get("error") {
                    return Ok(Err(RpcError {
                        code: error.get("code").and_then(Value::as_i64).unwrap_or(0),
                        message: error
                            .get("message")
                            .and_then(Value::as_str)
                            .unwrap_or("unknown error")
                            .to_string(),
                        data: error.get("data").cloned().unwrap_or(Value::Null),
                    }));
                }
                return Ok(Ok(message.get("result").cloned().unwrap_or(Value::Null)));
            }
        })
        .await
        .with_context(|| format!("MCP server `{}` timed out on {method}", self.name))?
    }

    async fn notify(&mut self, method: &str, params: Value) -> Result<()> {
        self.send(&json!({ "jsonrpc": "2.0", "method": method, "params": params }))
            .await
    }

    async fn send(&mut self, body: &Value) -> Result<()> {
        let mut line = serde_json::to_string(body)?;
        line.push('\n');
        let stdin = self
            .stdin
            .as_mut()
            .with_context(|| format!("MCP server `{}` is shut down", self.name))?;
        stdin
            .write_all(line.as_bytes())
            .await
            .with_context(|| format!("writing to MCP server `{}`", self.name))?;
        stdin.flush().await?;
        Ok(())
    }

    /// Settle which era the server speaks, then list what it advertises.
    async fn start(&mut self) -> Result<Vec<McpTool>> {
        let discovered = self.detect_era().await?;
        if self.era == Era::Legacy {
            self.initialize().await?;
        }
        // A modern server tells us up front whether it has tools at all, so a
        // resources-only server is quiet rather than an error.
        if let Some(capabilities) = discovered.as_ref().and_then(|d| d.get("capabilities"))
            && capabilities.is_object()
            && capabilities.get("tools").is_none()
        {
            return Ok(Vec::new());
        }
        self.list_tools().await
    }

    /// Walk every page. `tools/list` is paginated, and stopping at the first
    /// page would silently hide most of a large catalog.
    async fn list_tools(&mut self) -> Result<Vec<McpTool>> {
        let mut tools = Vec::new();
        let mut cursor: Option<String> = None;
        loop {
            let params = match &cursor {
                Some(cursor) => json!({ "cursor": cursor }),
                None => json!({}),
            };
            let page = self.request("tools/list", params, STARTUP_TIMEOUT).await?;
            if let Some(entries) = page.get("tools").and_then(Value::as_array) {
                tools.extend(entries.iter().filter_map(|entry| self.tool_from(entry)));
            }
            cursor = page
                .get("nextCursor")
                .and_then(Value::as_str)
                .map(str::to_string);
            // A server that keeps handing back a cursor would loop forever.
            if cursor.is_none() || tools.len() >= MAX_TOOLS_PER_SERVER {
                break;
            }
        }
        Ok(tools)
    }

    /// Probe with `server/discover`, as the stdio binding requires. A result or
    /// an `UnsupportedProtocolVersionError` means modern; any other error, or
    /// silence, means the server predates per-request metadata.
    async fn detect_era(&mut self) -> Result<Option<Value>> {
        let probe = self
            .try_request("server/discover", json!({}), PROBE_TIMEOUT)
            .await;
        let mut discovered = None;
        self.era = match probe {
            Ok(Ok(result)) => {
                let version = pick_version(result.get("supportedVersions"))
                    .unwrap_or_else(|| PROTOCOL_VERSION.to_string());
                discovered = Some(result);
                Era::Modern { version }
            }
            Ok(Err(error)) if error.code == UNSUPPORTED_PROTOCOL_VERSION => {
                let version = pick_version(error.data.get("supported")).with_context(|| {
                    format!(
                        "MCP server `{}` supports no protocol version Aster speaks: {}",
                        self.name, error.data
                    )
                })?;
                Era::Modern { version }
            }
            // An unrecognized error or a timeout is the legacy signal. It must
            // not be keyed to one code: legacy servers answer unknown methods
            // however they like, or not at all.
            Ok(Err(_)) | Err(_) => Era::Legacy,
        };
        Ok(discovered)
    }

    /// Start straight into the legacy handshake with no probe, for servers
    /// that treat an unknown method before `initialize` as fatal.
    async fn start_legacy(&mut self) -> Result<Vec<McpTool>> {
        self.era = Era::Legacy;
        self.initialize().await?;
        self.list_tools().await
    }

    /// The pre-2026 handshake, used only once the probe says the server needs it.
    async fn initialize(&mut self) -> Result<()> {
        self.request(
            "initialize",
            json!({
                "protocolVersion": LEGACY_PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": { "name": "aster", "version": env!("CARGO_PKG_VERSION") },
            }),
            STARTUP_TIMEOUT,
        )
        .await?;
        self.notify("notifications/initialized", json!({})).await
    }

    /// Skips malformed entries: one bad tool should not cost the whole server.
    fn tool_from(&self, entry: &Value) -> Option<McpTool> {
        let name = entry.get("name")?.as_str()?.to_string();
        let description = entry
            .get("description")
            .and_then(Value::as_str)
            .filter(|d| !d.trim().is_empty())
            .unwrap_or("no description provided")
            .to_string();
        let input_schema = match entry.get("inputSchema") {
            Some(schema) if schema.is_object() => schema.clone(),
            _ => json!({ "type": "object" }),
        };
        Some(McpTool {
            server: self.name.clone(),
            name,
            description,
            input_schema,
        })
    }

    async fn call(&mut self, tool: &str, arguments: &Value) -> Result<Value> {
        self.request(
            "tools/call",
            json!({ "name": tool, "arguments": arguments }),
            CALL_TIMEOUT,
        )
        .await
    }

    /// Turn a startup failure into one short line saying what to do about it.
    /// The raw error and full stderr go to the debug log, not the user.
    async fn diagnose(mut self, error: anyhow::Error) -> anyhow::Error {
        // Stderr EOF means the child is gone and its last words are in the
        // tail. A server that is still alive times out here instead.
        let _ = timeout(SHUTDOWN_GRACE, &mut self.stderr_task).await;
        let status = match timeout(SHUTDOWN_GRACE, self.child.wait()).await {
            Ok(Ok(status)) => Some(status),
            _ => None,
        };
        let tail = self
            .stderr_tail
            .lock()
            .map(|tail| tail.clone())
            .unwrap_or_default();
        tracing::debug!(
            server = %self.name, ?status, stderr = ?tail,
            "MCP server failed to start: {error:#}"
        );
        let headline = stderr_headline(&tail);
        // Checked before the exit status: an OAuth server that is blocked
        // waiting for the user to open its auth route never exits at all.
        let words: Vec<&str> = tail.iter().map(String::as_str).collect();
        if looks_like_auth_failure(&words.join("\n")) {
            return match (auth_url(&tail), headline) {
                (Some(url), _) => anyhow::anyhow!("needs auth: sign in at {url}"),
                (None, Some(line)) => anyhow::anyhow!("needs auth: {}", brief(&line)),
                (None, None) => anyhow::anyhow!("needs auth (check its credentials)"),
            };
        }
        if status.is_none() {
            return anyhow::anyhow!("is not responding (startup timed out)");
        }
        match headline {
            Some(line) => anyhow::anyhow!("crashed: {}", brief(&line)),
            None => anyhow::anyhow!("crashed on startup"),
        }
    }
}

/// One short reason from a stderr line: error-name prefixes, "imported from"
/// trailers, and anything past 80 chars are noise in a one-line report.
fn brief(line: &str) -> String {
    let mut reason = line.trim();
    if let Some((prefix, rest)) = reason.split_once(':')
        && prefix.len() <= 40
        && prefix.to_ascii_lowercase().contains("error")
        && !rest.trim().is_empty()
    {
        reason = rest.trim();
    }
    if let Some(at) = reason.find(" imported from ") {
        reason = reason[..at].trim_end();
    }
    const REASON_CHARS: usize = 80;
    if reason.chars().count() <= REASON_CHARS {
        return reason.to_string();
    }
    let cut: String = reason.chars().take(REASON_CHARS).collect();
    format!("{}…", cut.trim_end())
}

/// Whether a dying server's stderr reads like a credentials problem rather
/// than a crash. Bare "token" is excluded: parsers also complain about those.
fn looks_like_auth_failure(stderr: &str) -> bool {
    const SIGNS: [&str; 17] = [
        "unauthorized",
        "unauthenticated",
        "not authenticated",
        "authentication",
        "authorization",
        "401",
        "403",
        "forbidden",
        "api key",
        "api_key",
        "apikey",
        "access token",
        "access_token",
        "auth token",
        "credential",
        "login",
        "oauth",
    ];
    let text = stderr.to_ascii_lowercase();
    SIGNS.iter().any(|sign| text.contains(sign))
}

/// The login link the server printed, if any: OAuth-based servers hand the
/// user a URL to open, so the report should carry it rather than bury it.
fn auth_url(tail: &VecDeque<String>) -> Option<String> {
    tail.iter().find_map(|line| {
        let start = line.find("https://").or_else(|| line.find("http://"))?;
        let url = line[start..]
            .split_whitespace()
            .next()?
            .trim_end_matches(['.', ',', ')', ']', '"', '\'']);
        Some(url.to_string())
    })
}

/// The stderr line most likely to say why the server died: the first one
/// naming an error, else the last non-empty one.
fn stderr_headline(tail: &VecDeque<String>) -> Option<String> {
    let lines: Vec<&str> = tail
        .iter()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .collect();
    lines
        .iter()
        .find(|line| line.to_ascii_lowercase().contains("error"))
        .or(lines.last())
        .map(|line| line.to_string())
}

/// Live MCP servers plus the injector that decides what the model sees.
#[derive(Clone)]
pub struct McpRuntime {
    injector: Arc<Injector>,
    connections: Arc<Mutex<BTreeMap<String, Connection>>>,
    disabled_servers: Vec<DisabledServer>,
}

/// A server the agent knows about but cannot use yet — it can ask the user
/// for permission to enable it.
#[derive(Clone)]
pub struct DisabledServer {
    pub name: String,
    /// What the server does, so the agent can decide when to ask.
    pub description: String,
}

impl McpRuntime {
    /// Connect every enabled server. A server that fails to start is reported
    /// and skipped, so one broken entry cannot block the session.
    pub async fn connect(settings: &McpSettings) -> (Option<Self>, Vec<String>) {
        let mut connections = BTreeMap::new();
        let mut tools = Vec::new();
        let mut problems = Vec::new();
        let mut disabled_servers = Vec::new();

        // Front-ends inject session-scoped servers via ASTER_MCP_EXTRA, a JSON
        // map of server configs; the bridge uses it for its telegram tools.
        let mut servers = settings.servers.clone();
        if let Ok(raw) = std::env::var("ASTER_MCP_EXTRA") {
            match serde_json::from_str::<BTreeMap<String, ServerConfig>>(&raw) {
                Ok(extra) => servers.extend(extra),
                Err(e) => problems.push(format!("ASTER_MCP_EXTRA is not valid: {e}")),
            }
        }

        // Servers start concurrently: each costs a process spawn and a probe,
        // and serially that is the whole session's startup budget.
        let starting: Vec<_> = servers
            .iter()
            .filter(|(_, config)| {
                if config.disabled || config.command.trim().is_empty() {
                    return false;
                }
                true
            })
            .map(
                |(name, config)| async move { (name.clone(), Self::start_one(name, config).await) },
            )
            .collect();
        for (name, config) in &servers {
            if config.disabled {
                disabled_servers.push(DisabledServer {
                    name: name.clone(),
                    description: describe_disabled_server(name),
                });
            }
        }
        for (name, started) in futures_util::future::join_all(starting).await {
            match started {
                Ok((connection, listed)) => {
                    tools.extend(listed);
                    connections.insert(name, connection);
                }
                Err(e) => problems.push(format!("{name} {e:#}")),
            }
        }

        if tools.is_empty() && disabled_servers.is_empty() {
            return (None, problems);
        }
        let injector = match McpCatalog::new(tools) {
            Ok(catalog) => match Injector::new(catalog, settings.progressive()) {
                Ok(injector) => injector,
                Err(e) => {
                    problems.push(format!("MCP injector rejected: {e:#}"));
                    return (None, problems);
                }
            },
            Err(e) => {
                problems.push(format!("MCP catalog rejected: {e:#}"));
                return (None, problems);
            }
        };
        let runtime = Self {
            injector: Arc::new(injector),
            connections: Arc::new(Mutex::new(connections)),
            disabled_servers,
        };
        (Some(runtime), problems)
    }

    async fn start_one(name: &str, config: &ServerConfig) -> Result<(Connection, Vec<McpTool>)> {
        let mut connection = Connection::spawn(name, config).await?;
        let error = match connection.start().await {
            Ok(tools) => return Ok((connection, tools)),
            Err(error) => error,
        };
        // A strict legacy server treats the modern probe itself as fatal and
        // exits. When the child is dead, one respawn straight into the legacy
        // handshake tells that server apart from a genuinely broken one.
        let died = matches!(
            timeout(SHUTDOWN_GRACE, connection.child.wait()).await,
            Ok(Ok(_))
        );
        if !died {
            return Err(connection.diagnose(error).await);
        }
        let mut retry = Connection::spawn(name, config).await?;
        match retry.start_legacy().await {
            Ok(tools) => Ok((retry, tools)),
            Err(retry_error) => Err(retry.diagnose(retry_error).await),
        }
    }

    pub fn injector(&self) -> &Injector {
        &self.injector
    }

    pub fn injection(&self) -> Option<Injection> {
        self.injector.inject()
    }

    #[allow(dead_code)] // used in tests
    pub fn tool_count(&self) -> usize {
        self.injector.catalog().len()
    }

    /// Prompt section listing disabled servers the agent can ask the user to enable.
    pub fn disabled_servers_prompt(&self) -> Option<String> {
        if self.disabled_servers.is_empty() {
            return None;
        }
        let mut prompt = String::from(
            "Disabled MCP servers are available but not running. Do not mention them \
             unless the user asks about them, or a task genuinely cannot be done \
             another way. For web fetches, prefer `run_command` with `curl` (or a \
             similar CLI) first; only suggest enabling a browser server when the task \
             needs a real browser (JavaScript-heavy pages, screenshots, interaction). \
             To enable one, edit its `disabled: true` line in aster.yaml to \
             `disabled: false`. The user will need to restart the session for the \
             change to take effect.\n\n",
        );
        for ds in &self.disabled_servers {
            prompt.push_str(&format!("- {}: {}\n", ds.name, ds.description));
        }
        Some(prompt)
    }

    pub fn server_names(&self) -> Vec<String> {
        self.injector
            .catalog()
            .servers()
            .iter()
            .map(|s| s.name.clone())
            .collect()
    }

    /// Invoke an already-resolved tool. The caller authorizes `tool.id()`
    /// before reaching here.
    pub async fn call(&self, tool: &McpTool, arguments: &Value) -> Result<Value> {
        let mut connections = self.connections.lock().await;
        let connection = connections
            .get_mut(&tool.server)
            .with_context(|| format!("MCP server `{}` is not connected", tool.server))?;
        connection.call(&tool.name, arguments).await
    }

    /// Terminate every child. Dropping alone would rely on `kill_on_drop`,
    /// which cannot report failures.
    pub async fn shutdown(&self) {
        let mut connections = self.connections.lock().await;
        for (name, connection) in connections.iter_mut() {
            // Closing stdin is the graceful signal; killing is the fallback for
            // a server that ignores it.
            drop(connection.stdin.take());
            let exited = timeout(SHUTDOWN_GRACE, connection.child.wait()).await;
            if exited.is_err()
                && let Err(e) = connection.child.start_kill()
            {
                tracing::debug!("could not stop MCP server `{name}`: {e}");
            }
        }
        connections.clear();
    }
}

/// The newest version in `offered` that Aster also speaks. Versions are
/// date-stamped, so lexical order is chronological order.
fn pick_version(offered: Option<&Value>) -> Option<String> {
    let ours = [PROTOCOL_VERSION, LEGACY_PROTOCOL_VERSION];
    offered?
        .as_array()?
        .iter()
        .filter_map(Value::as_str)
        .filter(|v| ours.contains(v))
        .max()
        .map(str::to_string)
}

/// Flatten an MCP `tools/call` result into text for the model. The spec puts
/// the payload in `content`, a list of typed parts.
pub fn render_result(result: &Value) -> String {
    // Modern results are typed. An absent `resultType` means "complete", which
    // is what every pre-2026 server returns.
    match result.get("resultType").and_then(Value::as_str) {
        None | Some("complete") => {}
        Some("input_required") => {
            return "the MCP tool needs more input to finish; Aster does not yet \
                    carry multi round-trip requests, so treat this call as unfinished"
                .to_string();
        }
        Some(other) => return format!("the MCP tool returned an unsupported result type: {other}"),
    }
    let mut out = String::new();
    let parts = result
        .get("content")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for part in &parts {
        match part.get("type").and_then(Value::as_str) {
            Some("text") => {
                if let Some(text) = part.get("text").and_then(Value::as_str) {
                    if !out.is_empty() {
                        out.push('\n');
                    }
                    out.push_str(text);
                }
            }
            // Images and audio are not renderable in a text transcript; name
            // them so the model knows something came back.
            Some(kind) => {
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(&format!("[{kind} content omitted]"));
            }
            None => {}
        }
    }
    // A tool with an `outputSchema` returns `structuredContent` and *should*
    // repeat it as text. When it does not, that field is the whole answer.
    if out.is_empty()
        && let Some(structured) = result.get("structuredContent")
    {
        out = serde_json::to_string_pretty(structured).unwrap_or_else(|_| structured.to_string());
    }
    if result.get("isError").and_then(Value::as_bool) == Some(true) {
        return format!("the MCP tool reported an error: {out}");
    }
    if out.is_empty() {
        return result.to_string();
    }
    out
}

#[derive(clap::Args)]
pub struct McpArgs {
    #[command(subcommand)]
    pub action: McpAction,
}

#[derive(clap::Subcommand)]
pub enum McpAction {
    /// Connect every configured server and list the tools it advertises.
    List,
    /// Start a configured server so it can be used (clears `disabled`).
    Enable { name: String },
    /// Stop starting a server, keeping its configuration in place.
    Disable { name: String },
    /// Copy MCP servers from another coding tool (Claude Code, Codex, Cursor, opencode, Hermes) into .mcp.json.
    Import {
        /// Which tool to read; omitted, all of them are tried.
        #[arg(long, value_enum)]
        from: Option<crate::import::Source>,
    },
    /// Delete servers from the config files declaring them. Without a name, pick interactively.
    Remove { name: Option<String> },
}

/// `aster mcp list`: start the configured servers, report what they expose,
/// then stop them. The same connection path a chat session uses, so a failure
/// here is the failure a session would hit.
pub async fn run(args: McpArgs, repo_root: Option<&std::path::Path>) -> Result<()> {
    match &args.action {
        McpAction::List => {}
        McpAction::Enable { name } => return set_disabled(repo_root, name, false),
        McpAction::Disable { name } => return set_disabled(repo_root, name, true),
        McpAction::Import { from } => return crate::import::run_mcp_import(*from, repo_root),
        McpAction::Remove { name } => return remove_servers(repo_root, name.as_deref()),
    }
    let settings = crate::settings::Settings::load(repo_root)?;
    if settings.mcp.servers.is_empty() {
        if crate::json_mode() {
            println!("{}", json!({ "ok": true, "servers": [], "tools": [] }));
        } else {
            println!(
                "No MCP servers configured. Add them under `mcp.servers` in aster.yaml:\n\n\
                 mcp:\n  servers:\n    github:\n      command: npx\n      args: [\"-y\", \"@modelcontextprotocol/server-github\"]"
            );
        }
        return Ok(());
    }

    let (runtime, problems) = McpRuntime::connect(&settings.mcp).await;
    let tools: Vec<&McpTool> = runtime
        .as_ref()
        .map(|r| r.injector().catalog().tools().iter().collect())
        .unwrap_or_default();

    if crate::json_mode() {
        println!(
            "{}",
            json!({
                "ok": problems.is_empty(),
                "problems": problems,
                "servers": runtime.as_ref().map(|r| r.server_names()).unwrap_or_default(),
                "tools": tools.iter().map(|t| json!({
                    "id": t.id(),
                    "description": t.description,
                })).collect::<Vec<_>>(),
            })
        );
    } else {
        for problem in &problems {
            eprintln!("error: {problem}");
        }
        match runtime.as_ref() {
            Some(runtime) => {
                let injection = runtime.injection();
                println!(
                    "{} tools across {}",
                    tools.len(),
                    runtime.server_names().join(", ")
                );
                if let Some(injection) = injection {
                    let listed = matches!(injection.inventory, aster_mcp::Inventory::Tools(_));
                    println!(
                        "prompt cost {} tokens, budget {} ({})",
                        injection.manifest_tokens,
                        injection.inventory_budget_tokens,
                        if listed {
                            "tools listed by name"
                        } else {
                            "over budget, servers only; the agent searches"
                        }
                    );
                }
                print_tool_tree(tools);
            }
            None if problems.is_empty() => println!("No servers advertised any tools."),
            None => {}
        }
    }

    if let Some(runtime) = runtime {
        runtime.shutdown().await;
    }
    Ok(())
}

/// One tree per server: tool names aligned under the server heading, each
/// description cut to its first line and the terminal width.
fn print_tool_tree(tools: Vec<&McpTool>) {
    use std::collections::BTreeMap;
    let width = console::Term::stdout().size().1 as usize;
    let mut by_server: BTreeMap<&str, Vec<(&str, &str)>> = BTreeMap::new();
    let ids: Vec<String> = tools.iter().map(|t| t.id()).collect();
    for (tool, id) in tools.iter().zip(&ids) {
        let (server, name) = id.split_once('/').unwrap_or(("", id.as_str()));
        let first = tool.description.lines().next().unwrap_or_default().trim();
        by_server.entry(server).or_default().push((name, first));
    }
    for (server, mut rows) in by_server {
        rows.sort();
        println!(
            "\n{} {}",
            console::style(server).bold(),
            console::style(format!("({} tools)", rows.len())).dim()
        );
        let pad = rows.iter().map(|(name, _)| name.len()).max().unwrap_or(0);
        for (i, (name, desc)) in rows.iter().enumerate() {
            let glyph = if i + 1 == rows.len() {
                "└─"
            } else {
                "├─"
            };
            let head = format!("{glyph} {name:<pad$}  ");
            let room = width
                .saturating_sub(console::measure_text_width(&head) + 1)
                .clamp(20, 100);
            println!(
                "{glyph} {name:<pad$}  {}",
                console::style(console::truncate_str(desc, room, "…")).dim()
            );
        }
    }
}

/// Flip `disabled` for one server without printing, returning the file
/// edited. YAML is edited as text so comments and ordering survive;
/// `.mcp.json` as JSON. Shared by the CLI and the TUI control panel.
pub fn toggle_server(
    repo_root: Option<&std::path::Path>,
    name: &str,
    disabled: bool,
) -> Result<std::path::PathBuf> {
    match config_declaring(repo_root, name) {
        Some(path) => {
            let text = std::fs::read_to_string(&path)
                .with_context(|| format!("reading {}", path.display()))?;
            let updated = toggled(&text, name, disabled)
                .ok_or_else(|| anyhow::anyhow!("could not find {name:?} in {}", path.display()))?;
            std::fs::write(&path, updated)
                .with_context(|| format!("writing {}", path.display()))?;
            Ok(path)
        }
        None => mcp_json_set_disabled(repo_root, name, disabled)?.ok_or_else(|| {
            anyhow::anyhow!(
                "no MCP server named {name:?} in aster.yaml or .mcp.json. \
                 `aster mcp list` shows what is configured"
            )
        }),
    }
}

fn set_disabled(repo_root: Option<&std::path::Path>, name: &str, disabled: bool) -> Result<()> {
    let path = toggle_server(repo_root, name, disabled)?;
    let verb = if disabled { "disabled" } else { "enabled" };
    if crate::json_mode() {
        println!(
            "{}",
            json!({ "ok": true, "server": name, "disabled": disabled,
                    "path": path.display().to_string() })
        );
    } else {
        println!("{verb} {name} in {}", path.display());
        if !disabled {
            println!("Check it starts: aster mcp list");
        }
    }
    Ok(())
}

/// `aster mcp remove`: delete servers from every config file declaring them,
/// picking interactively when no name was given.
fn remove_servers(repo_root: Option<&std::path::Path>, name: Option<&str>) -> Result<()> {
    let settings = crate::settings::Settings::load(repo_root)?;
    let chosen: Vec<String> = match name {
        Some(name) => {
            anyhow::ensure!(
                settings.mcp.servers.contains_key(name),
                "no MCP server named {name:?}. `aster mcp list` shows what is configured"
            );
            vec![name.to_string()]
        }
        None if crate::picker::is_tty() => {
            let items: Vec<crate::picker::Item> = settings
                .mcp
                .servers
                .iter()
                .map(|(name, config)| crate::picker::Item {
                    name: name.clone(),
                    detail: format!("{} {}", config.command, config.args.join(" ")),
                })
                .collect();
            if items.is_empty() {
                println!("no MCP servers configured");
                return Ok(());
            }
            let Some(chosen) =
                crate::picker::multi_select("Select MCP servers to remove", &items, false)?
            else {
                println!("cancelled, nothing removed");
                return Ok(());
            };
            let names: Vec<String> = settings.mcp.servers.keys().cloned().collect();
            chosen.into_iter().map(|i| names[i].clone()).collect()
        }
        None => anyhow::bail!("a server name is required outside a terminal"),
    };
    if chosen.is_empty() {
        println!("nothing selected, nothing removed");
        return Ok(());
    }

    let mut removed = Vec::new();
    for name in &chosen {
        let paths = remove_everywhere(repo_root, name)?;
        anyhow::ensure!(
            !paths.is_empty(),
            "{name:?} is not declared in any aster.yaml or .mcp.json this repo reads"
        );
        removed.push((name.clone(), paths));
    }

    if crate::json_mode() {
        println!(
            "{}",
            json!({
                "ok": true,
                "removed": removed.iter().map(|(name, paths)| json!({
                    "server": name,
                    "paths": paths.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
                })).collect::<Vec<_>>(),
            })
        );
        return Ok(());
    }
    for (name, paths) in removed {
        for path in paths {
            println!("removed {name} from {}", path.display());
        }
    }
    Ok(())
}

/// Every config file this repo reads that declares `name`, edited in place.
fn remove_everywhere(
    repo_root: Option<&std::path::Path>,
    name: &str,
) -> Result<Vec<std::path::PathBuf>> {
    let mut edited = Vec::new();
    for path in yaml_config_paths(repo_root) {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        if let Some(updated) = without_server(&text, name) {
            std::fs::write(&path, updated)
                .with_context(|| format!("writing {}", path.display()))?;
            edited.push(path);
        }
    }
    for path in mcp_json_paths(repo_root) {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let mut config: Value =
            serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        let Some(servers) = config.get_mut("mcpServers").and_then(|v| v.as_object_mut()) else {
            continue;
        };
        if servers.remove(name).is_none() {
            continue;
        }
        let out = serde_json::to_string_pretty(&config)?;
        std::fs::write(&path, out + "\n").with_context(|| format!("writing {}", path.display()))?;
        edited.push(path);
    }
    Ok(edited)
}

/// Delete a server's block under `servers:`, or `None` when it is not there.
fn without_server(text: &str, name: &str) -> Option<String> {
    let lines: Vec<&str> = text.lines().collect();
    let key = format!("{name}:");
    let servers = lines.iter().position(|l| l.trim() == "servers:")?;
    let start = lines
        .iter()
        .skip(servers + 1)
        .position(|l| l.trim() == key && l.starts_with(' '))
        .map(|i| i + servers + 1)?;
    let indent = lines[start].len() - lines[start].trim_start().len();
    let mut end = lines.len();
    for (i, line) in lines.iter().enumerate().skip(start + 1) {
        if line.trim().is_empty() {
            continue;
        }
        if line.len() - line.trim_start().len() <= indent {
            end = i;
            break;
        }
    }
    let kept: Vec<&str> = lines[..start]
        .iter()
        .chain(lines[end..].iter())
        .copied()
        .collect();
    let mut out = kept.join("\n");
    if text.ends_with('\n') && !out.is_empty() {
        out.push('\n');
    }
    Some(out)
}

fn yaml_config_paths(repo_root: Option<&std::path::Path>) -> Vec<std::path::PathBuf> {
    let mut out: Vec<std::path::PathBuf> = repo_root
        .map(|root| {
            ["aster.yaml", "aster.yml", ".aster.yaml"]
                .iter()
                .map(|f| root.join(f))
                .collect()
        })
        .unwrap_or_default();
    if let Some(home) = dirs::home_dir() {
        out.push(home.join(".aster/aster.yaml"));
    }
    out
}

fn mcp_json_paths(repo_root: Option<&std::path::Path>) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    if let Some(root) = repo_root {
        out.push(root.join(".mcp.json"));
    }
    if let Some(home) = dirs::home_dir() {
        out.push(home.join(".aster/mcp.json"));
    }
    out
}

/// Stdio servers from a cross-tool `.mcp.json` (`mcpServers` key). Url-only
/// entries are skipped: they need a transport aster does not have.
pub fn mcp_json_servers(path: &std::path::Path) -> Result<Vec<(String, ServerConfig)>> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Ok(Vec::new());
    };
    let config: Value =
        serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
    let Some(map) = config.get("mcpServers").and_then(|v| v.as_object()) else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for (name, v) in map {
        let Some(command) = v.get("command").and_then(|c| c.as_str()) else {
            continue;
        };
        let args = v
            .get("args")
            .and_then(|a| a.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        let env = v
            .get("env")
            .and_then(|e| e.as_object())
            .map(|m| {
                m.iter()
                    .filter_map(|(k, x)| x.as_str().map(|s| (k.clone(), s.to_string())))
                    .collect()
            })
            .unwrap_or_default();
        let disabled = v.get("disabled").and_then(|d| d.as_bool()).unwrap_or(false);
        out.push((
            name.clone(),
            ServerConfig {
                command: command.to_string(),
                args,
                env,
                disabled,
            },
        ));
    }
    Ok(out)
}

/// Toggle `disabled` in the first `.mcp.json` declaring `name`, returning the
/// file edited, or `None` when no file declares it.
fn mcp_json_set_disabled(
    repo_root: Option<&std::path::Path>,
    name: &str,
    disabled: bool,
) -> Result<Option<std::path::PathBuf>> {
    let mut candidates = Vec::new();
    if let Some(root) = repo_root {
        candidates.push(root.join(".mcp.json"));
    }
    if let Some(home) = dirs::home_dir() {
        candidates.push(home.join(".aster/mcp.json"));
    }
    for path in candidates {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let mut config: Value =
            serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        let Some(server) = config
            .get_mut("mcpServers")
            .and_then(|v| v.get_mut(name))
            .and_then(|v| v.as_object_mut())
        else {
            continue;
        };
        server.insert("disabled".into(), Value::Bool(disabled));
        let out = serde_json::to_string_pretty(&config)?;
        std::fs::write(&path, out + "\n").with_context(|| format!("writing {}", path.display()))?;
        return Ok(Some(path));
    }
    Ok(None)
}

/// The config file whose `mcp.servers` declares `name`: the project's first,
/// since that is the one overriding the global.
fn config_declaring(repo_root: Option<&std::path::Path>, name: &str) -> Option<std::path::PathBuf> {
    let mut candidates: Vec<std::path::PathBuf> = repo_root
        .map(|root| {
            ["aster.yaml", "aster.yml", ".aster.yaml"]
                .iter()
                .map(|f| root.join(f))
                .collect()
        })
        .unwrap_or_default();
    if let Some(home) = dirs::home_dir() {
        candidates.push(home.join(".aster/aster.yaml"));
    }
    candidates.into_iter().find(|path| {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_yaml::from_str::<crate::settings::Settings>(&text).ok())
            .is_some_and(|s| s.mcp.servers.contains_key(name))
    })
}

/// Rewrite the `disabled:` line under `name`, inserting one when the block has
/// none. Returns `None` when the server key is not in the text.
fn toggled(text: &str, name: &str, disabled: bool) -> Option<String> {
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    let key = format!("{name}:");
    // Scoped to the servers map, so a key of the same name elsewhere in the
    // file is not what gets rewritten.
    let servers = lines.iter().position(|l| l.trim() == "servers:")?;
    let start = lines
        .iter()
        .skip(servers + 1)
        .position(|l| l.trim() == key && l.starts_with(' '))
        .map(|i| i + servers + 1)?;
    let indent = lines[start].len() - lines[start].trim_start().len();

    let body = start + 1..lines.len();
    let mut insert_at = lines.len();
    for i in body {
        let line = &lines[i];
        if line.trim().is_empty() {
            continue;
        }
        let this_indent = line.len() - line.trim_start().len();
        if this_indent <= indent {
            insert_at = i;
            break;
        }
        if line.trim_start().starts_with("disabled:") {
            lines[i] = format!("{}disabled: {disabled}", " ".repeat(this_indent));
            return Some(join(&lines, text));
        }
    }
    lines.insert(
        insert_at.min(lines.len()),
        format!("{}disabled: {disabled}", " ".repeat(indent + 2)),
    );
    Some(join(&lines, text))
}

fn join(lines: &[String], original: &str) -> String {
    let mut out = lines.join("\n");
    if original.ends_with('\n') {
        out.push('\n');
    }
    out
}

/// Short description for well-known servers so the agent can decide when to ask.
fn describe_disabled_server(name: &str) -> String {
    match name {
        "chrome" => {
            "Browse the web, take screenshots, and inspect pages with Chrome DevTools.".into()
        }
        "playwright" => "Headless browser for page screenshots and JavaScript-heavy sites.".into(),
        "github" => "Create issues and PRs, search repos, read code on GitHub.".into(),
        other => format!("MCP server `{other}`."),
    }
}

#[cfg(test)]
#[path = "tests/mcp_test.rs"]
mod tests;
