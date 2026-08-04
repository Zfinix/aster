//! `aster-eval [sessions-dir] [--since DAYS] [--model NAME] [--json]
//! [--baseline FILE]`. Defaults to every session under `~/.aster/sessions`.

use std::path::PathBuf;

use anyhow::{Context, Result};
use aster_eval::{Filter, Report, analyze, render, render_comparison};

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
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

const HELP: &str = "\
aster-eval [SESSIONS_DIR] [options]

Grades recorded aster sessions: model round-trips per turn, how well the model
batches tool calls, and which tools answer nothing.

  --since DAYS     only sessions created in the last DAYS days
  --model NAME     only sessions recorded against this model
  --json           emit the report as JSON, for use as a baseline
  --baseline FILE  compare against a report saved earlier with --json";
