//! `aster bots`: install and inspect bot packages. A bot is a published
//! specialist, its instructions plus its own skills, routines, and the things
//! it assumes exist. It contributes an agent the registry dispatches like any
//! other, so `bot` is to `agent` what `plugin` is to `skill`.

mod check;
mod grok;
mod ir;
mod write;

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand};
use console::style;

use check::Status;
use write::BotRecord;

pub(crate) const BOTS_DIR: &str = "bots";

#[derive(Args)]
pub struct BotsArgs {
    #[command(subcommand)]
    command: Option<BotsCommand>,
}

#[derive(Subcommand)]
enum BotsCommand {
    /// Install a bot from a share JSON file.
    #[command(visible_alias = "a")]
    Add {
        /// A Grok Bot share JSON path. A marketplace URL is not read yet.
        source: String,
        /// Install into this project (`.aster/bots`) instead of the user-global root.
        #[arg(short = 'p', long)]
        project: bool,
        /// Install into the user-global root. The default; accepted for symmetry.
        #[arg(short = 'g', long, conflicts_with = "project")]
        global: bool,
        /// Tools the bot's agent may call. Defaults to the read-only set.
        #[arg(long, value_delimiter = ',')]
        tools: Vec<String>,
        /// Replace a bot of the same name.
        #[arg(long)]
        force: bool,
    },
    /// List installed bots and what each contributes.
    #[command(visible_alias = "ls")]
    List,
    /// Show one bot: its skills, routines, and what it still needs.
    Show {
        /// The bot's name, as `aster bots list` prints it.
        name: String,
    },
    /// Remove an installed bot.
    #[command(visible_alias = "rm")]
    Remove {
        /// The bot's name.
        name: String,
    },
}

/// Every bots root, project first so a project bot shadows a global one.
pub(crate) fn roots(repo_root: Option<&Path>) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(root) = repo_root {
        roots.push(root.join(".aster").join(BOTS_DIR));
    }
    if let Ok(home) = crate::persist::home() {
        roots.push(home.join(BOTS_DIR));
    }
    roots
}

pub fn run(args: BotsArgs, repo_root: Option<&Path>) -> Result<()> {
    match args.command.unwrap_or(BotsCommand::List) {
        BotsCommand::Add {
            source,
            project,
            global: _,
            tools,
            force,
        } => add(repo_root, &source, project, tools, force),
        BotsCommand::List => list(repo_root),
        BotsCommand::Show { name } => show(repo_root, &name),
        BotsCommand::Remove { name } => remove(repo_root, &name),
    }
}

fn install_root(repo_root: Option<&Path>, project: bool) -> Result<PathBuf> {
    let base = match project {
        true => match repo_root {
            Some(root) => root.join(".aster"),
            None => std::env::current_dir()
                .context("could not determine the current directory")?
                .join(".aster"),
        },
        false => crate::persist::home()?,
    };
    Ok(base.join(BOTS_DIR))
}

fn default_tools() -> Vec<String> {
    aster_agents::DEFAULT_TOOLS
        .iter()
        .map(|t| (*t).to_string())
        .collect()
}

fn add(
    repo_root: Option<&Path>,
    source: &str,
    project: bool,
    tools: Vec<String>,
    force: bool,
) -> Result<()> {
    if source.starts_with("http://") || source.starts_with("https://") {
        bail!(
            "reading a bot from a URL is not implemented yet. Export the bot as share JSON and \
             pass the file: `aster bots add ./bot.json`"
        );
    }
    let path = Path::new(source);
    if !path.is_file() {
        bail!("{source} is not a file. Pass a Grok Bot share JSON.");
    }
    let raw = fs::read_to_string(path).with_context(|| format!("reading {source}"))?;
    let ir = grok::read(&raw, source)?;

    let tools = match tools.is_empty() {
        true => default_tools(),
        false => tools,
    };
    let root = install_root(repo_root, project)?;
    let written = write::install(&root, &ir, &raw, &tools, force)?;

    let settings = crate::settings::Settings::load(repo_root)?;
    let checked = check::check(&ir, &settings.mcp.servers);

    println!(
        "added {} ({})",
        style(&written.name).green().bold(),
        ir.origin.format
    );
    println!("  {}", written.dir.display());
    println!();
    report(&ir, written.skills, &checked);
    println!();
    println!("nothing scheduled, nothing authenticated.");
    println!(
        "  aster bots show {}{}",
        written.name,
        pad(&written.name, "    what it still needs")
    );
    println!(
        "  aster run {} \"...\"{}",
        written.name,
        pad(&written.name, "    try it")
    );
    Ok(())
}

fn pad(name: &str, tail: &str) -> String {
    let width = 24usize.saturating_sub(name.len());
    format!("{:width$}{tail}", "")
}

/// The counts are the point: "1 of 1" and "0 of 3" are different imports, and
/// nobody should have to open a directory to tell which one they got.
fn report(ir: &ir::BotIr, installed_skills: usize, checked: &[check::Checked]) {
    let declared = installed_skills
        + ir.missing
            .iter()
            .filter(|m| m.starts_with("skill body"))
            .count();
    println!(
        "  instructions   {}",
        match ir.instructions.trim().is_empty() {
            true => style("missing").red().to_string(),
            false => style("ok").green().to_string(),
        }
    );
    println!(
        "  skills         {installed_skills} of {declared}{}",
        match installed_skills {
            0 => String::new(),
            _ => format!(
                ": {}",
                ir.skills
                    .iter()
                    .map(|s| ir::slug(&s.name))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    );

    let cron: Vec<String> = ir
        .cron_routines()
        .map(|(r, expr)| format!("{} ({expr})", r.name))
        .collect();
    let other = ir.non_cron_routines().count();
    let routines = match (cron.len(), other) {
        (0, 0) => "none".to_string(),
        (0, n) => format!("{n} not schedulable"),
        (_, 0) => format!(
            "{} cron: {}, inert until you add them",
            cron.len(),
            cron.join(", ")
        ),
        (_, n) => format!(
            "{} cron: {}, inert until you add them; {n} not schedulable",
            cron.len(),
            cron.join(", ")
        ),
    };
    println!("  routines       {routines}");

    let needs = check::count(checked, Status::NeedsSetup);
    let unsupported = check::count(checked, Status::Unsupported);
    let available = check::count(checked, Status::Available);
    let mut parts = Vec::new();
    if available > 0 {
        parts.push(format!("{available} available"));
    }
    if needs > 0 {
        parts.push(style(format!("{needs} needs setup")).yellow().to_string());
    }
    if unsupported > 0 {
        parts.push(
            style(format!("{unsupported} unsupported"))
                .red()
                .to_string(),
        );
    }
    println!(
        "  requirements   {}",
        match parts.is_empty() {
            true => "none".to_string(),
            false => parts.join(", "),
        }
    );
    if !ir.notes.is_empty() {
        println!(
            "  notes          {} kept as the publisher's, never your memory",
            ir.notes.len()
        );
    }
}

struct Installed {
    name: String,
    dir: PathBuf,
    record: BotRecord,
}

fn installed(repo_root: Option<&Path>) -> Vec<Installed> {
    let mut out: Vec<Installed> = Vec::new();
    for root in roots(repo_root) {
        let Ok(entries) = fs::read_dir(&root) else {
            continue;
        };
        for entry in entries.filter_map(|e| e.ok()) {
            let dir = entry.path();
            if !dir.join(write::RECORD_FILE).is_file() {
                continue;
            }
            let name = dir
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            if out.iter().any(|b| b.name == name) {
                continue;
            }
            match BotRecord::load(&dir) {
                Ok(record) => out.push(Installed { name, dir, record }),
                Err(e) => tracing::warn!(path = %dir.display(), "skipping bot: {e:#}"),
            }
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

fn list(repo_root: Option<&Path>) -> Result<()> {
    let bots = installed(repo_root);
    if bots.is_empty() {
        println!("no bots installed. `aster bots add ./bot.json` installs one.");
        return Ok(());
    }
    for bot in &bots {
        let ir = &bot.record.bot;
        println!("{}", style(&bot.name).bold());
        println!(
            "  agent {}, {} skill(s), {} routine(s), {} requirement(s)",
            bot.name,
            ir.skills.len(),
            ir.routines.len(),
            ir.requirements.len()
        );
        println!("  from {} ({})", ir.origin.source, ir.origin.format);
    }
    Ok(())
}

fn find(repo_root: Option<&Path>, name: &str) -> Result<Installed> {
    installed(repo_root)
        .into_iter()
        .find(|b| b.name == name)
        .with_context(|| {
            format!("no bot named {name:?}. `aster bots list` shows what is installed")
        })
}

fn show(repo_root: Option<&Path>, name: &str) -> Result<()> {
    let bot = find(repo_root, name)?;
    let ir = &bot.record.bot;
    let settings = crate::settings::Settings::load(repo_root)?;
    let checked = check::check(ir, &settings.mcp.servers);

    println!("{}", style(&bot.name).bold());
    println!("  {}", bot.dir.display());
    println!("  from {} ({})", ir.origin.source, ir.origin.format);
    println!("  tools {}", bot.record.tools.join(", "));
    println!();
    report(ir, ir.skills.len(), &checked);

    if !checked.is_empty() {
        println!();
        println!("requirements");
        for c in &checked {
            let label = match c.status {
                Status::Available => style(c.status.label()).green(),
                Status::NeedsSetup => style(c.status.label()).yellow(),
                Status::Unsupported => style(c.status.label()).red(),
            };
            println!("  {:<14} {label}: {}", c.id, c.detail);
        }
    }

    if !ir.missing.is_empty() {
        println!();
        println!("did not come across");
        for gap in &ir.missing {
            println!("  {gap}");
        }
    }

    let cron: Vec<_> = ir.cron_routines().collect();
    if !cron.is_empty() {
        println!();
        println!("routines, to add under `schedules:` in aster.yaml when you want them:");
        println!();
        for (routine, expr) in cron {
            println!("  - name: {}", ir::slug(&routine.name));
            println!("    cron: \"{expr}\"");
            println!("    agent: {}", bot.name);
            println!("    task: {}", yaml_block(&routine.task));
        }
        println!();
        println!("then `aster cron install`. Nothing runs until you do.");
    }
    Ok(())
}

/// Tasks are prose and routinely multi-line, so they go in as a literal block.
fn yaml_block(task: &str) -> String {
    let lines: Vec<&str> = task.lines().collect();
    match lines.len() {
        0 => "\"\"".to_string(),
        1 => format!(
            "\"{}\"",
            lines[0].replace('\\', "\\\\").replace('"', "\\\"")
        ),
        _ => {
            let body = lines
                .iter()
                .map(|l| format!("\n      {l}"))
                .collect::<String>();
            format!("|-{body}")
        }
    }
}

fn remove(repo_root: Option<&Path>, name: &str) -> Result<()> {
    let bot = find(repo_root, name)?;
    fs::remove_dir_all(&bot.dir).with_context(|| format!("removing {}", bot.dir.display()))?;
    println!("removed {} from {}", style(name).bold(), bot.dir.display());
    Ok(())
}

#[cfg(test)]
#[path = "../tests/bots_test.rs"]
mod tests;
