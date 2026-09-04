//! Review config (`aster.yaml`), loaded from the repo root or `~/.aster/`.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use globset::{Glob, GlobSet, GlobSetBuilder};
use serde::Deserialize;

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub review: Review,
    pub permissions: aster_policy::PermissionsConfig,
    pub mcp: crate::mcp::McpSettings,
    pub agent: Agent,
    pub agents: Agents,
    pub ui: Ui,
    pub schedules: Vec<aster_cron::Schedule>,
}

/// Terminal presentation choices.
#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Ui {
    /// Whether chat prints the session header (model, provider, skills).
    pub welcome: Option<bool>,
}

/// Limits on one agent turn. Both are also settable per run via
/// `ASTER_MAX_TOOL_ROUNDS` and `ASTER_COMMAND_TIMEOUT`.
#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Agent {
    /// Tool call rounds before the agent is forced to answer with what it has.
    pub max_tool_rounds: Option<usize>,
    /// Seconds a `run_command` call may run before it is killed.
    pub command_timeout_secs: Option<u64>,
    /// History size (chars) above which older turns are compacted into a
    /// summary. Lower it for small-context models.
    pub compact_budget_chars: Option<usize>,
}

/// Swarm configuration for sub-agent fan-out.  Also settable per run via
/// `ASTER_COLLECTOR_MODEL`, `ASTER_AGENT_MAX_CONCURRENT`,
/// `ASTER_AGENT_MAX_PER_TURN`, and `ASTER_AGENT_TIMEOUT`.
#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Agents {
    /// Cheap model for collector agents.  Falls back to the session model
    /// when unset.
    pub collector_model: Option<String>,
    /// Max sub-agents running concurrently (default 8).
    pub max_concurrent: Option<usize>,
    /// Max `agent` tool tasks per turn (default 24).
    pub max_per_turn: Option<usize>,
    /// Seconds a single sub-agent may run (default 300).
    pub agent_timeout_secs: Option<u64>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Review {
    /// Fallback model when a stage override is unset.
    pub model: Option<String>,
    /// OpenAI-compatible endpoint.
    pub base_url: Option<String>,
    pub hypothesis_model: Option<String>,
    pub verify_model: Option<String>,
    /// Drop findings below this confidence (0.0-1.0).
    pub min_confidence: Option<f32>,
    pub max_diff_bytes: Option<usize>,
    /// Static analyzer backends, e.g. ["semgrep"].
    pub analyzers: Vec<String>,
    /// Repo-relative path to an ast-grep rule YAML for the `ast-grep` backend.
    pub astgrep_rules: Option<String>,
    /// Defect classes to bias the hypothesis pass toward.
    pub focus_areas: Vec<String>,
    /// Reasoning budget for thinking models: off, low, medium, high, xhigh,
    /// max, or ultra.
    pub effort: Option<aster_ai::Effort>,
    /// Enable OpenRouter web search: the agent gets a server tool, review
    /// stages get the `web` plugin. No effect on non-OpenRouter endpoints.
    pub web_search: Option<bool>,
    /// Globs of files to review. Empty = everything (minus `exclude`).
    pub include: Vec<String>,
    /// Globs of files to never review.
    pub exclude: Vec<String>,
}

impl Settings {
    /// The global config, then the repo's, with the repo's on top. Malformed
    /// files error rather than being skipped, so a typo is never silent.
    pub fn load(repo_root: Option<&Path>) -> Result<Self> {
        let global = match dirs::home_dir().map(|h| h.join(".aster/aster.yaml")) {
            Some(path) if path.exists() => Some(parse(&path)?),
            _ => None,
        };
        let project = project_config(repo_root)
            .map(|path| parse(&path))
            .transpose()?;

        let mut settings = match (global, project) {
            (Some(global), Some(project)) => global.overlaid_with(project),
            (Some(only), None) | (None, Some(only)) => only,
            (None, None) => Self::default(),
        };
        // The cross-tool `.mcp.json` files are read natively, aster.yaml
        // winning name collisions and the repo's file beating the global one.
        let mut json_paths = Vec::new();
        if let Some(root) = repo_root {
            json_paths.push(root.join(".mcp.json"));
        }
        if let Some(home) = dirs::home_dir() {
            json_paths.push(home.join(".aster/mcp.json"));
        }
        for path in json_paths {
            for (name, server) in crate::mcp::mcp_json_servers(&path)? {
                settings.mcp.servers.entry(name).or_insert(server);
            }
        }
        // Installed plugins contribute their own servers, namespaced by plugin
        // so two packages can ship a server of the same name.
        let (plugins, problems) = crate::plugins::installed(repo_root);
        crate::plugins::report(&plugins, &problems);
        for (name, server) in crate::plugins::mcp_servers(&plugins) {
            settings.mcp.servers.entry(name).or_insert(server);
        }
        Ok(settings)
    }

    /// Layer `project` over `self`. Scalars are the project's; permission lists union,
    /// since both files are grants. `mode` takes the stricter of the two, so a project
    /// file cannot loosen a global `ask` just by omitting the key.
    fn overlaid_with(self, project: Settings) -> Settings {
        Settings {
            review: self.review.overlaid_with(project.review),
            permissions: merge_permissions(self.permissions, project.permissions),
            mcp: merge_mcp(self.mcp, project.mcp),
            agent: Agent {
                max_tool_rounds: project.agent.max_tool_rounds.or(self.agent.max_tool_rounds),
                command_timeout_secs: project
                    .agent
                    .command_timeout_secs
                    .or(self.agent.command_timeout_secs),
                compact_budget_chars: project
                    .agent
                    .compact_budget_chars
                    .or(self.agent.compact_budget_chars),
            },
            agents: Agents {
                collector_model: project
                    .agents
                    .collector_model
                    .or(self.agents.collector_model),
                max_concurrent: project.agents.max_concurrent.or(self.agents.max_concurrent),
                max_per_turn: project.agents.max_per_turn.or(self.agents.max_per_turn),
                agent_timeout_secs: project
                    .agents
                    .agent_timeout_secs
                    .or(self.agents.agent_timeout_secs),
            },
            ui: Ui {
                welcome: project.ui.welcome.or(self.ui.welcome),
            },
            // Schedules merge by name, the project's definition winning, so a
            // repo can override a global cadence without dropping the rest.
            schedules: {
                let mut merged = self.schedules;
                for s in project.schedules {
                    if let Some(existing) = merged.iter_mut().find(|e| e.name == s.name) {
                        *existing = s;
                    } else {
                        merged.push(s);
                    }
                }
                merged
            },
        }
    }
}

/// Servers union by name, the project's definition winning, so a repo can point a
/// shared name at its own binary. The tool filter unions like permissions do: a
/// global `deny` is a decision a project file must not undo by omission.
fn merge_mcp(
    mut global: crate::mcp::McpSettings,
    project: crate::mcp::McpSettings,
) -> crate::mcp::McpSettings {
    global.servers.extend(project.servers);
    global.tools = crate::mcp::ToolFilter {
        allow: union(global.tools.allow, project.tools.allow),
        deny: union(global.tools.deny, project.tools.deny),
    };
    global
}

fn union(mut a: Vec<String>, b: Vec<String>) -> Vec<String> {
    for item in b {
        if !a.contains(&item) {
            a.push(item);
        }
    }
    a
}

fn merge_permissions(
    global: aster_policy::PermissionsConfig,
    project: aster_policy::PermissionsConfig,
) -> aster_policy::PermissionsConfig {
    aster_policy::PermissionsConfig {
        mode: global.mode.stricter(project.mode),
        allow: union(global.allow, project.allow),
        ask: union(global.ask, project.ask),
        deny: union(global.deny, project.deny),
        additional_directories: union(
            global.additional_directories,
            project.additional_directories,
        ),
        allow_credentials: union(global.allow_credentials, project.allow_credentials),
        use_default_rules: global.use_default_rules && project.use_default_rules,
    }
}

impl Review {
    fn overlaid_with(self, project: Review) -> Review {
        Review {
            model: project.model.or(self.model),
            base_url: project.base_url.or(self.base_url),
            hypothesis_model: project.hypothesis_model.or(self.hypothesis_model),
            verify_model: project.verify_model.or(self.verify_model),
            min_confidence: project.min_confidence.or(self.min_confidence),
            max_diff_bytes: project.max_diff_bytes.or(self.max_diff_bytes),
            analyzers: pick(self.analyzers, project.analyzers),
            astgrep_rules: project.astgrep_rules.or(self.astgrep_rules),
            effort: project.effort.or(self.effort),
            web_search: project.web_search.or(self.web_search),
            focus_areas: pick(self.focus_areas, project.focus_areas),
            include: pick(self.include, project.include),
            exclude: pick(self.exclude, project.exclude),
        }
    }
}

/// Review lists replace rather than union: an `include` of `["src/**"]` in a
/// project means that and nothing else.
fn pick(global: Vec<String>, project: Vec<String>) -> Vec<String> {
    if project.is_empty() { global } else { project }
}

fn parse(path: &Path) -> Result<Settings> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    serde_yaml::from_str(&text).with_context(|| format!("parsing {}", path.display()))
}

/// The config every directory reads; the caller creates it on first write.
pub(crate) fn user_config() -> Result<PathBuf> {
    Ok(dirs::home_dir()
        .context("no home directory for the global config")?
        .join(".aster/aster.yaml"))
}

/// The repo's own config, under whichever of the three names it uses.
pub(crate) fn project_config(repo_root: Option<&Path>) -> Option<PathBuf> {
    repo_root.and_then(|root| {
        ["aster.yaml", "aster.yml", ".aster.yaml"]
            .iter()
            .map(|name| root.join(name))
            .find(|path| path.exists())
    })
}

/// Where a setting the next start must read belongs: the repo's config when
/// one exists, else the global one, which the caller creates.
pub(crate) fn writable_config(repo_root: Option<&Path>) -> Result<PathBuf> {
    match project_config(repo_root) {
        Some(path) => Ok(path),
        None => user_config(),
    }
}

pub struct Saved {
    /// The global config, where the choice lives.
    pub path: PathBuf,
    /// A project config that pinned the same keys and was moved along with it.
    pub also: Option<PathBuf>,
}

/// Save a choice that belongs to the user rather than to one repo: the model and
/// endpoint follow you between directories, so they go in the global config. A
/// project file pinning the same key is moved along with it, since it outranks.
pub fn persist_user_review(repo_root: Option<&Path>, pairs: &[(&str, &str)]) -> Result<Saved> {
    let path = user_config()?;
    write_review(&path, pairs)?;

    let Some(project) = project_config(repo_root).filter(|p| *p != path) else {
        return Ok(Saved { path, also: None });
    };
    let text = std::fs::read_to_string(&project).unwrap_or_default();
    let pinned: Vec<(&str, &str)> = pairs
        .iter()
        .copied()
        .filter(|(key, _)| pins(&text, "review", key))
        .collect();
    if pinned.is_empty() {
        return Ok(Saved { path, also: None });
    }
    write_review(&project, &pinned)?;
    Ok(Saved {
        path,
        also: Some(project),
    })
}

/// Write `review.<key>` pairs into one file, editing it line by line so
/// comments and layout survive.
pub(crate) fn write_review(path: &Path, pairs: &[(&str, &str)]) -> Result<()> {
    let mut text = std::fs::read_to_string(path).unwrap_or_default();
    for (key, value) in pairs {
        text = with_key(&text, "review", key, value);
    }
    save(path, text)
}

pub(crate) fn save(path: &Path, text: String) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    std::fs::write(path, text).with_context(|| format!("writing {}", path.display()))
}

/// Where `<section>.<key>` sits in a config's lines.
enum Slot {
    /// Already set, on this line at this indent.
    At(usize, usize),
    /// The block exists without the key; it goes in at this line and indent.
    Insert(usize, usize),
    /// There is no `<section>:` block yet.
    Missing,
}

/// True when a config sets `<section>.<key>` itself, and so would outrank the
/// global one no matter what is written there.
pub(crate) fn pins(text: &str, section: &str, key: &str) -> bool {
    matches!(slot(&split(text), section, key), Slot::At(..))
}

fn split(text: &str) -> Vec<String> {
    text.lines().map(str::to_string).collect()
}

fn indent_of(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

fn slot(lines: &[String], section: &str, key: &str) -> Slot {
    let header = format!("{section}:");
    let Some(start) = lines
        .iter()
        .position(|l| l.trim_end() == header && !l.starts_with([' ', '\t']))
    else {
        return Slot::Missing;
    };
    let mut level = None;
    for (i, line) in lines.iter().enumerate().skip(start + 1) {
        if line.trim().is_empty() {
            continue;
        }
        let indent = indent_of(line);
        if indent == 0 {
            return Slot::Insert(i, level.unwrap_or(2));
        }
        // Depth comes from the first child rather than being assumed, so a
        // four-space file keeps its shape.
        let level = *level.get_or_insert(indent);
        if indent == level && line.trim_start().starts_with(&format!("{key}:")) {
            return Slot::At(i, indent);
        }
    }
    Slot::Insert(lines.len(), level.unwrap_or(2))
}

/// One past the last line of the value at `start`: a nested list or block
/// scalar runs on past the key's own line. Trailing blanks stay put.
fn value_end(lines: &[String], start: usize) -> usize {
    let indent = indent_of(&lines[start]);
    let mut end = start + 1;
    for (i, line) in lines.iter().enumerate().skip(start + 1) {
        if line.trim().is_empty() {
            continue;
        }
        let this = indent_of(line);
        // A list may be written at the key's own indent, so `- ` items belong
        // to it as much as anything indented deeper does.
        if this < indent || (this == indent && !line.trim_start().starts_with("- ")) {
            break;
        }
        end = i + 1;
    }
    end
}

/// Rewrite one `<section>.<key>` in place, adding the key or the whole block
/// when missing. Everything else in the file is left byte for byte.
pub(crate) fn with_key(text: &str, section: &str, key: &str, value: &str) -> String {
    let mut lines = split(text);
    match slot(&lines, section, key) {
        Slot::Missing => {
            let mut out = text.to_string();
            if !out.is_empty() && !out.ends_with('\n') {
                out.push('\n');
            }
            out.push_str(&format!("{section}:\n  {key}: {value}\n"));
            out
        }
        Slot::At(i, indent) => {
            let end = value_end(&lines, i);
            let line = format!("{}{key}: {value}", " ".repeat(indent));
            lines.splice(i..end, [line]);
            rejoin(&lines, text)
        }
        Slot::Insert(at, indent) => {
            lines.insert(at, format!("{}{key}: {value}", " ".repeat(indent)));
            rejoin(&lines, text)
        }
    }
}

/// Take `<section>.<key>` back out, and the now-empty section header with it,
/// since a header with no keys under it parses as null rather than as absent.
/// `None` when the file did not set the key.
pub(crate) fn without_key(text: &str, section: &str, key: &str) -> Option<String> {
    let mut lines = split(text);
    let Slot::At(i, _) = slot(&lines, section, key) else {
        return None;
    };
    lines.drain(i..value_end(&lines, i));

    let header = format!("{section}:");
    let at = lines
        .iter()
        .position(|l| l.trim_end() == header && !l.starts_with([' ', '\t']));
    if let Some(at) = at
        && lines
            .iter()
            .skip(at + 1)
            .find(|l| !l.trim().is_empty())
            .is_none_or(|l| indent_of(l) == 0)
    {
        lines.remove(at);
    }
    Some(rejoin(&lines, text))
}

fn rejoin(lines: &[String], original: &str) -> String {
    let mut out = lines.join("\n");
    if original.ends_with('\n') || original.is_empty() {
        out.push('\n');
    }
    out
}

pub struct PathFilter {
    include: Option<GlobSet>,
    exclude: GlobSet,
}

/// Generated/vendored files always excluded, on top of the user's `exclude`.
const DEFAULT_EXCLUDE: &[&str] = &[
    "**/*.lock",
    "**/package-lock.json",
    "**/pnpm-lock.yaml",
    "**/yarn.lock",
    "**/composer.lock",
    "**/Gemfile.lock",
    "**/Cargo.lock",
    "**/Pipfile.lock",
    "**/poetry.lock",
    "**/requirements.txt",
    "**/*.min.js",
    "**/*.min.css",
    "**/*.min.css.map",
    "**/*.min.js.map",
    "**/*.map",
    "**/*.snap",
    "**/dist/**",
    "**/build/**",
    "**/out/**",
    "**/node_modules/**",
    "**/vendor/**",
    "**/.git/**",
    "**/.hg/**",
    "**/.svn/**",
    "**/.DS_Store",
    "**/Thumbs.db",
    "**/*.class",
    "**/target/**",
    "**/*.pyc",
];

impl PathFilter {
    /// Empty `include` means everything. `exclude` is unioned with [`DEFAULT_EXCLUDE`].
    pub fn new(include: &[String], exclude: &[String]) -> Result<Self> {
        let mut excludes: Vec<String> = DEFAULT_EXCLUDE.iter().map(|s| s.to_string()).collect();
        excludes.extend(exclude.iter().cloned());
        Ok(Self {
            include: if include.is_empty() {
                None
            } else {
                Some(glob_set(include)?)
            },
            exclude: glob_set(&excludes)?,
        })
    }

    /// True when a path matches `include` (or include is empty) and no `exclude`.
    pub fn allows(&self, path: &str) -> bool {
        if self.exclude.is_match(path) {
            return false;
        }
        match &self.include {
            Some(set) => set.is_match(path),
            None => true,
        }
    }
}

pub(crate) fn glob_set(patterns: &[String]) -> Result<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    for p in patterns {
        builder.add(Glob::new(p).with_context(|| format!("invalid glob: {p}"))?);
    }
    builder.build().context("building glob set")
}

/// Keep only the per-file sections of a unified diff whose target path passes `filter`.
pub fn filter_diff(diff: &str, filter: &PathFilter) -> String {
    let mut out = String::new();
    let mut keep = true;
    for line in diff.lines() {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            keep = rest
                .split_whitespace()
                .nth(1)
                .and_then(|b| b.strip_prefix("b/"))
                .map(|path| filter.allows(path))
                .unwrap_or(true);
        }
        if keep {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

#[cfg(test)]
#[path = "tests/settings_test.rs"]
mod tests;
