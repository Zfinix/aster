//! `aster cron`: install, list, remove, and test schedules from aster.yaml.

use anyhow::{Context, Result};
use clap::Args;

#[derive(Debug, Args)]
pub(crate) struct CronArgs {
    #[command(subcommand)]
    pub command: CronCommand,
}

#[derive(Debug, clap::Subcommand)]
pub(crate) enum CronCommand {
    /// Install every schedule in aster.yaml into the OS scheduler.
    Install,
    /// Show schedules and whether each is installed.
    List,
    /// Remove one schedule from the OS scheduler.
    Remove { name: String },
    /// Run one schedule right now, for testing.
    Run { name: String },
}

pub(crate) fn run(args: CronArgs) -> Result<()> {
    let repo_root = std::env::current_dir().context("could not determine the current directory")?;
    let settings = crate::settings::Settings::load(Some(&repo_root))?;
    match args.command {
        CronCommand::Install => {
            let bin = current_bin()?;
            aster_cron::install_all(&settings.schedules, &bin, &repo_root)?;
            println!("installed {} schedule(s)", settings.schedules.len());
        }
        CronCommand::List => {
            if settings.schedules.is_empty() {
                println!("no schedules in aster.yaml");
                return Ok(());
            }
            for s in &settings.schedules {
                let state = if aster_cron::is_installed(&s.name) {
                    "installed"
                } else {
                    "not installed"
                };
                println!("{:<24} {:<12} {:<10} {}", s.name, s.cron, s.agent, state);
            }
        }
        CronCommand::Remove { name } => {
            aster_cron::remove(&name)?;
            println!("removed {name}");
        }
        CronCommand::Run { name } => {
            let sched = settings
                .schedules
                .iter()
                .find(|s| s.name == name)
                .with_context(|| format!("no schedule named {name:?} in aster.yaml"))?;
            let args = crate::run::RunArgs {
                agent: sched.agent.clone(),
                task: sched.task.clone(),
                json: crate::json_mode(),
                schedule: Some(sched.name.clone()),
                notify: sched.notify,
                cwd: Some(repo_root.clone()),
            };
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(crate::run::run(args))?;
        }
    }
    Ok(())
}

fn current_bin() -> Result<std::path::PathBuf> {
    std::env::current_exe().context("could not locate the aster binary")
}
