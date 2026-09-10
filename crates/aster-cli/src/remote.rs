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
    /// Bridge iMessage through a Photon agent server (signed webhooks).
    Photon(PhotonArgs),
    /// Bridge iMessage natively via Messages.app (free, macOS only).
    #[command(name = "imessage", alias = "i-message")]
    IMessage(IMessageArgs),
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

#[derive(Args)]
struct PhotonArgs {
    /// Webhook signing secret from the Photon dashboard; defaults to ASTER_PHOTON_SECRET.
    #[arg(long, value_name = "SECRET")]
    secret: Option<String>,

    /// Port the webhook listener binds on 127.0.0.1.
    #[arg(long, value_name = "PORT", default_value_t = 8799)]
    port: u16,

    /// iMessage sender (handle or phone) allowed to drive the agent (repeatable).
    /// Defaults to ASTER_REMOTE_USERS, a comma-separated list.
    #[arg(long = "sender", value_name = "SENDER")]
    senders: Vec<String>,

    /// Permission mode for remote turns; prompts arrive as plain questions.
    #[arg(long, value_name = "MODE", default_value = "manual",
          value_parser = ["plan", "manual", "auto", "edit", "yolo"])]
    mode: String,
}

pub async fn run(args: RemoteArgs) -> Result<()> {
    match args.channel {
        Channel::Telegram(args) => telegram(args).await,
        Channel::Photon(args) => photon(args).await,
        Channel::IMessage(args) => imessage(args).await,
        Channel::McpTelegram => aster_remote::run_mcp_telegram().await,
    }
}

#[derive(Args)]
struct IMessageArgs {
    /// iMessage sender (handle or phone) allowed to drive the agent (repeatable).
    /// Defaults to ASTER_REMOTE_USERS, a comma-separated list.
    #[arg(long = "sender", value_name = "SENDER")]
    senders: Vec<String>,

    /// Permission mode for remote turns; prompts arrive as plain questions.
    #[arg(long, value_name = "MODE", default_value = "manual",
          value_parser = ["plan", "manual", "auto", "edit", "yolo"])]
    mode: String,
}

async fn imessage(args: IMessageArgs) -> Result<()> {
    let mut senders = args.senders;
    if senders.is_empty()
        && let Ok(raw) = env::var("ASTER_REMOTE_USERS")
    {
        senders = raw
            .split(',')
            .filter_map(|s| {
                let s = s.trim();
                (!s.is_empty()).then(|| s.to_string())
            })
            .collect();
    }
    let db_path = env::var("HOME")
        .map(|h| std::path::PathBuf::from(h).join("Library/Messages/chat.db"))
        .context("could not determine HOME")?;
    let config = aster_remote::IMessageConfig {
        allowed_senders: senders,
        db_path,
        bin: env::current_exe().context("resolving the aster binary path")?,
        repo_root: env::current_dir().context("could not determine the current directory")?,
        mode: args.mode,
    };
    aster_remote::run_imessage(config).await
}

async fn photon(args: PhotonArgs) -> Result<()> {
    let secret = args
        .secret
        .or_else(|| env::var("ASTER_PHOTON_SECRET").ok())
        .context("no webhook secret; pass --secret or set ASTER_PHOTON_SECRET")?;
    let mut senders = args.senders;
    if senders.is_empty()
        && let Ok(raw) = env::var("ASTER_REMOTE_USERS")
    {
        senders = raw
            .split(',')
            .filter_map(|s| {
                let s = s.trim();
                (!s.is_empty()).then(|| s.to_string())
            })
            .collect();
    }
    let config = aster_remote::PhotonConfig {
        secret,
        port: args.port,
        allowed_senders: senders,
        bin: env::current_exe().context("resolving the aster binary path")?,
        repo_root: env::current_dir().context("could not determine the current directory")?,
        mode: args.mode,
    };
    aster_remote::run_photon(config).await
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
