//! `aster-eval [sessions-dir] [--since DAYS] [--model NAME] [--json]
//! [--baseline FILE]`. Defaults to every session under `~/.local/share/aster/sessions`.

use std::path::PathBuf;

use anyhow::{Context, Result};
use aster_eval::{Filter, Report, analyze, render, render_comparison};

fn main() -> Result<()> {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    if raw.first().is_some_and(|a| a == "live") {
        return live(raw[1..].to_vec());
    }
    let mut args = raw.into_iter();
    let mut dir: Option<PathBuf> = None;
    let mut filter = Filter::default();
    let mut json = false;
    let mut baseline: Option<PathBuf> = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--since" => {
                let days = args.next().context("--since needs a number of days")?;
                filter = Filter::since_days(days.parse().context("--since takes whole days")?);
            }
            "--model" => filter.model = Some(args.next().context("--model needs a name")?),
            "--baseline" => baseline = Some(args.next().context("--baseline needs a path")?.into()),
            "--json" => json = true,
            "-h" | "--help" => {
                println!("{}", HELP);
                return Ok(());
            }
            other if other.starts_with('-') => anyhow::bail!("unknown flag {other}"),
            other => dir = Some(other.into()),
        }
    }

    let dir = match dir {
        Some(dir) => dir,
        None => aster_persist::default_home()?.join("sessions"),
    };
    let report = analyze(&dir, &filter)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    print!("{}", render(&report));
    if let Some(path) = baseline {
        let earlier: Report = serde_json::from_str(
            &std::fs::read_to_string(&path)
                .with_context(|| format!("reading baseline {}", path.display()))?,
        )
        .context("baseline is not an aster-eval report")?;
        print!("{}", render_comparison(&report.compare(&earlier)));
    }
    Ok(())
}

fn live(args: Vec<String>) -> Result<()> {
    let mut models: Vec<String> = Vec::new();
    let mut target = aster_eval::Target {
        bin: "aster".into(),
        repo: aster_eval::repo_root(&std::env::current_dir()?),
    };
    let mut runs = 1;
    let mut args = args.into_iter();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--models" => {
                models = args
                    .next()
                    .context("--models needs a comma-separated list")?
                    .split(',')
                    .map(str::trim)
                    .filter(|m| !m.is_empty())
                    .map(str::to_string)
                    .collect();
            }
            "--runs" => {
                runs = args
                    .next()
                    .context("--runs needs a number")?
                    .parse()
                    .context("--runs takes a whole number")?;
            }
            "--bin" => target.bin = args.next().context("--bin needs a path")?.into(),
            "--repo" => target.repo = args.next().context("--repo needs a path")?.into(),
            "-h" | "--help" => {
                println!("{LIVE_HELP}");
                return Ok(());
            }
            other => anyhow::bail!("unknown flag {other}"),
        }
    }

    let runs = aster_eval::sweep(&target, &aster_eval::default_cases(), &models, runs)?;
    print!("{}", aster_eval::render_live(&runs));
    if runs.iter().any(|r| r.failed() > 0) {
        std::process::exit(1);
    }
    Ok(())
}

const LIVE_HELP: &str = "\
aster-eval live [options]

Runs aster against fixed cases and compares models. Each case spawns
`aster --stream` in the repo and is judged from the events it emits.

  --models a,b   models to compare (default: the configured one)
  --runs N       run every case N times and report medians (default: 1)
  --bin PATH     aster binary to measure (default: `aster` on PATH)
  --repo DIR     checkout aster runs against (default: the enclosing git repo)";

const HELP: &str = "\
aster-eval [SESSIONS_DIR] [options]

Grades recorded aster sessions: model round-trips per turn, how well the model
batches tool calls, and which tools answer nothing.

  --since DAYS     only sessions created in the last DAYS days
  --model NAME     only sessions recorded against this model
  --json           emit the report as JSON, for use as a baseline
  --baseline FILE  compare against a report saved earlier with --json

Subcommands:
  live             run fixed cases against models (see `live --help`)";
