//! `aster config`: the one command for providers, models, keys, and every
//! setting. Reads are bare, writes are positional (`aster config model x`),
//! and every read reports where the value came from, since the shell outranks
//! both files.

pub mod key;
pub mod models;
pub mod provider;

use std::path::{Path, PathBuf};
use std::process;
use std::{env, fs};

use anyhow::{Context, Result, bail};
use aster_ai::keys::env_non_empty;
use clap::{Args, Subcommand};
use cliclack::{log, outro, password, select, set_theme};
use serde_json::{Value, json};

use crate::settings::Settings;
use crate::term::{BOLD, DIM, paint};
use crate::util::or_cancel;

#[derive(Args)]
pub struct ConfigArgs {
    #[command(subcommand)]
    command: Option<ConfigCmd>,
}

#[derive(Subcommand)]
enum ConfigCmd {
    /// Every setting, what the next turn resolves it to, and where it came from.
    List,
    /// Print one setting's resolved value, and nothing else.
    Get(GetArgs),
    /// Write one setting, checking the file still parses before saving it.
    Set(SetArgs),
    /// Take one setting back out, restoring its default.
    Unset(UnsetArgs),
    /// Which config files Aster reads here, and which of them exist.
    Path,
    /// Open a config file in $EDITOR, then check what you saved parses.
    Edit(Target),
    /// The endpoints Aster knows, marking the one in use.
    Providers,
    /// Which endpoint is in use, and where that came from.
    Provider(ProviderReadArgs),
    /// What the configured endpoint serves.
    Models(models::ModelsArgs),
    /// Which model is in use, and where that came from.
    Model(ModelReadArgs),
    /// Every key Aster reads, whether it is set, and which file it came from.
    Keys(models::KeysArgs),
    /// The key status for the endpoint in use.
    Key(KeyReadArgs),
}

/// Which file a write lands in. Neither flag means the repo's config when it
/// has one, else the global one.
#[derive(Args, Clone, Copy)]
struct Target {
    /// Write to `~/.aster/aster.yaml`, the config every directory reads.
    #[arg(long, conflicts_with = "local")]
    global: bool,

    /// Write to this repo's `aster.yaml`, creating it when there is none.
    #[arg(long)]
    local: bool,
}

#[derive(Args)]
struct GetArgs {
    /// Dotted key, as `aster config list` spells it, e.g. `review.model`.
    #[arg(value_name = "KEY")]
    key: String,
}

#[derive(Args)]
struct SetArgs {
    /// Dotted key, as `aster config list` spells it, e.g. `review.model`.
    #[arg(value_name = "KEY")]
    key: String,

    /// The value. List keys take a comma-separated value; an empty one empties
    /// the list.
    #[arg(value_name = "VALUE")]
    value: String,

    #[command(flatten)]
    target: Target,
}

#[derive(Args)]
struct UnsetArgs {
    /// Dotted key, as `aster config list` spells it, e.g. `review.model`.
    #[arg(value_name = "KEY")]
    key: String,

    /// Only clear the global config, leaving a project one that sets it.
    #[arg(long, conflicts_with = "local")]
    global: bool,

    /// Only clear this repo's config, leaving the global one that sets it.
    #[arg(long)]
    local: bool,
}

pub async fn run(args: ConfigArgs) -> Result<()> {
    let repo_root = env::current_dir().context("could not determine the current directory")?;
    let Some(command) = args.command else {
        if !crate::picker::is_tty() {
            return list(&repo_root);
        }
        // Nothing set up yet means the form has nothing to show; onboarding
        // is the answer, the same one `aster init` gives.
        if !crate::init::Current::read(&repo_root).configured {
            return crate::init::run_onboarding().await;
        }
        return menu(&repo_root).await;
    };
    match command {
        ConfigCmd::List => list(&repo_root),
        ConfigCmd::Get(args) => get(&repo_root, &args.key),
        ConfigCmd::Set(args) => set(&repo_root, &args.key, &args.value, args.target),
        ConfigCmd::Unset(args) => unset(&repo_root, &args.key, args.global, args.local),
        ConfigCmd::Path => paths(&repo_root),
        ConfigCmd::Edit(target) => edit(&repo_root, target),
        ConfigCmd::Providers => models::list_providers_command(),
        ConfigCmd::Provider(args) => match args.target {
            Some(target) => provider::use_provider(provider::UseProviderArgs {
                target,
                model: args.model,
            }),
            None => provider_status(&repo_root),
        },
        ConfigCmd::Models(args) => models::run(args).await,
        ConfigCmd::Model(args) => match args.id {
            Some(id) => models::use_model(models::UseModelArgs { id }),
            None => model_status(&repo_root),
        },
        ConfigCmd::Keys(args) => key::list(&repo_root, args.all),
        ConfigCmd::Key(args) => match (args.var, args.value) {
            (Some(var), value) => key::set(&repo_root, &var, value.as_deref(), args.local, false),
            (None, _) => key_status(&repo_root),
        },
    }
}

/// The endpoint in use and where each part of it came from, so the read form
/// answers the same question every other read here does.
fn provider_status(repo_root: &Path) -> Result<()> {
    let settings = Settings::load(Some(repo_root))?;
    let (base_url, model) = provider::resolve_endpoint(&settings.review, None);
    let source = match (
        env_non_empty("ASTER_BASE_URL").is_some(),
        settings.review.base_url.is_some(),
    ) {
        (true, _) => "your shell",
        (false, true) => "aster.yaml",
        (false, false) => "the built-in default",
    };
    if crate::json_mode() {
        println!(
            "{}",
            json!({
                "ok": true,
                "provider": crate::init::provider_label(&base_url),
                "base_url": base_url,
                "model": model,
                "source": source,
            })
        );
        return Ok(());
    }
    println!("provider {}", crate::init::provider_label(&base_url));
    println!("endpoint {}", base_url);
    println!("model    {model}");
    println!("{}", paint(DIM, &format!("from {source}")));
    Ok(())
}

/// The model in use and where it came from.
fn model_status(repo_root: &Path) -> Result<()> {
    let settings = Settings::load(Some(repo_root))?;
    let (_, model) = provider::resolve_endpoint(&settings.review, None);
    let source = match (
        env_non_empty("ASTER_MODEL").is_some(),
        settings.review.model.is_some(),
    ) {
        (true, _) => "your shell",
        (false, true) => "aster.yaml",
        (false, false) => "the built-in default",
    };
    if crate::json_mode() {
        println!(
            "{}",
            json!({ "ok": true, "model": model, "source": source })
        );
        return Ok(());
    }
    println!("{model}");
    println!("{}", paint(DIM, &format!("from {source}")));
    Ok(())
}

/// The key situation for the endpoint in use: which var it reads, whether one
/// is set, and which layer is supplying it.
fn key_status(repo_root: &Path) -> Result<()> {
    let settings = Settings::load(Some(repo_root))?;
    let (base_url, _) = provider::resolve_endpoint(&settings.review, None);
    let vars = aster_ai::keys::key_vars(&base_url);
    let rows: Vec<(String, key::Source)> = vars
        .iter()
        .map(|v| ((*v).to_string(), key::source(v, repo_root)))
        .collect();
    if crate::json_mode() {
        let rows: Vec<_> = rows
            .iter()
            .map(|(v, s)| json!({ "var": v, "source": s.as_str() }))
            .collect();
        let setup = provider::Setup::needed(&base_url);
        println!(
            "{}",
            json!({
                "ok": true,
                "base_url": base_url,
                "vars": rows,
                "configured": setup.is_none(),
                "setup": setup,
            })
        );
        return Ok(());
    }
    println!(
        "{}",
        paint(
            BOLD,
            &format!("keys for {}", crate::init::provider_label(&base_url))
        )
    );
    for (var, source) in &rows {
        println!("  {var:<24} {}", source.label());
    }
    if rows.iter().all(|(_, s)| *s == key::Source::Unset) {
        println!(
            "{}",
            paint(
                DIM,
                "No key set for this endpoint. `aster config key <VAR>` stores one."
            )
        );
    }
    Ok(())
}

/// `aster config provider <target>`: the read form has no argument, the write
/// form takes the endpoint to switch to.
#[derive(Args)]
struct ProviderReadArgs {
    /// Provider id, name, or base URL, as `aster config providers` spells it.
    /// Left out, the endpoint in use is reported instead.
    #[arg(value_name = "PROVIDER")]
    target: Option<String>,

    /// Model to adopt with the endpoint. Defaults to its example model, since
    /// an endpoint kept with a model it does not serve fails next turn.
    #[arg(long, value_name = "ID")]
    model: Option<String>,
}

/// `aster config model <id>`: same read/write split as provider.
#[derive(Args)]
struct ModelReadArgs {
    /// Model id, as `aster config models` spells it. Left out, the model in
    /// use is reported instead.
    #[arg(value_name = "ID")]
    id: Option<String>,
}

/// `aster config key <var> [value]`: read the endpoint's key status, or store
/// any key var. The value is asked for without echoing when left out.
#[derive(Args)]
struct KeyReadArgs {
    /// The variable, e.g. `OPENROUTER_API_KEY`. `aster config keys` spells
    /// them all. Left out, the endpoint in use is reported on instead.
    #[arg(value_name = "VAR")]
    var: Option<String>,

    /// The key itself. Left out, it is asked for without echoing, which keeps
    /// it out of your shell history.
    #[arg(value_name = "VALUE")]
    value: Option<String>,

    /// Write this repo's `.env` instead of `~/.aster/.env`, and git-ignore it.
    #[arg(long)]
    local: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    Text,
    Bool,
    Number,
    List,
    Choice(&'static [&'static str]),
}

impl Kind {
    fn as_str(self) -> &'static str {
        match self {
            Kind::Text => "text",
            Kind::Bool => "bool",
            Kind::Number => "number",
            Kind::List => "list",
            Kind::Choice(_) => "choice",
        }
    }

    fn choices(self) -> &'static [&'static str] {
        match self {
            Kind::Choice(options) => options,
            _ => &[],
        }
    }
}

/// Display only: a written value is always the bare number.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Unit {
    None,
    Seconds,
    Chars,
    Bytes,
    Tokens,
    Percent,
}

impl Unit {
    fn as_str(self) -> &'static str {
        match self {
            Unit::None => "none",
            Unit::Seconds => "seconds",
            Unit::Chars => "chars",
            Unit::Bytes => "bytes",
            Unit::Tokens => "tokens",
            Unit::Percent => "percent",
        }
    }
}

/// How the form groups settings, which is not the section they sit in: `review`
/// holds the model every surface uses as well as the review pipeline's knobs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Group {
    Model,
    Permissions,
    Agent,
    Subagents,
    Review,
    Mcp,
    Ui,
}

impl Group {
    const ALL: [Group; 7] = [
        Group::Model,
        Group::Permissions,
        Group::Agent,
        Group::Subagents,
        Group::Review,
        Group::Mcp,
        Group::Ui,
    ];

    fn title(self) -> &'static str {
        match self {
            Group::Model => "Model and provider",
            Group::Permissions => "Permissions",
            Group::Agent => "Agent limits",
            Group::Subagents => "Sub-agents",
            Group::Review => "Code review",
            Group::Mcp => "MCP tools",
            Group::Ui => "Display",
        }
    }

    fn blurb(self) -> &'static str {
        match self {
            Group::Model => "what Aster talks to, and how hard it thinks",
            Group::Permissions => "what the agent may edit, read, and run",
            Group::Agent => "how far one turn may go",
            Group::Subagents => "the fan-out the agent tool is allowed",
            Group::Review => "the review pipeline only, not chat",
            Group::Mcp => "how much of the tool catalogue the model sees",
            Group::Ui => "what chat prints on its own",
        }
    }

    fn keys(self) -> impl Iterator<Item = (usize, &'static Key)> {
        KEYS.iter()
            .enumerate()
            .filter(move |(_, k)| k.group == self)
    }
}

#[derive(Debug)]
struct Key {
    /// Dotted path, `<section>.<key>`, matching docs/CONFIG.md.
    name: &'static str,
    label: &'static str,
    group: Group,
    kind: Kind,
    unit: Unit,
    /// Shell variables that outrank the file, in the order they win.
    env: &'static [&'static str],
    default: &'static str,
    help: &'static str,
}

impl Key {
    fn section(&self) -> &'static str {
        self.name.split_once('.').map_or(self.name, |(s, _)| s)
    }

    fn leaf(&self) -> &'static str {
        self.name.split_once('.').map_or(self.name, |(_, k)| k)
    }
}

const EFFORTS: &[&str] = &["off", "low", "medium", "high", "xhigh", "max", "ultra"];
const MODES: &[&str] = &["plan", "manual", "auto", "edit", "yolo"];

/// `mcp.servers` and `mcp.tools` are absent on purpose: `aster mcp` owns them.
const KEYS: &[Key] = &[
    Key {
        name: "review.model",
        label: "Default model",
        group: Group::Model,
        kind: Kind::Text,
        unit: Unit::None,
        env: &["ASTER_MODEL"],
        default: "openai/gpt-4o-mini",
        help: "Used by chat, review, and fix. The `review.` prefix is historical",
    },
    Key {
        name: "review.base_url",
        label: "Endpoint",
        group: Group::Model,
        kind: Kind::Text,
        unit: Unit::None,
        env: &["ASTER_BASE_URL"],
        default: aster_ai::DEFAULT_BASE_URL,
        help: "Any OpenAI-compatible URL, catalog or not; custom endpoints read their key from ASTER_API_KEY. `aster provider use` sets this and a model together",
    },
    Key {
        name: "review.effort",
        label: "Reasoning effort",
        group: Group::Model,
        kind: Kind::Choice(EFFORTS),
        unit: Unit::None,
        env: &["ASTER_EFFORT", "ASTER_REASONING_EFFORT"],
        default: "low",
        help: "How long a thinking model may reason before answering",
    },
    Key {
        name: "review.web_search",
        label: "Web search",
        group: Group::Model,
        kind: Kind::Bool,
        unit: Unit::None,
        env: &["ASTER_WEB_SEARCH"],
        default: "false",
        help: "Let OpenRouter search the web. No effect on other providers",
    },
    Key {
        name: "permissions.mode",
        label: "Mode",
        group: Group::Permissions,
        kind: Kind::Choice(MODES),
        unit: Unit::None,
        env: &[],
        default: "edit",
        help: "What happens to an action no rule matched",
    },
    Key {
        name: "permissions.allow",
        label: "Always allow",
        group: Group::Permissions,
        kind: Kind::List,
        unit: Unit::None,
        env: &[],
        default: "nothing",
        help: "Rules that run without asking, e.g. Bash(cargo test:*)",
    },
    Key {
        name: "permissions.ask",
        label: "Always ask",
        group: Group::Permissions,
        kind: Kind::List,
        unit: Unit::None,
        env: &[],
        default: "nothing",
        help: "Rules confirmed first, whatever the mode",
    },
    Key {
        name: "permissions.deny",
        label: "Never allow",
        group: Group::Permissions,
        kind: Kind::List,
        unit: Unit::None,
        env: &[],
        default: "nothing",
        help: "Rules refused outright, e.g. Edit(infra/**)",
    },
    Key {
        name: "permissions.use_default_rules",
        label: "Built-in rules",
        group: Group::Permissions,
        kind: Kind::Bool,
        unit: Unit::None,
        env: &[],
        default: "true",
        help: "Guards on .git, CI workflows, risky commands, and secret files",
    },
    Key {
        name: "permissions.additional_directories",
        label: "Readable directories",
        group: Group::Permissions,
        kind: Kind::List,
        unit: Unit::None,
        env: &[],
        default: "the repo only",
        help: "Directories outside the repo the agent may read without asking",
    },
    Key {
        name: "permissions.allow_credentials",
        label: "Credential access",
        group: Group::Permissions,
        kind: Kind::List,
        unit: Unit::None,
        env: &[],
        default: "nothing",
        help: "Credential dirs a command may read, written gh:~/.config/gh",
    },
    Key {
        name: "agent.max_tool_rounds",
        label: "Tool rounds per turn",
        group: Group::Agent,
        kind: Kind::Number,
        unit: Unit::None,
        env: &["ASTER_MAX_TOOL_ROUNDS"],
        default: "60",
        help: "Rounds before the agent must answer with what it has",
    },
    Key {
        name: "agent.command_timeout_secs",
        label: "Command timeout",
        group: Group::Agent,
        kind: Kind::Number,
        unit: Unit::Seconds,
        env: &["ASTER_COMMAND_TIMEOUT"],
        default: "300",
        help: "How long one command may run. Builds and test suites live here",
    },
    Key {
        name: "agent.compact_budget_chars",
        label: "Compact after",
        group: Group::Agent,
        kind: Kind::Number,
        unit: Unit::Chars,
        env: &["ASTER_COMPACT_BUDGET"],
        default: "192000",
        help: "History size at which older turns fold into a summary. Lower it for small-context models",
    },
    Key {
        name: "agents.collector_model",
        label: "Collector model",
        group: Group::Subagents,
        kind: Kind::Text,
        unit: Unit::None,
        env: &["ASTER_COLLECTOR_MODEL"],
        default: "the main model",
        help: "A cheaper model for sub-agents that only gather",
    },
    Key {
        name: "agents.max_concurrent",
        label: "Running at once",
        group: Group::Subagents,
        kind: Kind::Number,
        unit: Unit::None,
        env: &["ASTER_AGENT_MAX_CONCURRENT"],
        default: "8",
        help: "Sub-agents allowed to run in parallel",
    },
    Key {
        name: "agents.max_per_turn",
        label: "Tasks per turn",
        group: Group::Subagents,
        kind: Kind::Number,
        unit: Unit::None,
        env: &["ASTER_AGENT_MAX_PER_TURN"],
        default: "24",
        help: "Sub-agent tasks one turn may start",
    },
    Key {
        name: "agents.agent_timeout_secs",
        label: "Sub-agent timeout",
        group: Group::Subagents,
        kind: Kind::Number,
        unit: Unit::Seconds,
        env: &["ASTER_AGENT_TIMEOUT"],
        default: "300",
        help: "How long a single sub-agent may run",
    },
    Key {
        name: "review.hypothesis_model",
        label: "First-pass model",
        group: Group::Review,
        kind: Kind::Text,
        unit: Unit::None,
        env: &["ASTER_HYPOTHESIS_MODEL"],
        default: "the main model",
        help: "Cheap, high-recall model that proposes findings",
    },
    Key {
        name: "review.verify_model",
        label: "Verify model",
        group: Group::Review,
        kind: Kind::Text,
        unit: Unit::None,
        env: &["ASTER_VERIFY_MODEL"],
        default: "the main model",
        help: "Independent model that tries to refute each finding",
    },
    Key {
        name: "review.min_confidence",
        label: "Confidence floor",
        group: Group::Review,
        kind: Kind::Number,
        unit: Unit::None,
        env: &[],
        default: "0.5",
        help: "Findings the verifier is less sure of than this are dropped (0 to 1)",
    },
    Key {
        name: "review.max_diff_bytes",
        label: "Largest diff",
        group: Group::Review,
        kind: Kind::Number,
        unit: Unit::Bytes,
        env: &[],
        default: "200000",
        help: "Diffs longer than this are truncated before the model sees them",
    },
    Key {
        name: "review.analyzers",
        label: "Static analyzers",
        group: Group::Review,
        kind: Kind::List,
        unit: Unit::None,
        env: &["ASTER_ANALYZERS"],
        default: "none",
        help: "Backends whose findings also get verified: semgrep, ast-grep",
    },
    Key {
        name: "review.astgrep_rules",
        label: "ast-grep rules",
        group: Group::Review,
        kind: Kind::Text,
        unit: Unit::None,
        env: &["ASTER_ASTGREP_RULES"],
        default: "none",
        help: "Repo-relative path to an ast-grep rule YAML",
    },
    Key {
        name: "review.focus_areas",
        label: "Focus areas",
        group: Group::Review,
        kind: Kind::List,
        unit: Unit::None,
        env: &[],
        default: "everything",
        help: "Defect classes the first pass leans toward, e.g. correctness, security",
    },
    Key {
        name: "review.include",
        label: "Only review",
        group: Group::Review,
        kind: Kind::List,
        unit: Unit::None,
        env: &[],
        default: "everything",
        help: "Globs to review. Empty means every file the diff touches",
    },
    Key {
        name: "review.exclude",
        label: "Never review",
        group: Group::Review,
        kind: Kind::List,
        unit: Unit::None,
        env: &[],
        default: "the built-in list",
        help: "Globs to skip, on top of lockfiles and build output",
    },
    Key {
        name: "mcp.context_tokens",
        label: "Context window",
        group: Group::Mcp,
        kind: Kind::Number,
        unit: Unit::Tokens,
        env: &[],
        default: "100000",
        help: "The window the tool inventory is measured against",
    },
    Key {
        name: "mcp.inventory_percent",
        label: "Inventory budget",
        group: Group::Mcp,
        kind: Kind::Number,
        unit: Unit::Percent,
        env: &[],
        default: "1.5",
        help: "Share of that window tool descriptions may spend before the model has to search instead",
    },
    Key {
        name: "mcp.search_limit",
        label: "Search results",
        group: Group::Mcp,
        kind: Kind::Number,
        unit: Unit::None,
        env: &[],
        default: "10",
        help: "Tools returned by one search",
    },
    Key {
        name: "ui.welcome",
        label: "Session header",
        group: Group::Ui,
        kind: Kind::Bool,
        unit: Unit::None,
        env: &[],
        default: "true",
        help: "Print the model, provider, and skills header when chat starts",
    },
];

fn key(name: &str) -> Result<&'static Key> {
    if let Some(key) = KEYS.iter().find(|k| k.name == name) {
        return Ok(key);
    }
    if name.starts_with("mcp.servers") || name.starts_with("mcp.tools") {
        bail!("{name} is managed by `aster mcp`, not `aster config`");
    }
    match nearest(name) {
        Some(near) => bail!("no config key {name:?}. Did you mean {near}?"),
        None => bail!("no config key {name:?}. `aster config list` shows every key"),
    }
}

/// The closest key by shared prefix, so a half-remembered name still lands.
fn nearest(name: &str) -> Option<&'static str> {
    let shared = |k: &Key| {
        k.name
            .bytes()
            .zip(name.bytes())
            .take_while(|(a, b)| a == b)
            .count()
    };
    KEYS.iter()
        .filter(|k| shared(k) >= 4)
        .max_by_key(|k| shared(k))
        .map(|k| k.name)
}

/// The merged value across the config files, `Null` when none of them sets it.
fn configured(settings: &Settings, name: &str) -> Value {
    let review = &settings.review;
    let perms = &settings.permissions;
    let agent = &settings.agent;
    let agents = &settings.agents;
    let mcp = &settings.mcp;
    match name {
        "review.model" => json!(review.model),
        "review.base_url" => json!(review.base_url),
        "review.effort" => json!(review.effort.map(|e| e.as_str())),
        "review.web_search" => json!(review.web_search),
        "review.hypothesis_model" => json!(review.hypothesis_model),
        "review.verify_model" => json!(review.verify_model),
        "review.min_confidence" => review.min_confidence.map_or(Value::Null, float),
        "review.max_diff_bytes" => json!(review.max_diff_bytes),
        "review.analyzers" => json!(review.analyzers),
        "review.astgrep_rules" => json!(review.astgrep_rules),
        "review.focus_areas" => json!(review.focus_areas),
        "review.include" => json!(review.include),
        "review.exclude" => json!(review.exclude),
        "permissions.mode" => json!(perms.mode.as_str()),
        "permissions.allow" => json!(perms.allow),
        "permissions.ask" => json!(perms.ask),
        "permissions.deny" => json!(perms.deny),
        "permissions.use_default_rules" => json!(perms.use_default_rules),
        "permissions.additional_directories" => json!(perms.additional_directories),
        "permissions.allow_credentials" => json!(perms.allow_credentials),
        "agent.max_tool_rounds" => json!(agent.max_tool_rounds),
        "agent.command_timeout_secs" => json!(agent.command_timeout_secs),
        "agent.compact_budget_chars" => json!(agent.compact_budget_chars),
        "agents.collector_model" => json!(agents.collector_model),
        "agents.max_concurrent" => json!(agents.max_concurrent),
        "agents.max_per_turn" => json!(agents.max_per_turn),
        "agents.agent_timeout_secs" => json!(agents.agent_timeout_secs),
        "mcp.context_tokens" => json!(mcp.context_tokens),
        "mcp.inventory_percent" => float(mcp.inventory_percent),
        "mcp.search_limit" => json!(mcp.search_limit),
        "ui.welcome" => json!(settings.ui.welcome),
        _ => Value::Null,
    }
}

/// An f32 widened to f64 gains digits that were never in it, so 0.6 would print
/// as 0.6000000238. Its own shortest representation is the one to keep.
fn float(value: f32) -> Value {
    value
        .to_string()
        .parse::<f64>()
        .map_or(Value::Null, |v| json!(v))
}

/// One config file, kept as text so a key's origin comes from the file itself
/// rather than from a merged value that happens to equal its default.
struct Layer {
    path: PathBuf,
    label: String,
    text: String,
}

fn layers(repo_root: &Path) -> Vec<Layer> {
    let mut out = Vec::new();
    for path in [
        crate::settings::user_config().ok(),
        crate::settings::project_config(Some(repo_root)),
    ]
    .into_iter()
    .flatten()
    {
        if let Ok(text) = fs::read_to_string(&path) {
            let label = label(&path, repo_root);
            out.push(Layer { path, label, text });
        }
    }
    out
}

struct Resolved {
    value: Value,
    /// `env ASTER_MODEL`, the files that set it, or `default`.
    source: String,
    /// Set when the shell outranks a file that also sets the key.
    shadowed: Option<&'static str>,
}

/// What one file sets for a key on its own. `pins` decides whether it sets it at
/// all, since a `Settings` parsed from a file reports defaults for what it omits.
fn in_layer(key: &Key, layer: &Layer) -> Value {
    if !crate::settings::pins(&layer.text, key.section(), key.leaf()) {
        return Value::Null;
    }
    serde_yaml::from_str::<Settings>(&layer.text)
        .map(|settings| configured(&settings, key.name))
        .unwrap_or(Value::Null)
}

/// Each file's own answer, so an editor can write back to the file the user
/// picked rather than the one that won.
fn scoped(key: &Key, layers: &[Layer], global: &Path) -> Value {
    let mut user = Value::Null;
    let mut workspace = Value::Null;
    for layer in layers {
        match layer.path == global {
            true => user = in_layer(key, layer),
            false => workspace = in_layer(key, layer),
        }
    }
    json!({ "global": user, "local": workspace })
}

fn resolve(key: &Key, settings: &Settings, layers: &[Layer]) -> Resolved {
    let from_env = key
        .env
        .iter()
        .find_map(|var| Some((*var, env_non_empty(var)?)));
    resolve_from(key, settings, layers, from_env)
}

/// The shell wins, then whichever files set the key, then the default. Taking
/// the environment as an argument keeps the decision testable.
fn resolve_from(
    key: &Key,
    settings: &Settings,
    layers: &[Layer],
    from_env: Option<(&'static str, String)>,
) -> Resolved {
    let files: Vec<&Layer> = layers
        .iter()
        .filter(|l| crate::settings::pins(&l.text, key.section(), key.leaf()))
        .collect();

    if let Some((var, value)) = from_env {
        return Resolved {
            value: json!(value),
            source: format!("env {var}"),
            shadowed: (!files.is_empty()).then_some(var),
        };
    }
    let value = configured(settings, key.name);
    if value.is_null() || files.is_empty() {
        return Resolved {
            value: Value::Null,
            source: "default".into(),
            shadowed: None,
        };
    }
    Resolved {
        value,
        source: files
            .iter()
            .map(|l| l.label.clone())
            .collect::<Vec<_>>()
            .join(" + "),
        shadowed: None,
    }
}

/// The value as it is written in the file. `Null` means no file set it, so the
/// default stands in.
fn render(value: &Value, key: &Key) -> String {
    match value {
        Value::Null => key.default.to_string(),
        Value::String(s) => s.clone(),
        Value::Array(items) if items.is_empty() => "[]".into(),
        Value::Array(items) => items
            .iter()
            .map(|i| {
                i.as_str()
                    .map(str::to_string)
                    .unwrap_or_else(|| i.to_string())
            })
            .collect::<Vec<_>>()
            .join(", "),
        other => other.to_string(),
    }
}

/// The value as a person reads it, never written back: an empty list says what
/// empty means, and a number carries what it counts.
fn display(value: &Value, key: &Key) -> String {
    let empty = matches!(value, Value::Array(items) if items.is_empty());
    let text = match value.is_null() || empty {
        true => key.default.to_string(),
        false => render(value, key),
    };
    with_unit(&text, key.unit)
}

/// The unit lands on a number and nowhere else, so a default written in words is
/// left as it reads.
fn with_unit(text: &str, unit: Unit) -> String {
    let Ok(number) = text.parse::<f64>() else {
        return text.to_string();
    };
    let count = crate::util::human(number as u64);
    match unit {
        Unit::None => text.to_string(),
        Unit::Seconds => format!("{text}s"),
        Unit::Percent => format!("{text}%"),
        Unit::Chars => format!("{count} chars"),
        Unit::Bytes => format!("{count} bytes"),
        Unit::Tokens => format!("{count} tokens"),
    }
}

/// The first screen: the provider, the keys, a group of settings, the file
/// writes land in, or the way out.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Top {
    Provider,
    Keys,
    Group(Group),
    Scope,
    Done,
}

/// The second screen: a setting to change, or back to the groups.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Row {
    Key(usize),
    Back,
}

/// The form a bare `aster config` opens. Every write goes through `set` and
/// `unset`'s path, so the form can do nothing the flags cannot.
pub(crate) async fn menu(repo_root: &Path) -> Result<()> {
    set_theme(crate::init::AsterTheme);
    print!("{}", crate::tui::mark_ansi());

    let mut target = Target {
        global: crate::settings::project_config(Some(repo_root)).is_none(),
        local: crate::settings::project_config(Some(repo_root)).is_some(),
    };
    let mut at = Top::Provider;
    let mut saved = 0usize;

    log::info(headline(repo_root)?)?;
    loop {
        let path = target.path(repo_root)?;
        let mut menu = select::<Top>("What do you want to change?").initial_value(at);
        menu = menu.item(Top::Provider, "Provider & model", provider_hint(repo_root)?);
        menu = menu.item(
            Top::Keys,
            "API keys",
            "every key Aster reads, stored in .env",
        );
        for group in Group::ALL {
            menu = menu.item(Top::Group(group), group.title(), group.blurb());
        }
        menu = menu.item(Top::Scope, "Save to", label(&path, repo_root));
        menu = menu.item(Top::Done, "Done", "close the form");

        let Some(top) = or_cancel(menu.interact())? else {
            break;
        };
        at = top;
        match top {
            Top::Done => break,
            Top::Scope => target = flip(target),
            Top::Provider => saved += provider_flow(target, repo_root).await?,
            Top::Keys => saved += keys_flow(repo_root, target.local)?,
            Top::Group(group) => saved += settings_in(group, &path, repo_root)?,
        }
    }

    outro(match saved {
        0 => "Nothing changed.".to_string(),
        1 => "1 setting saved.".to_string(),
        n => format!("{n} settings saved."),
    })?;
    Ok(())
}

fn provider_hint(repo_root: &Path) -> Result<String> {
    let settings = Settings::load(Some(repo_root))?;
    let (base_url, model) = provider::resolve_endpoint(&settings.review, None);
    Ok(format!(
        "{} · {model}",
        crate::init::provider_label(&base_url)
    ))
}

/// The init wizard's provider step, reached from the form: endpoint, model,
/// and optionally the key, saved to the scoped file. Returns writes made.
async fn provider_flow(target: Target, repo_root: &Path) -> Result<usize> {
    let providers = crate::init::load_providers()?;
    let current = crate::init::Current::read(repo_root);
    let Some(chosen) = crate::init::provider_setup(&providers, &current).await? else {
        return Ok(0);
    };
    let path = target.path(repo_root)?;
    crate::settings::write_review(
        &path,
        &[("base_url", &chosen.base_url), ("model", &chosen.model)],
    )?;
    log::success(format!(
        "{} · {} · saved to {}",
        crate::init::provider_label(&chosen.base_url),
        chosen.model,
        label(&path, repo_root)
    ))?;
    let mut saved = 1;
    if let Some(key) = chosen.api_key.as_deref() {
        key::set(repo_root, chosen.key_var, Some(key), target.local, false)?;
        saved += 1;
    }
    for var in ["ASTER_BASE_URL", "ASTER_MODEL"] {
        if env_non_empty(var).is_some() {
            log::warning(format!(
                "{var} is set in this shell and outranks the saved value"
            ))?;
        }
    }
    Ok(saved)
}

/// Every key Aster reads in one list: pick one, then type a value to store it,
/// enter to keep it, or `-` to clear it. Returns writes made.
fn keys_flow(repo_root: &Path, local: bool) -> Result<usize> {
    let mut saved = 0usize;
    let mut at = 0usize;
    loop {
        let rows = key::rows(repo_root, true);
        let back = rows.len();
        let mut menu = select::<usize>("Which key? (type to search)")
            .initial_value(at.min(back))
            .filter_mode()
            .max_rows(12);
        for (i, row) in rows.iter().enumerate() {
            let state = match &row.masked {
                Some(tail) => format!("{tail} · {}", row.source.label()),
                None => "not set".to_string(),
            };
            menu = menu.item(i, row.var, format!("{} · {state}", row.label));
        }
        menu = menu.item(back, "Back", "");

        let Some(i) = or_cancel(menu.interact())? else {
            break;
        };
        at = i;
        if i == back {
            break;
        }
        let row = &rows[i];
        let prompt = match &row.masked {
            Some(tail) => format!(
                "{} ({tail} is stored · enter to keep · {CLEAR} to clear)",
                row.var
            ),
            None => format!("{} (enter to go back)", row.var),
        };
        let Some(entered) = or_cancel(password(prompt).mask('•').allow_empty().interact())?
        else {
            continue;
        };
        let entered = entered.trim().to_string();
        if entered.is_empty() {
            continue;
        }
        if entered == CLEAR {
            key::unset(repo_root, row.var, false, false)?;
        } else {
            key::set(repo_root, row.var, Some(&entered), local, false)?;
        }
        saved += 1;
    }
    Ok(saved)
}

/// One group's settings, until Back. Returns how many were written.
fn settings_in(group: Group, path: &Path, repo_root: &Path) -> Result<usize> {
    let mut saved = 0;
    let mut at = group
        .keys()
        .next()
        .map(|(i, _)| Row::Key(i))
        .unwrap_or(Row::Back);
    let width = group.keys().map(|(_, k)| k.label.len()).max().unwrap_or(0);

    loop {
        let settings = Settings::load(Some(repo_root))?;
        let layers = layers(repo_root);
        // The value goes in the label rather than the hint: clack shows a hint
        // for the highlighted row only.
        let mut menu = select::<Row>(group.title()).initial_value(at).max_rows(12);
        for (i, key) in group.keys() {
            let resolved = resolve(key, &settings, &layers);
            let row = format!(
                "{:<width$}  {}",
                key.label,
                display(&resolved.value, key),
                width = width
            );
            menu = menu.item(
                Row::Key(i),
                row,
                format!("{} · {}", key.name, resolved.source),
            );
        }
        menu = menu.item(Row::Back, "Back", "");

        let Some(row) = or_cancel(menu.interact())? else {
            break;
        };
        at = row;
        match row {
            Row::Back => break,
            Row::Key(i) => {
                if change(&KEYS[i], &settings, path, repo_root)? {
                    saved += 1;
                }
            }
        }
    }
    Ok(saved)
}

/// What the next turn runs with, so the form opens on the same answer `aster
/// status` gives.
fn headline(repo_root: &Path) -> Result<String> {
    let settings = Settings::load(Some(repo_root))?;
    let (base_url, model) = provider::resolve_endpoint(&settings.review, None);
    Ok(format!(
        "{} · {model} · {} mode",
        crate::init::provider_label(&base_url),
        settings.permissions.mode.as_str()
    ))
}

fn flip(target: Target) -> Target {
    Target {
        global: !target.global,
        local: target.global,
    }
}

enum Answer {
    Set(String),
    Clear,
    Keep,
}

/// Typed at a value prompt to clear the key. Clack turns an empty submit back
/// into the default it prefilled, so "leave it blank" cannot mean anything.
const CLEAR: &str = "-";

/// Prompt for one key and write it. `true` when the file changed.
fn change(key: &'static Key, settings: &Settings, path: &Path, repo_root: &Path) -> Result<bool> {
    // Prefilled from the file rather than from a shell variable that outranks it.
    let current = configured(settings, key.name);
    let answer = match key.kind {
        Kind::Choice(options) => pick(key, options, &current)?,
        Kind::Bool => pick(key, &["true", "false"], &current)?,
        _ => typed(key, &current, path)?,
    };

    let text = fs::read_to_string(path).unwrap_or_default();
    let updated = match &answer {
        Answer::Keep => return Ok(false),
        Answer::Clear => crate::settings::without_key(&text, key.section(), key.leaf()),
        Answer::Set(value) => Some(crate::settings::with_key(
            &text,
            key.section(),
            key.leaf(),
            &yaml_value(key, value),
        )),
    };
    let Some(updated) = updated else {
        log::info(format!("{} is not set in this file", key.label))?;
        return Ok(false);
    };
    if let Answer::Set(value) = &answer {
        check(&updated).with_context(|| format!("{} cannot be set to {value:?}", key.name))?;
    }
    crate::settings::save(path, updated)?;

    let settings = Settings::load(Some(repo_root))?;
    let resolved = resolve(key, &settings, &layers(repo_root));
    log::success(format!(
        "{} is now {} · saved to {}",
        key.label,
        display(&resolved.value, key),
        label(path, repo_root)
    ))?;
    if let Some(var) = resolved.shadowed {
        log::warning(format!("{var} is set in this shell and outranks it"))?;
    }
    Ok(true)
}

/// A fixed-value key picks from its values, plus a row that clears it.
fn pick(key: &Key, options: &'static [&'static str], current: &Value) -> Result<Answer> {
    let now = current.as_str().map(str::to_string);
    let mut menu = select::<Option<&str>>(key.help)
        .initial_value(now.as_deref().filter(|v| options.contains(v)));
    for option in options {
        menu = menu.item(Some(*option), *option, "");
    }
    menu = menu.item(None, "default", format!("clear it · {}", key.default));

    Ok(match or_cancel(menu.interact())? {
        None => Answer::Keep,
        Some(None) => Answer::Clear,
        // Enter on the value already highlighted is a look, not a change.
        Some(Some(value)) if now.as_deref() == Some(value) => Answer::Keep,
        Some(Some(value)) => Answer::Set(value.to_string()),
    })
}

/// Everything else is typed, validated against the parser before the prompt
/// closes, so a bad value is corrected in place.
fn typed(key: &'static Key, current: &Value, path: &Path) -> Result<Answer> {
    let text = fs::read_to_string(path).unwrap_or_default();
    let validate = move |input: &String| {
        let value = input.trim();
        if value.is_empty() || value == CLEAR {
            return Ok(());
        }
        let candidate =
            crate::settings::with_key(&text, key.section(), key.leaf(), &yaml_value(key, value));
        check(&candidate).map_err(|e| format!("{e:#}"))
    };

    let now = (!current.is_null()).then(|| render(current, key));
    let mut input = cliclack::input(format!("{} — {}", key.label, key.help))
        .required(false)
        .validate(validate)
        .placeholder(&match &now {
            Some(_) => format!("enter keeps it · {CLEAR} clears it"),
            None => format!("{} · enter keeps the default", key.default),
        });
    if let Some(now) = &now {
        input = input.default_input(now);
    }

    let Some(answer) = or_cancel(input.interact::<String>())? else {
        return Ok(Answer::Keep);
    };
    let answer = answer.trim();
    Ok(match answer {
        "" => Answer::Keep,
        CLEAR => Answer::Clear,
        // Enter on an untouched prompt hands back what was already there.
        _ if now.as_deref() == Some(answer) => Answer::Keep,
        _ => Answer::Set(answer.to_string()),
    })
}

fn list(repo_root: &Path) -> Result<()> {
    let settings = Settings::load(Some(repo_root))?;
    let layers = layers(repo_root);
    let resolved = |key| resolve(key, &settings, &layers);

    if crate::json_mode() {
        let global = crate::settings::user_config()?;
        let keys: Vec<Value> = KEYS
            .iter()
            .map(|key| {
                let resolved = resolved(key);
                json!({
                    "key": key.name,
                    "label": key.label,
                    "group": key.group.title(),
                    "kind": key.kind.as_str(),
                    "choices": key.kind.choices(),
                    "unit": key.unit.as_str(),
                    "value": resolved.value,
                    "display": display(&resolved.value, key),
                    "default": key.default,
                    "source": resolved.source,
                    "shadowed": resolved.shadowed,
                    "scopes": scoped(key, &layers, &global),
                    "env": key.env,
                    "help": key.help,
                })
            })
            .collect();
        println!("{}", json!({ "ok": true, "keys": keys }));
        return Ok(());
    }

    for group in Group::ALL {
        let rows: Vec<(&Key, Resolved)> = group.keys().map(|(_, k)| (k, resolved(k))).collect();
        let label_width = rows.iter().map(|(k, _)| k.label.len()).max().unwrap_or(0);
        let value_width = rows
            .iter()
            .map(|(k, r)| display(&r.value, k).chars().count())
            .max()
            .unwrap_or(0)
            .min(34);

        println!("{}", paint(BOLD, group.title()));
        for (key, resolved) in &rows {
            let source = format!("{} · {}", key.name, resolved.source);
            println!(
                "  {:<label_width$}  {:<value_width$}  {}",
                key.label,
                display(&resolved.value, key),
                paint(DIM, &source)
            );
        }
        println!();
    }
    Ok(())
}

fn get(repo_root: &Path, name: &str) -> Result<()> {
    let key = key(name)?;
    let settings = Settings::load(Some(repo_root))?;
    let resolved = resolve(key, &settings, &layers(repo_root));
    let value = render(&resolved.value, key);

    if crate::json_mode() {
        println!(
            "{}",
            json!({
                "ok": true,
                "key": key.name,
                "value": resolved.value,
                "default": key.default,
                "source": resolved.source,
            })
        );
        return Ok(());
    }
    println!("{value}");
    if let Some(var) = resolved.shadowed {
        eprintln!("note: {var} is set in this shell and outranks the config file");
    }
    Ok(())
}

fn set(repo_root: &Path, name: &str, value: &str, target: Target) -> Result<()> {
    let key = key(name)?;
    let path = target.path(repo_root)?;
    let text = fs::read_to_string(&path).unwrap_or_default();
    // A file that already fails to parse would make the next error read as
    // this write's fault.
    check(&text)
        .with_context(|| format!("{} does not parse; fix it first", label(&path, repo_root)))?;

    let written = yaml_value(key, value);
    let updated = crate::settings::with_key(&text, key.section(), key.leaf(), &written);
    check(&updated).with_context(|| format!("{name} cannot be set to {value:?}"))?;
    crate::settings::save(&path, updated)?;

    report(repo_root, key, &path, "saved to")
}

fn unset(repo_root: &Path, name: &str, global: bool, local: bool) -> Result<()> {
    let key = key(name)?;
    // Clearing one file while the other still sets the key would look like the
    // command did nothing, so by default every file that sets it is cleared.
    let targets: Vec<PathBuf> = match (global, local) {
        (false, false) => layers(repo_root).into_iter().map(|l| l.path).collect(),
        _ => vec![Target { global, local }.path(repo_root)?],
    };
    let mut cleared = Vec::new();
    for path in targets {
        let text = fs::read_to_string(&path).unwrap_or_default();
        let Some(updated) = crate::settings::without_key(&text, key.section(), key.leaf()) else {
            continue;
        };
        check(&updated)
            .with_context(|| format!("removing {name} from {}", label(&path, repo_root)))?;
        crate::settings::save(&path, updated)?;
        cleared.push(path);
    }

    let Some(path) = cleared.first().cloned() else {
        if crate::json_mode() {
            println!(
                "{}",
                json!({ "ok": true, "key": key.name, "changed": false })
            );
        } else {
            println!("{name} was not set; it already reads {}", key.default);
        }
        return Ok(());
    };
    report(repo_root, key, &path, "cleared from")
}

/// What the next turn resolves the key to now, so an environment variable
/// outranking what was just written is not hidden.
fn report(repo_root: &Path, key: &Key, path: &Path, verb: &str) -> Result<()> {
    let settings = Settings::load(Some(repo_root))?;
    let resolved = resolve(key, &settings, &layers(repo_root));

    if crate::json_mode() {
        println!(
            "{}",
            json!({
                "ok": true,
                "key": key.name,
                "value": resolved.value,
                "source": resolved.source,
                "path": path.display().to_string(),
                "changed": true,
            })
        );
        return Ok(());
    }
    println!("{} {}", key.name, render(&resolved.value, key));
    println!("{verb} {}", label(path, repo_root));
    if let Some(var) = resolved.shadowed {
        eprintln!("note: {var} is set in this shell and outranks the saved value");
    }
    Ok(())
}

fn paths(repo_root: &Path) -> Result<()> {
    let global = crate::settings::user_config()?;
    let project = crate::settings::project_config(Some(repo_root));

    if crate::json_mode() {
        println!(
            "{}",
            json!({
                "ok": true,
                "global": global.display().to_string(),
                "global_exists": global.exists(),
                "project": project.as_ref().map(|p| p.display().to_string()),
                "project_exists": project.as_ref().is_some_and(|p| p.exists()),
                "project_default": repo_root.join("aster.yaml").display().to_string(),
            })
        );
        return Ok(());
    }
    println!(
        "global   {}{}",
        label(&global, repo_root),
        if global.exists() { "" } else { "  (none yet)" }
    );
    match &project {
        Some(path) => println!("project  {}", label(path, repo_root)),
        None => println!("project  (none in this repo)"),
    }
    Ok(())
}

fn edit(repo_root: &Path, target: Target) -> Result<()> {
    let path = target.path(repo_root)?;
    if !path.exists() {
        crate::settings::save(&path, String::new())?;
    }
    let editor = ["ASTER_EDITOR", "VISUAL", "EDITOR"]
        .iter()
        .find_map(|var| env_non_empty(var))
        .unwrap_or_else(|| if cfg!(windows) { "notepad" } else { "vi" }.into());
    // `EDITOR` carries flags often enough that taking it as one word would
    // break `code -w` and `emacsclient -nw`.
    let mut words = editor.split_whitespace();
    let program = words.next().context("empty editor command")?;
    let status = process::Command::new(program)
        .args(words)
        .arg(&path)
        .status()
        .with_context(|| format!("running {editor}"))?;
    if !status.success() {
        bail!("{editor} exited with {status}");
    }

    let text = fs::read_to_string(&path).unwrap_or_default();
    let problem = check(&text).err().map(|e| format!("{e:#}"));
    if crate::json_mode() {
        println!(
            "{}",
            json!({
                "ok": problem.is_none(),
                "path": path.display().to_string(),
                "error": problem,
            })
        );
        return Ok(());
    }
    match problem {
        Some(problem) => bail!("{} no longer parses: {problem}", label(&path, repo_root)),
        None => println!("{} parses", label(&path, repo_root)),
    }
    Ok(())
}

impl Target {
    fn path(self, repo_root: &Path) -> Result<PathBuf> {
        match (self.global, self.local) {
            (true, _) => crate::settings::user_config(),
            (_, true) => Ok(repo_root.join("aster.yaml")),
            _ => crate::settings::writable_config(Some(repo_root)),
        }
    }
}

/// Lists are written inline so one key stays one line, which is what keeps the
/// rest of the file byte for byte.
fn yaml_value(key: &Key, raw: &str) -> String {
    if key.kind != Kind::List {
        return scalar(raw);
    }
    let items: Vec<String> = raw
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| format!("\"{}\"", s.replace('"', "\\\"")))
        .collect();
    format!("[{}]", items.join(", "))
}

/// Quote anything YAML would read as something other than a string.
fn scalar(raw: &str) -> String {
    let plain = !raw.is_empty()
        && raw
            .chars()
            .all(|c| c.is_alphanumeric() || "-_./:+".contains(c))
        && !raw.starts_with(['-', '.'])
        && !raw.contains(": ");
    if plain {
        return raw.to_string();
    }
    format!("\"{}\"", raw.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Validated with the parser the next run uses, so nothing unreadable is saved.
fn check(text: &str) -> Result<()> {
    serde_yaml::from_str::<Settings>(text)?;
    Ok(())
}

/// How a config file prints: repo-relative inside the repo, `~/…` under the home
/// directory.
fn label(path: &Path, repo_root: &Path) -> String {
    if let Ok(rest) = path.strip_prefix(repo_root) {
        return rest.display().to_string();
    }
    let home = dirs::home_dir().and_then(|home| Some(path.strip_prefix(home).ok()?.to_path_buf()));
    match home {
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}

#[cfg(test)]
#[path = "../tests/config_test.rs"]
mod tests;
