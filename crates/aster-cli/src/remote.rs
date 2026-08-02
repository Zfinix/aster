//! `aster remote` — drive the agent from messaging channels.

use std::env;

use anyhow::{Context, Result};
use clap::{Args, Subcommand};

#[derive(Args)]
pub struct RemoteArgs {
    #[command(subcommand)]
    channel: Channel,
}

#[derive(Subcommand)]
enum Channel {
    /// Bridge a Telegram bot to the agent (long-polling, no public URL needed).
    Telegram(TelegramArgs),
    /// Internal: MCP server with Telegram chat tools, spawned per bridge turn.
    #[command(hide = true, name = "mcp-telegram")]
    McpTelegram,
}

#[derive(Args)]
struct TelegramArgs {
    /// Bot token from @BotFather; defaults to ASTER_TELEGRAM_TOKEN.
    #[arg(long, value_name = "TOKEN")]
    token: Option<String>,

    /// Telegram user id allowed to drive the agent (repeatable).
    /// Defaults to ASTER_REMOTE_USERS, a comma-separated list.
    #[arg(long = "user", value_name = "ID")]
    users: Vec<i64>,

    /// Permission mode for remote turns; prompts arrive as buttons in the chat.
    #[arg(long, value_name = "MODE", default_value = "manual",
          value_parser = ["plan", "manual", "auto", "edit", "yolo"])]
    mode: String,
}

pub async fn run(args: RemoteArgs) -> Result<()> {
    match args.channel {
        Channel::Telegram(args) => telegram(args).await,
        Channel::McpTelegram => aster_remote::run_mcp_telegram().await,
    }
}

async fn telegram(args: TelegramArgs) -> Result<()> {
    let token = args
        .token
        .or_else(|| env::var("ASTER_TELEGRAM_TOKEN").ok())
        .context("no bot token; pass --token or set ASTER_TELEGRAM_TOKEN")?;
    let mut users = args.users;
    if users.is_empty()
        && let Ok(raw) = env::var("ASTER_REMOTE_USERS")
    {
        users = raw
            .split(',')
            .filter_map(|id| id.trim().parse().ok())
            .collect();
    }
    let config = aster_remote::TelegramConfig {
        token,
        allowed_users: users,
        bin: env::current_exe().context("resolving the aster binary path")?,
        repo_root: env::current_dir().context("could not determine the current directory")?,
        mode: args.mode,
    };
    aster_remote::run_telegram(config).await
}
