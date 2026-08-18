#![forbid(unsafe_code)]

mod agents;
mod auth;
mod budget;
mod chat;
mod config;
mod edits;
mod fix;
mod git;
mod github;
mod images;
mod import;
mod init;
mod instructions;
mod mcp;
mod models;
mod persist;
mod picker;
mod plugins;
mod project;
mod provider;
mod redact;
mod remote;
mod review;
mod sessions;
mod settings;
mod skills;
mod status;
mod term;
mod test_runner;
mod tui;
mod update;
mod util;
mod web;

use std::env;
use std::fs;
use std::io::stderr;
use std::process;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use anyhow::Result;
use aster_ai::Effort;
use clap::Parser;
use clap::Subcommand;
use clap::ValueEnum;

#[derive(Parser)]
#[command(
    name = "aster",
    version,
    about = "A self-hostable agent harness for software work",
    args_conflicts_with_subcommands = true
)]
struct Cli {
    /// Bare `aster` chats: the interactive TUI in a terminal, one-shot otherwise.
    #[command(subcommand)]
    command: Option<Command>,

    /// Chat flags accepted directly on the root: `aster --resume`, `aster -p "…"`.
    #[command(flatten)]
    chat: chat::ChatArgs,

    /// Emit JSON on stdout instead of human text. Accepted by every subcommand,
    /// before or after it, and turns errors into `{"ok":false,"error":…}`.
    #[arg(long, global = true)]
    json: bool,

    /// Hidden alias for `--json`, matching the `--format json` spelling other CLIs use.
    #[arg(long, global = true, value_name = "FORMAT", hide = true)]
    format: Option<OutputFormat>,
}

/// Flattened into the commands that reach a provider, so the flag stays off
/// the help of the ones that never run a model.
#[derive(clap::Args, Clone, Copy)]
pub struct EffortArgs {
    /// Reasoning budget for thinking models: off, low, medium, or high.
    /// Overrides ASTER_EFFORT and aster.yaml `review.effort`.
    #[arg(long, value_name = "LEVEL")]
    pub effort: Option<Effort>,
}

#[derive(Subcommand)]
enum Command {
    /// Set up Aster in this repo: pick a provider, write aster.yaml, store your key.
    Init(init::InitArgs),
    /// Link a GitHub account via device flow (opens your browser).
    Login,
    /// Remove the stored GitHub token.
    Logout,
    /// Review a diff: the current branch, an explicit range, a file, or a PR.
    Review(review::ReviewArgs),
    /// Hidden alias for bare `aster`, kept so existing `aster chat …` scripts work.
    #[command(hide = true)]
    Chat(chat::ChatArgs),
    /// Apply model-generated fixes for review findings (dry-run by default).
    Fix(fix::FixArgs),
    /// List or show saved chat sessions for this repo.
    Sessions(sessions::SessionsArgs),
    /// List or add durable memory (project facts and blocks).
    Memory(sessions::MemoryArgs),
    /// Show what the next turn would run with: provider, model, limits, wiring.
    Status,
    /// Install, list, and remove agent skills.
    Skills(skills::SkillsArgs),
    /// Install, list, and validate Agent Plugins packages.
    Plugins(plugins::PluginsArgs),
    /// Crawl or extract web pages as Markdown.
    Web(web::WebArgs),
    /// Inspect the MCP servers configured for this repo.
    Mcp(mcp::McpArgs),
    /// List the models the provider serves, and switch which one is used.
    Model(models::ModelArgs),
    /// List the endpoints Aster knows, and switch which one is used.
    Provider(provider::ProviderArgs),
    /// Older spelling of `aster model list`, kept for existing scripts.
    #[command(hide = true)]
    Models(models::ModelsArgs),
    /// Drive the agent remotely from a messaging channel (Telegram).
    Remote(remote::RemoteArgs),
}

#[derive(Clone, Copy, ValueEnum)]
enum OutputFormat {
    Text,
    Json,
}

/// Set once from the root `--json` flag, then read anywhere a command chooses
/// between its human and machine output.
static JSON: AtomicBool = AtomicBool::new(false);

/// True when the run was asked for machine-readable output.
pub fn json_mode() -> bool {
    JSON.load(Ordering::Relaxed)
}

/// Set once from the command's `--effort` flag; `None` leaves env and aster.yaml in charge.
static EFFORT: OnceLock<Option<Effort>> = OnceLock::new();

/// The `--effort` level this run was started with, if any.
pub fn effort_flag() -> Option<Effort> {
    EFFORT.get().copied().flatten()
}

#[tokio::main]
async fn main() -> Result<()> {
    let _ = dotenvy::dotenv();
    if let Some(global) = dirs::home_dir().map(|h| h.join(".aster/.env")) {
        let _ = dotenvy::from_path(&global);
    }

    let cli = Cli::parse();
    let json = cli.json || matches!(cli.format, Some(OutputFormat::Json));
    JSON.store(json, Ordering::Relaxed);

    // No subcommand means chat: the flattened root flags are the chat args.
    let command = cli.command.unwrap_or(Command::Chat(cli.chat));
    let _ = EFFORT.set(match &command {
        Command::Chat(a) => a.effort.effort,
        Command::Review(a) => a.effort.effort,
        Command::Fix(a) => a.effort.effort,
        _ => None,
    });
    // Full-screen TUI logs must go to a file, not stderr.
    let chat_tui = matches!(&command, Command::Chat(a) if a.is_interactive());
    // Interactive `aster sessions` can hand off into the chat TUI on enter.
    let sessions_tui = matches!(&command, Command::Sessions(a) if a.is_interactive());
    let tui_mode = matches!(&command, Command::Review(a) if a.tui) || chat_tui || sessions_tui;
    let stream_mode = matches!(&command, Command::Review(a) if a.stream)
        || matches!(&command, Command::Fix(_))
        || matches!(&command, Command::Chat(a) if !a.is_interactive());
    let telemetry = init_tracing(tui_mode, stream_mode);

    let result = match command {
        Command::Init(args) => init::run(args),
        Command::Login => auth::login().await,
        Command::Logout => config::clear_token(),
        Command::Review(args) => review::run(args).await,
        Command::Chat(args) => chat::run(args).await,
        Command::Fix(args) => fix::run(args).await,
        Command::Sessions(args) => sessions::run_sessions(args).await,
        Command::Memory(args) => sessions::run_memory(args),
        Command::Status => status::run(),
        Command::Skills(args) => skills::run(args).await,
        Command::Plugins(args) => plugins::run(args, std::env::current_dir().ok().as_deref()),
        Command::Web(args) => web::run(args).await,
        Command::Mcp(args) => mcp::run(args, std::env::current_dir().ok().as_deref()).await,
        Command::Model(args) => models::run_model(args).await,
        Command::Provider(args) => provider::run(args),
        Command::Models(args) => models::run(args).await,
        Command::Remote(args) => remote::run(args).await,
    };

    // Batched spans are still in memory at this point, including the ones a
    // failing run produced, which are the interesting ones.
    if let Some(telemetry) = &telemetry {
        telemetry.shutdown();
    }

    // In JSON mode a failure is data too, so callers parse one shape either way.
    match result {
        Err(e) if json_mode() => {
            println!(
                "{}",
                serde_json::json!({ "ok": false, "error": format!("{e:#}") })
            );
            process::exit(1);
        }
        other => other,
    }
}

/// The console layer keeps obeying `RUST_LOG`; spans go to OTLP on their own
/// filter, so turning export on never changes what is printed. Returns the
/// exporter handle, which must outlive the run to flush what it batched.
fn init_tracing(tui_mode: bool, stream_mode: bool) -> Option<aster_telemetry::Telemetry> {
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;
    use tracing_subscriber::{EnvFilter, Layer, Registry};

    let console = tracing_subscriber::fmt::layer().with_target(false);
    let console: Box<dyn Layer<Registry> + Send + Sync> =
        match fs::File::create(env::temp_dir().join("aster-tui.log")) {
            // A full-screen TUI cannot share the terminal with log lines.
            Ok(file) if tui_mode => {
                Box::new(console.with_ansi(false).with_writer(Mutex::new(file)))
            }
            _ if stream_mode => Box::new(console.with_writer(stderr)),
            _ => Box::new(console),
        };
    let console = console.with_filter(EnvFilter::new(
        env::var("RUST_LOG").unwrap_or_else(|_| "aster_harness=info".into()),
    ));

    let (export, telemetry) = match aster_telemetry::from_env::<Registry>("aster") {
        Ok(Some((layer, telemetry))) => (Some(layer), Some(telemetry)),
        Ok(None) => (None, None),
        // Bad endpoint, missing collector config: worth saying, not worth
        // refusing to start over.
        Err(e) => {
            eprintln!("telemetry disabled: {e:#}");
            (None, None)
        }
    };
    // Both layers filter against `Registry`, so they go on as one set rather
    // than stacking, which would type the second against the first.
    let mut layers: Vec<Box<dyn Layer<Registry> + Send + Sync>> = vec![console.boxed()];
    if let Some(export) = export {
        layers.push(
            export
                .with_filter(EnvFilter::new(
                    env::var("OTEL_FILTER").unwrap_or_else(|_| "info".into()),
                ))
                .boxed(),
        );
    }

    tracing_subscriber::registry().with(layers).init();
    telemetry
}
