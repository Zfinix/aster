#![forbid(unsafe_code)]

mod auth;
mod budget;
mod chat;
mod config;
mod edits;
mod fix;
mod git;
mod github;
mod init;
mod instructions;
mod mcp;
mod persist;
mod provider;
mod review;
mod sessions;
mod settings;
mod skills;
mod term;
mod test_runner;
mod tui;
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

#[derive(Parser)]
#[command(
    name = "aster",
    version,
    about = "A self-hostable agent harness for software work"
)]
struct Cli {
    /// Defaults to `chat`: bare `aster` opens the interactive TUI.
    #[command(subcommand)]
    command: Option<Command>,

    /// Emit JSON on stdout instead of human text. Accepted by every subcommand,
    /// before or after it, and turns errors into `{"ok":false,"error":…}`.
    #[arg(long, global = true)]
    json: bool,

    /// Reasoning budget for thinking models: off, low, medium, or high.
    /// Overrides ASTER_EFFORT and aster.yaml `review.effort`.
    #[arg(long, global = true, value_name = "LEVEL")]
    effort: Option<Effort>,
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
    /// Chat with the Aster agent (interactive TUI by default; --print for one-shot).
    Chat(chat::ChatArgs),
    /// Apply model-generated fixes for review findings (dry-run by default).
    Fix(fix::FixArgs),
    /// List or show saved chat sessions for this repo.
    Sessions(sessions::SessionsArgs),
    /// List or add durable memory (project facts and blocks).
    Memory(sessions::MemoryArgs),
    /// Install, list, and remove agent skills.
    Skills(skills::SkillsArgs),
    /// Crawl or extract web pages as Markdown.
    Web(web::WebArgs),
    /// Inspect the MCP servers configured for this repo.
    Mcp(mcp::McpArgs),
}

/// Set once from the root `--json` flag, then read anywhere a command chooses
/// between its human and machine output.
static JSON: AtomicBool = AtomicBool::new(false);

/// True when the run was asked for machine-readable output.
pub fn json_mode() -> bool {
    JSON.load(Ordering::Relaxed)
}

/// Set once from the root `--effort` flag; `None` leaves env and aster.yaml in charge.
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
    JSON.store(cli.json, Ordering::Relaxed);
    let _ = EFFORT.set(cli.effort);

    // Bare `aster` is the interactive chat TUI; `aster --json` is one-shot chat.
    let command = cli.command.unwrap_or_else(|| {
        let argv: &[&str] = if cli.json {
            &["aster", "chat"]
        } else {
            &["aster", "chat", "--tui"]
        };
        Cli::parse_from(argv).command.expect("chat is a subcommand")
    });
    // Full-screen TUI logs must go to a file, not stderr.
    let chat_tui = matches!(&command, Command::Chat(a) if a.is_interactive());
    let tui_mode = matches!(&command, Command::Review(a) if a.tui) || chat_tui;
    let stream_mode = matches!(&command, Command::Review(a) if a.stream)
        || matches!(&command, Command::Fix(_))
        || matches!(&command, Command::Chat(a) if !a.is_interactive());
    init_tracing(tui_mode, stream_mode);

    let result = match command {
        Command::Init(args) => init::run(args),
        Command::Login => auth::login().await,
        Command::Logout => config::clear_token(),
        Command::Review(args) => review::run(args).await,
        Command::Chat(args) => chat::run(args).await,
        Command::Fix(args) => fix::run(args).await,
        Command::Sessions(args) => sessions::run_sessions(args),
        Command::Memory(args) => sessions::run_memory(args),
        Command::Skills(args) => skills::run(args).await,
        Command::Web(args) => web::run(args).await,
        Command::Mcp(args) => mcp::run(args, std::env::current_dir().ok().as_deref()).await,
    };

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

fn init_tracing(tui_mode: bool, stream_mode: bool) {
    let filter = env::var("RUST_LOG").unwrap_or_else(|_| "aster_harness=info".into());
    let builder = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false);
    if tui_mode && let Ok(file) = fs::File::create(env::temp_dir().join("aster-tui.log")) {
        builder
            .with_ansi(false)
            .with_writer(Mutex::new(file))
            .init();
        return;
    }
    if stream_mode {
        builder.with_writer(stderr).init();
        return;
    }
    builder.init();
}
