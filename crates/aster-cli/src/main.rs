#![forbid(unsafe_code)]

mod auth;
mod chat;
mod config;
mod edits;
mod fix;
mod git;
mod github;
mod provider;
mod review;
mod settings;
mod tui;

use std::{env, fs};

use anyhow::Result;
use clap::{Parser, Subcommand};

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
    /// Link a GitHub account via device flow (opens your browser).
    Login,
    /// Remove the stored GitHub token.
    Logout,
    /// Review a diff: the current branch, an explicit range, a file, or a PR.
    Review(review::ReviewArgs),
    /// Ask Aster a question (one-shot chat with the review-agent persona).
    Chat(chat::ChatArgs),
    /// Apply model-generated fixes for review findings (dry-run by default).
    Fix(fix::FixArgs),
}

#[tokio::main]
async fn main() -> Result<()> {
    let _ = dotenvy::dotenv();
    let command = Cli::parse().command;

    let tui_mode = matches!(&command, Command::Review(a) if a.tui);
    let stream_mode = matches!(&command, Command::Review(a) if a.stream)
        || matches!(&command, Command::Chat(_) | Command::Fix(_));
    init_tracing(tui_mode, stream_mode);

    match command {
        Command::Login => auth::login().await,
        Command::Logout => config::clear_token(),
        Command::Review(args) => review::run(args).await,
        Command::Chat(args) => chat::run(args).await,
        Command::Fix(args) => fix::run(args).await,
    }
}

fn init_tracing(tui_mode: bool, stream_mode: bool) {
    let filter = env::var("RUST_LOG").unwrap_or_else(|_| "aster_harness=info".into());
    let builder = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false);
    if tui_mode {
        // Logs still exist for debugging, just off-screen.
        if let Ok(file) = fs::File::create(env::temp_dir().join("aster-tui.log")) {
            builder
                .with_ansi(false)
                .with_writer(std::sync::Mutex::new(file))
                .init();
            return;
        }
    }
    if stream_mode {
        builder.with_writer(std::io::stderr).init();
        return;
    }
    builder.init();
}
