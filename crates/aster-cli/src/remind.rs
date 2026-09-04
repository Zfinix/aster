//! `aster remind`: a one-shot native notification, installed into the OS
//! scheduler and removed by its own fire command.

use anyhow::{Context, Result};
use clap::Args;

#[derive(Debug, Args)]
pub(crate) struct RemindArgs {
    /// What the notification says.
    pub text: Option<String>,
    /// When to fire: "in 30m", "in 2h", or "at 18:00".
    pub when: Option<String>,

    /// Internal: fired by the scheduled entry; posts the notification and
    /// removes the reminder so it fires exactly once.
    #[arg(long, hide = true)]
    pub fire: Option<String>,
    /// Internal: the reminder text carried through the fire command.
    #[arg(long, hide = true)]
    pub text_override: Option<String>,
}

pub(crate) fn run(args: RemindArgs) -> Result<()> {
    if let Some(id) = &args.fire {
        aster_cron::remove(id)?;
        let text = args.text_override.unwrap_or_else(|| "reminder".to_string());
        aster_cron::notify::send("aster reminder", &text)?;
        return Ok(());
    }

    let Some(when) = &args.when else {
        anyhow::bail!("usage: aster remind \"<text>\" \"in 30m\" | \"at 18:00\"");
    };
    let Some(text) = &args.text else {
        anyhow::bail!("usage: aster remind \"<text>\" \"in 30m\" | \"at 18:00\"");
    };
    let fire_at = aster_cron::remind::parse_when(when)?;
    let id = format!(
        "remind-{}",
        ulid::Ulid::new().to_string().to_ascii_lowercase()
    );
    let bin = std::env::current_exe().context("could not locate the aster binary")?;
    let fire_args = aster_cron::remind::fire_args(&bin.to_string_lossy(), &id, text);
    let cron = aster_cron::remind::one_shot_cron(fire_at);

    #[cfg(target_os = "macos")]
    {
        let intervals = aster_cron::schedule::calendar_intervals(&cron)?;
        let log = aster_cron::log_dir()?.join(format!("{id}.log"));
        aster_cron::launchd::install(&id, &intervals, &fire_args, &std::env::current_dir()?, &log)?;
    }
    #[cfg(not(target_os = "macos"))]
    {
        aster_cron::crontab::install(&id, &cron, &fire_args.join(" "))?;
    }

    println!("reminder set for {} ({id})", fire_at.format("%H:%M"));
    Ok(())
}
