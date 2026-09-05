//! Scheduling for Aster: `aster.yaml` schedules installed into the OS
//! scheduler (launchd on macOS, cron on Linux), plus native reminders.
//! The OS scheduler is the wheel we do not re-invent; there is no daemon here.

pub mod crontab;
pub mod launchd;
pub mod notify;
pub mod remind;
pub mod schedule;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
pub use schedule::{Schedule, matches_at, validate, validate_cron};

/// Where scheduled-run logs live: `<config>/aster/cron/<name>.log`.
pub fn log_dir() -> Result<PathBuf> {
    let home = dirs::home_dir().context("no home directory")?;
    Ok(home.join(".aster").join("cron"))
}

/// The argv a schedule's plist or crontab entry runs, relative to `repo_root`.
pub fn program_args(aster_bin: &Path, sched: &Schedule, repo_root: &Path) -> Vec<String> {
    let mut args = vec![
        aster_bin.to_string_lossy().into_owned(),
        "run".to_string(),
        sched.agent.clone(),
        sched.task.clone(),
        "--json".to_string(),
        "--schedule".to_string(),
        sched.name.clone(),
    ];
    if sched.notify {
        args.push("--notify".to_string());
    }
    args.push("--cwd".to_string());
    args.push(repo_root.to_string_lossy().into_owned());
    args
}

/// Install every schedule into the OS scheduler for the current platform.
pub fn install_all(schedules: &[Schedule], aster_bin: &Path, repo_root: &Path) -> Result<()> {
    validate(schedules)?;
    let logs = log_dir()?;
    std::fs::create_dir_all(&logs).with_context(|| format!("creating {}", logs.display()))?;
    for sched in schedules {
        install_one(sched, aster_bin, repo_root, &logs)?;
    }
    Ok(())
}

fn install_one(sched: &Schedule, aster_bin: &Path, repo_root: &Path, logs: &Path) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        let log_path = logs.join(format!("{}.log", sched.name));
        let intervals = schedule::calendar_intervals(&sched.cron)?;
        launchd::install(
            &sched.name,
            &intervals,
            &program_args(aster_bin, sched, repo_root),
            repo_root,
            &log_path,
        )?;
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = repo_root;
        let _ = logs;
        crontab::install(
            &sched.name,
            &sched.cron,
            &program_args(aster_bin, sched, repo_root).join(" "),
        )?;
    }
    Ok(())
}

/// Remove one schedule from the OS scheduler.
pub fn remove(name: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        launchd::remove(name)
    }
    #[cfg(not(target_os = "macos"))]
    {
        crontab::remove(name)
    }
}

/// Whether `name` is installed on the current platform.
pub fn is_installed(name: &str) -> bool {
    #[cfg(target_os = "macos")]
    {
        launchd::is_installed(name)
    }
    #[cfg(not(target_os = "macos"))]
    {
        crontab::is_installed(name)
    }
}

#[cfg(test)]
#[path = "tests/lib_test.rs"]
mod tests;
