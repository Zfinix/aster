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
    /// Bridge iMessage, WhatsApp, and Telegram through Photon's hosted lines
    /// (polling, no public URL needed).
    Spectrum(SpectrumArgs),
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

#[derive(Args)]
struct SpectrumArgs {
    /// Photon project API key (pho_sk_...); defaults to ASTER_PHOTON_KEY, which
    /// keeps it out of the process list.
    #[arg(long, value_name = "KEY")]
    key: Option<String>,

    /// Photon project id (pho_prj_...); defaults to ASTER_PHOTON_PROJECT.
    #[arg(long, value_name = "ID")]
    project: Option<String>,

    /// Platform to answer on (repeatable); iMessage, WhatsApp, and Telegram
    /// when omitted.
    #[arg(long = "platform", value_name = "PLATFORM",
          value_parser = ["imessage", "whatsapp", "whatsapp_business", "telegram", "sms"])]
    platforms: Vec<String>,

    /// Sender handle allowed to drive the agent (repeatable): a phone number,
    /// an iMessage email, or a Telegram user id, optionally scoped as
    /// `imessage:+15551234567`. Defaults to ASTER_REMOTE_USERS, a
    /// comma-separated list.
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
        Channel::Spectrum(args) => spectrum(args).await,
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

/// The senders from the flags, or else from ASTER_REMOTE_USERS.
fn senders_or_env(senders: Vec<String>) -> Vec<String> {
    if !senders.is_empty() {
        return senders;
    }
    env::var("ASTER_REMOTE_USERS")
        .map(|raw| {
            raw.split(',')
                .filter_map(|s| {
                    let s = s.trim();
                    (!s.is_empty()).then(|| s.to_string())
                })
                .collect()
        })
        .unwrap_or_default()
}

async fn imessage(args: IMessageArgs) -> Result<()> {
    let senders = senders_or_env(args.senders);
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
    let senders = senders_or_env(args.senders);
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

async fn spectrum(args: SpectrumArgs) -> Result<()> {
    let api_key = args
        .key
        .or_else(|| env::var("ASTER_PHOTON_KEY").ok())
        .filter(|key| !key.trim().is_empty())
        .context("no Photon API key; pass --key or set ASTER_PHOTON_KEY")?;
    let project_id = args
        .project
        .or_else(|| env::var("ASTER_PHOTON_PROJECT").ok())
        .filter(|project| !project.trim().is_empty())
        .context("no Photon project; pass --project or set ASTER_PHOTON_PROJECT")?;
    let config = aster_remote::SpectrumConfig {
        api_key: api_key.trim().to_string(),
        project_id: project_id.trim().to_string(),
        api_base: env::var("ASTER_PHOTON_API")
            .ok()
            .filter(|base| !base.trim().is_empty())
            .unwrap_or_else(|| aster_remote::SPECTRUM_API.to_string()),
        platforms: args.platforms,
        allowed_senders: senders_or_env(args.senders),
        bin: env::current_exe().context("resolving the aster binary path")?,
        repo_root: env::current_dir().context("could not determine the current directory")?,
        mode: args.mode,
    };
    aster_remote::run_spectrum(config).await
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
