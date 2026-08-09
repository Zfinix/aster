//! The `mcp.json` configuration (§7.2). A malformed document disables MCP for
//! the plugin; a malformed entry skips only that server.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use serde_json::{Map, Value};

use crate::path::{Anchor, PLUGIN_DATA_VAR, PLUGIN_ROOT_VAR, contained, cwd_anchor, expand};

/// The one MCP configuration schema this client implements.
pub const MCP_SCHEMA: &str = "https://agent-plugins.org/schemas/1.0.0/mcp.schema.json";

#[derive(Debug, Clone)]
pub struct Server {
    pub name: String,
    pub transport: Transport,
}

#[derive(Debug, Clone)]
pub enum Transport {
    Stdio(Stdio),
    /// Remote transports, kept so a plugin's servers can be reported even where
    /// this client cannot connect to them.
    Http(Http),
}

/// A stdio server with every path resolved and every placeholder expanded, ready
/// to spawn. `env` already carries the reserved variables.
#[derive(Debug, Clone)]
pub struct Stdio {
    pub command: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub cwd: PathBuf,
}

#[derive(Debug, Clone)]
pub struct Http {
    /// False for the deprecated HTTP+SSE transport.
    pub streamable: bool,
    pub url: String,
    pub headers: BTreeMap<String, String>,
}

impl Server {
    pub fn transport_name(&self) -> &'static str {
        match &self.transport {
            Transport::Stdio(_) => "stdio",
            Transport::Http(http) if http.streamable => "streamable-http",
            Transport::Http(_) => "sse",
        }
    }
}

/// Parse `mcp.json`. Entries that fail validation are skipped with a warning;
/// a document-level failure returns an error and disables MCP for the plugin.
pub fn parse(
    text: &str,
    root: &Path,
    data_dir: &Path,
    warnings: &mut Vec<String>,
) -> Result<Vec<Server>> {
    let value: Value = serde_json::from_str(text)?;
    let Value::Object(object) = value else {
        bail!("mcp.json must contain a top-level object");
    };
    for key in object.keys() {
        if key != "$schema" && key != "mcpServers" {
            bail!("`{key}` is not a permitted mcp.json field");
        }
    }
    match object.get("$schema").and_then(Value::as_str) {
        Some(MCP_SCHEMA) => {}
        Some(other) => bail!("unsupported Agent Plugins version: $schema is {other:?}"),
        None => bail!("`$schema` is required and must be {MCP_SCHEMA:?}"),
    }
    let Some(entries) = object.get("mcpServers").and_then(Value::as_object) else {
        bail!("`mcpServers` is required and must be an object");
    };

    let mut servers = Vec::new();
    for (name, entry) in entries {
        match server(entry, root, data_dir) {
            Ok(transport) => servers.push(Server {
                name: name.clone(),
                transport,
            }),
            Err(e) => warnings.push(format!("skipping MCP server `{name}`: {e:#}")),
        }
    }
    Ok(servers)
}

fn server(entry: &Value, root: &Path, data_dir: &Path) -> Result<Transport> {
    let Some(fields) = entry.as_object() else {
        bail!("a server entry must be an object");
    };
    match fields.get("type").and_then(Value::as_str) {
        Some("stdio") => stdio(fields, root, data_dir).map(Transport::Stdio),
        Some(kind @ ("streamable-http" | "sse")) => {
            http(fields, kind == "streamable-http").map(Transport::Http)
        }
        Some(other) => bail!("unknown transport type {other:?}"),
        None => bail!("`type` is required"),
    }
}

fn stdio(fields: &Map<String, Value>, root: &Path, data_dir: &Path) -> Result<Stdio> {
    closed(fields, &["type", "command", "args", "env", "cwd"])?;
    let root_text = text_path(root)?;
    let data_text = text_path(data_dir)?;

    let command = match fields.get("command") {
        Some(Value::String(command)) if !command.is_empty() => command.clone(),
        _ => bail!("`command` is required and must be a non-empty string"),
    };
    // Never expanded: a `./` path resolves against the root, a bare name goes to
    // the platform's executable search.
    let command = match command.starts_with("./") {
        true => text_path(&crate::path::plugin_relative(root, &command)?)?,
        false if command.contains('/') || command.contains('\\') => {
            bail!("`command` must be a bare executable name or a `./` plugin-relative path")
        }
        false => command,
    };

    let args = match fields.get("args") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| match item.as_str() {
                Some(arg) => Ok(expand(arg, &root_text, &data_text)),
                None => bail!("`args` must be an array of strings"),
            })
            .collect::<Result<Vec<_>>>()?,
        Some(_) => bail!("`args` must be an array of strings"),
    };

    let mut env = BTreeMap::new();
    match fields.get("env") {
        None | Some(Value::Null) => {}
        Some(Value::Object(pairs)) => {
            for (key, value) in pairs {
                if key == PLUGIN_ROOT_VAR || key == PLUGIN_DATA_VAR {
                    bail!("`env` must not set {key}; the client supplies it");
                }
                let Some(value) = value.as_str() else {
                    bail!("`env` values must be strings");
                };
                env.insert(key.clone(), expand(value, &root_text, &data_text));
            }
        }
        Some(_) => bail!("`env` must be an object of strings"),
    }
    // Set last so they replace any same-named entry the plugin configured (§9.1).
    env.insert(PLUGIN_ROOT_VAR.to_string(), root_text.clone());
    env.insert(PLUGIN_DATA_VAR.to_string(), data_text.clone());

    let cwd = match fields.get("cwd") {
        None | Some(Value::Null) => root.to_path_buf(),
        Some(Value::String(value)) => {
            let anchor = cwd_anchor(value)?;
            let expanded = PathBuf::from(expand(value, &root_text, &data_text));
            let anchor_dir = match anchor {
                Anchor::Root => root,
                Anchor::Data => data_dir,
            };
            if !contained(anchor_dir, &expanded) {
                bail!("`cwd` resolves outside {}", anchor_dir.display());
            }
            expanded
        }
        Some(_) => bail!("`cwd` must be a string"),
    };

    Ok(Stdio {
        command,
        args,
        env,
        cwd,
    })
}

fn http(fields: &Map<String, Value>, streamable: bool) -> Result<Http> {
    closed(fields, &["type", "url", "headers"])?;
    let url = match fields.get("url") {
        Some(Value::String(url)) if !url.is_empty() => url.clone(),
        _ => bail!("`url` is required and must be a non-empty string"),
    };
    validate_url(&url)?;

    let mut headers = BTreeMap::new();
    let mut seen = BTreeSet::new();
    match fields.get("headers") {
        None | Some(Value::Null) => {}
        Some(Value::Object(pairs)) => {
            for (name, value) in pairs {
                validate_header(name, value)?;
                if !seen.insert(name.to_ascii_lowercase()) {
                    bail!("`headers` repeats {name:?} under different casing");
                }
                let value = value.as_str().unwrap_or_default();
                headers.insert(name.clone(), value.to_string());
            }
        }
        Some(_) => bail!("`headers` must be an object of strings"),
    }

    Ok(Http {
        streamable,
        url,
        headers,
    })
}

fn closed(fields: &Map<String, Value>, allowed: &[&str]) -> Result<()> {
    for key in fields.keys() {
        if !allowed.contains(&key.as_str()) {
            bail!("`{key}` is not permitted on this transport");
        }
    }
    Ok(())
}

/// Absolute http(s), no user information, no fragment, and plaintext only for a
/// loopback host.
fn validate_url(url: &str) -> Result<()> {
    let Some((scheme, rest)) = url.split_once("://") else {
        bail!("`url` must be an absolute http or https URL");
    };
    let scheme = scheme.to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        bail!("`url` must use http or https");
    }
    if url.contains('#') {
        bail!("`url` must not contain a fragment");
    }
    let authority = rest.split(['/', '?']).next().unwrap_or_default();
    if authority.is_empty() {
        bail!("`url` has no host");
    }
    if authority.contains('@') {
        bail!("`url` must not contain user information");
    }
    if scheme == "http" && !is_loopback(host_of(authority)) {
        bail!("`url` must use https for a non-loopback host");
    }
    Ok(())
}

fn host_of(authority: &str) -> &str {
    if let Some(end) = authority.strip_prefix('[').and_then(|r| r.find(']')) {
        return &authority[1..=end];
    }
    authority.split(':').next().unwrap_or(authority)
}

fn is_loopback(host: &str) -> bool {
    if host.eq_ignore_ascii_case("localhost") || host == "::1" {
        return true;
    }
    let mut octets = host.split('.');
    let first = octets.next().and_then(|o| o.parse::<u8>().ok());
    let rest: Vec<&str> = octets.collect();
    first == Some(127) && rest.len() == 3 && rest.iter().all(|o| o.parse::<u8>().is_ok())
}

fn validate_header(name: &str, value: &Value) -> Result<()> {
    const SYMBOLS: &str = "!#$%&'*+-.^_`|~";
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || SYMBOLS.contains(c))
    {
        bail!("{name:?} is not a valid HTTP header name");
    }
    let Some(value) = value.as_str() else {
        bail!("`headers` values must be strings");
    };
    if !value.chars().all(|c| c == '\t' || (' '..='~').contains(&c)) {
        bail!("the {name:?} header value is not a valid HTTP field value");
    }
    Ok(())
}

fn text_path(path: &Path) -> Result<String> {
    match path.to_str() {
        Some(text) => Ok(text.to_string()),
        None => bail!("{} is not valid UTF-8", path.display()),
    }
}
