#![forbid(unsafe_code)]
//! Progressive disclosure for Model Context Protocol (MCP) tools.
//! [`Injector`] emits one bridge and a context-bounded inventory.
//! Hosts own transport, authorization, approval, and auditing.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct McpTool {
    /// Stable server identifier configured by the host.
    pub server: String,
    pub name: String,
    /// Short, task-oriented description used for discovery.
    pub description: String,
    /// The unmodified MCP input schema. It is exposed only by `describe`.
    pub input_schema: Value,
}

impl McpTool {
    /// `server/name`, the identifier accepted by the bridge.
    pub fn id(&self) -> String {
        format!("{}/{}", self.server, self.name)
    }

    fn validate(&self) -> Result<()> {
        if self.server.trim().is_empty() {
            bail!("an MCP tool has an empty server name");
        }
        if self.name.trim().is_empty() {
            bail!("an MCP tool on {} has an empty tool name", self.server);
        }
        if self.description.trim().is_empty() {
            bail!("MCP tool {} has an empty description", self.id());
        }
        if !self.input_schema.is_object() {
            bail!("MCP tool {} has a non-object input schema", self.id());
        }
        Ok(())
    }
}

/// A compact description of an MCP server, generated from its permitted tools.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct McpServer {
    pub name: String,
    pub description: String,
    pub tool_count: usize,
}

/// The tools a session is allowed to discover or invoke.
/// The catalogue validates unique `server/name` identifiers.
#[derive(Debug, Clone)]
pub struct McpCatalog {
    tools: Vec<McpTool>,
    servers: Vec<McpServer>,
}

impl McpCatalog {
    pub fn new(tools: Vec<McpTool>) -> Result<Self> {
        let mut ids = BTreeSet::new();
        let mut servers: BTreeMap<String, usize> = BTreeMap::new();
        for tool in &tools {
            tool.validate()?;
            if !ids.insert(tool.id()) {
                bail!("duplicate MCP tool id {}", tool.id());
            }
            *servers.entry(tool.server.clone()).or_default() += 1;
        }
        let servers = servers
            .into_iter()
            .map(|(name, tool_count)| McpServer {
                description: format!("{} MCP tools", tool_count),
                name,
                tool_count,
            })
            .collect();
        Ok(Self { tools, servers })
    }

    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    pub fn len(&self) -> usize {
        self.tools.len()
    }

    pub fn tools(&self) -> &[McpTool] {
        &self.tools
    }

    pub fn servers(&self) -> &[McpServer] {
        &self.servers
    }

    pub fn get(&self, id: &str) -> Option<&McpTool> {
        self.tools.iter().find(|tool| tool.id() == id)
    }

    /// Returns lexical matches ranked by task words in the name, description,
    /// and JSON-schema parameter names/descriptions. This has no embedding or
    /// network dependency, so every result is reproducible and auditable.
    pub fn search(&self, query: &str, limit: usize) -> Vec<ToolMatch> {
        let terms = terms(query);
        if terms.is_empty() || limit == 0 {
            return Vec::new();
        }

        let mut matches: Vec<ToolMatch> = self
            .tools
            .iter()
            .filter_map(|tool| {
                let score = score(tool, &terms);
                (score > 0).then(|| ToolMatch {
                    id: tool.id(),
                    server: tool.server.clone(),
                    name: tool.name.clone(),
                    description: tool.description.clone(),
                    score,
                })
            })
            .collect();
        matches.sort_by(|left, right| {
            right
                .score
                .cmp(&left.score)
                .then_with(|| left.id.cmp(&right.id))
        });
        matches.truncate(limit);
        matches
    }
}

/// Context-sensitive settings for progressive tool injection.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct ProgressiveConfig {
    /// Tokens remaining in the active model context after system instructions,
    /// conversation history, and a host-reserved response budget.
    pub available_context_tokens: usize,
    /// Maximum percentage of available context a direct tool manifest may use.
    /// The default is 6%.
    pub inventory_threshold_percent: f32,
    /// Results returned when `search` omits `limit`.
    pub search_default_limit: usize,
    /// Upper bound accepted from a model-supplied `search` limit.
    pub max_search_limit: usize,
}

impl Default for ProgressiveConfig {
    fn default() -> Self {
        Self {
            available_context_tokens: 100_000,
            inventory_threshold_percent: 6.0,
            search_default_limit: 5,
            max_search_limit: 20,
        }
    }
}

impl ProgressiveConfig {
    fn validate(self) -> Result<()> {
        if self.available_context_tokens == 0 {
            bail!("available_context_tokens must be greater than zero");
        }
        if !(self.inventory_threshold_percent > 0.0 && self.inventory_threshold_percent <= 100.0) {
            bail!("inventory_threshold_percent must be in (0, 100]");
        }
        if self.search_default_limit == 0 || self.max_search_limit == 0 {
            bail!("search result limits must be greater than zero");
        }
        if self.search_default_limit > self.max_search_limit {
            bail!("search_default_limit cannot exceed max_search_limit");
        }
        Ok(())
    }

    fn inventory_budget_tokens(self) -> usize {
        ((self.available_context_tokens as f64)
            * (f64::from(self.inventory_threshold_percent) / 100.0))
            .floor() as usize
    }
}

/// The catalogue summary placed in the system prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Inventory {
    /// Tool names and descriptions fit the configured context budget.
    Tools(Vec<ToolManifestEntry>),
    /// The tool list would be too large; advertise only available MCP servers.
    Servers(Vec<McpServer>),
}

/// A schema-free tool entry.
/// The bridge returns its full schema only through `describe`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolManifestEntry {
    pub id: String,
    pub description: String,
}

#[derive(Debug, Clone)]
pub struct Injection {
    /// The single OpenAI-compatible `aster_mcp` function definition.
    pub bridge_tool: Value,
    pub prompt: String,
    pub inventory: Inventory,
    /// Approximate token count of the full schema-free tool manifest.
    pub manifest_tokens: usize,
    /// Configured maximum direct-manifest budget.
    pub inventory_budget_tokens: usize,
}

/// Creates progressive MCP prompt injections and routes their one bridge tool.
#[derive(Debug, Clone)]
pub struct Injector {
    catalog: McpCatalog,
    config: ProgressiveConfig,
}

impl Injector {
    pub fn new(catalog: McpCatalog, config: ProgressiveConfig) -> Result<Self> {
        config.validate()?;
        Ok(Self { catalog, config })
    }

    pub fn catalog(&self) -> &McpCatalog {
        &self.catalog
    }

    pub fn inject(&self) -> Option<Injection> {
        if self.catalog.is_empty() {
            return None;
        }
        let entries = self
            .catalog
            .tools()
            .iter()
            .map(|tool| ToolManifestEntry {
                id: tool.id(),
                description: tool.description.clone(),
            })
            .collect::<Vec<_>>();
        let manifest_tokens = estimated_tokens(&render_tools(&entries));
        let inventory_budget_tokens = self.config.inventory_budget_tokens();
        let inventory = if manifest_tokens <= inventory_budget_tokens {
            Inventory::Tools(entries)
        } else {
            Inventory::Servers(self.catalog.servers().to_vec())
        };
        let prompt = render_inventory(&inventory, self.config.max_search_limit);
        Some(Injection {
            bridge_tool: bridge_tool_definition(),
            prompt,
            inventory,
            manifest_tokens,
            inventory_budget_tokens,
        })
    }

    /// Parses and validates one `aster_mcp` request.
    /// Hosts must authorize the real `Execute` target before invoking it.
    pub fn route(&self, arguments: &str) -> Result<BridgeAction> {
        let request: BridgeRequest =
            serde_json::from_str(arguments).context("aster_mcp arguments must be valid JSON")?;
        match request {
            BridgeRequest::Search { query, limit } => {
                let query = non_empty("search query", query)?;
                let limit = limit.unwrap_or(self.config.search_default_limit);
                if limit == 0 || limit > self.config.max_search_limit {
                    bail!(
                        "search limit must be between 1 and {}",
                        self.config.max_search_limit
                    );
                }
                Ok(BridgeAction::Search(self.catalog.search(&query, limit)))
            }
            BridgeRequest::Describe { name } => {
                let name = non_empty("tool name", name)?;
                let tool = self.catalog.get(&name).cloned().with_context(|| {
                    format!("MCP tool {name:?} is not available to this session")
                })?;
                Ok(BridgeAction::Describe(tool))
            }
            BridgeRequest::Execute { name, arguments } => {
                let name = non_empty("tool name", name)?;
                let tool = self.catalog.get(&name).cloned().with_context(|| {
                    format!("MCP tool {name:?} is not available to this session")
                })?;
                if !arguments.is_object() {
                    bail!("MCP tool arguments must be a JSON object");
                }
                Ok(BridgeAction::Execute { tool, arguments })
            }
        }
    }

    /// Routes a bridge request and invokes only a resolved real tool.
    /// Discovery actions never call the invoker.
    pub fn handle(&self, arguments: &str, invoker: &mut impl McpInvoker) -> Result<Value> {
        match self.route(arguments)? {
            BridgeAction::Search(matches) => Ok(json!({ "matches": matches })),
            BridgeAction::Describe(tool) => Ok(json!({
                "id": tool.id(),
                "description": tool.description,
                "input_schema": tool.input_schema,
            })),
            BridgeAction::Execute { tool, arguments } => invoker.invoke(&tool, &arguments),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
enum BridgeRequest {
    Search {
        query: String,
        #[serde(default)]
        limit: Option<usize>,
    },
    Describe {
        name: String,
    },
    Execute {
        name: String,
        arguments: Value,
    },
}

/// A routed bridge request.
/// `Execute` retains the resolved tool to prevent target substitution.
#[derive(Debug, Clone, PartialEq)]
pub enum BridgeAction {
    Search(Vec<ToolMatch>),
    Describe(McpTool),
    Execute { tool: McpTool, arguments: Value },
}

/// A schema-free discovery result.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolMatch {
    pub id: String,
    pub server: String,
    pub name: String,
    pub description: String,
    pub score: u32,
}

/// Host-supplied execution boundary for an MCP transport.
pub trait McpInvoker {
    /// Call the already-resolved tool.
    /// Implementations must authorize `tool.id()`, not merely `aster_mcp`.
    fn invoke(&mut self, tool: &McpTool, arguments: &Value) -> Result<Value>;
}

/// The one bridge tool injected into an OpenAI-compatible tools array.
pub fn bridge_tool_definition() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "aster_mcp",
            "description": "Discover, inspect, and execute MCP tools available to this session. Use action=search with a task-focused query to find tools; action=describe before constructing arguments; action=execute only with a discovered tool id and arguments that match its described schema.",
            "parameters": {
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["search", "describe", "execute"],
                        "description": "search finds tools; describe returns one tool's full schema; execute invokes one tool."
                    },
                    "query": { "type": "string", "description": "Task-focused search query. Required for search." },
                    "limit": { "type": "integer", "minimum": 1, "description": "Maximum search results. Optional for search." },
                    "name": { "type": "string", "description": "Exact server/tool id from search or the visible manifest. Required for describe and execute." },
                    "arguments": { "type": "object", "description": "Arguments matching the described MCP tool schema. Required for execute." }
                },
                "required": ["action"]
            }
        }
    })
}

/// A deliberately conservative, dependency-free estimate used for the
/// threshold decision. Hosts with provider tokenizers may replace it by passing
/// an adjusted `available_context_tokens` budget.
pub fn estimated_tokens(text: &str) -> usize {
    text.chars().count().div_ceil(4)
}

fn render_inventory(inventory: &Inventory, max_search_limit: usize) -> String {
    let mut out = String::from(
        "## MCP tools\n\nMCP capabilities are available through the `aster_mcp` bridge. \
        Do not guess argument names: call `aster_mcp` with `action: \"describe\"` \
        before executing a tool. Search returns at most ",
    );
    out.push_str(&max_search_limit.to_string());
    out.push_str(" matches.\n");
    match inventory {
        Inventory::Tools(entries) => {
            out.push_str("\nAvailable tools:\n");
            out.push_str(&render_tools(entries));
        }
        Inventory::Servers(servers) => {
            out.push_str("\nAvailable MCP servers (search their capabilities when needed):\n");
            for server in servers {
                out.push_str(&format!("- **{}** — {}\n", server.name, server.description));
            }
        }
    }
    out
}

fn render_tools(entries: &[ToolManifestEntry]) -> String {
    entries
        .iter()
        .map(|entry| format!("- **{}** — {}\n", entry.id, entry.description))
        .collect()
}

fn non_empty(label: &str, value: String) -> Result<String> {
    if value.trim().is_empty() {
        bail!("{label} cannot be empty");
    }
    Ok(value)
}

fn terms(text: &str) -> Vec<String> {
    text.split(|ch: char| !ch.is_alphanumeric())
        .filter(|term| !term.is_empty())
        .map(str::to_lowercase)
        .collect()
}

fn score(tool: &McpTool, query: &[String]) -> u32 {
    let name = terms(&tool.name);
    let description = terms(&tool.description);
    let schema = terms(&tool.input_schema.to_string());
    query.iter().fold(0, |score, term| {
        score
            + name_score(&name, term, 16, 8)
            + name_score(&description, term, 6, 3)
            + name_score(&schema, term, 4, 2)
    })
}

fn name_score(haystack: &[String], needle: &str, exact: u32, prefix: u32) -> u32 {
    haystack.iter().fold(0, |score, token| {
        score
            + if token == needle {
                exact
            } else if token.starts_with(needle) || needle.starts_with(token) {
                prefix
            } else {
                0
            }
    })
}

#[cfg(test)]
#[path = "tests/lib_test.rs"]
mod tests;
