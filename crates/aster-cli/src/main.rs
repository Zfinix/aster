#![forbid(unsafe_code)]

mod auth;
mod chat;
mod config;
mod edits;
mod fix;
mod git;
mod github;
mod init;
mod persist;
mod provider;
mod review;
mod sessions;
mod settings;
mod skills;
mod tui;

use std::env;
use std::fs;
use std::io::stderr;
use std::sync::Mutex;

use anyhow::Result;
use clap::Parser;
use clap::Subcommand;

#[derive(Parser)]
#[command(
    name = "aster",
    version,
    about = "AI code review: hypothesize → verify → shape"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
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
    /// Chat with the review agent (interactive TUI by default; --print for one-shot).
    Chat(chat::ChatArgs),
    /// Apply model-generated fixes for review findings (dry-run by default).
    Fix(fix::FixArgs),
    /// List or show saved chat sessions for this repo.
    Sessions(sessions::SessionsArgs),
    /// List or add durable memory (project facts and blocks).
    Memory(sessions::MemoryArgs),
    /// Install, list, and remove agent skills.
    Skills(skills::SkillsArgs),
}

#[tokio::main]
async fn main() -> Result<()> {
    let _ = dotenvy::dotenv();
    if let Some(global) = dirs::config_dir().map(|d| d.join("aster/.env")) {
        let _ = dotenvy::from_path(&global);
    }

    let command = Cli::parse().command;
    // Full-screen TUI logs must go to a file, not stderr.
    let chat_tui = matches!(&command, Command::Chat(a) if a.is_interactive());
    let tui_mode = matches!(&command, Command::Review(a) if a.tui) || chat_tui;
    let stream_mode = matches!(&command, Command::Review(a) if a.stream)
        || matches!(&command, Command::Fix(_))
        || matches!(&command, Command::Chat(a) if !a.is_interactive());
    init_tracing(tui_mode, stream_mode);

    match command {
        Command::Init(args) => init::run(args),
        Command::Login => auth::login().await,
        Command::Logout => config::clear_token(),
        Command::Review(args) => review::run(args).await,
        Command::Chat(args) => chat::run(args).await,
        Command::Fix(args) => fix::run(args).await,
        Command::Sessions(args) => sessions::run_sessions(args),
        Command::Memory(args) => sessions::run_memory(args),
        Command::Skills(args) => skills::run(args).await,
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
