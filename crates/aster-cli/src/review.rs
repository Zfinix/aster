use std::io::{self, IsTerminal, Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, mpsc};
use std::time::Duration;
use std::{env, fs, mem};

use anyhow::{Context, Result, bail};
use aster_ai::AiClient;
use aster_harness::{
    HarnessConfig, Progress, ProgressSink, ReviewDeps, ReviewInput, indexing, review_with_progress,
};
use aster_models::{Finding, ReviewReport};
use clap::Args;
use tempfile::TempDir;

use crate::provider::env_or;
use crate::{config, git, github};

#[derive(Args)]
pub struct ReviewArgs {
    /// Review a GitHub PR by number (fetches its diff).
    #[arg(long, value_name = "N")]
    pr: Option<u64>,

    /// Explicit git range to diff, e.g. `main..HEAD`.
    #[arg(long, value_name = "RANGE", conflicts_with_all = ["pr", "diff"])]
    range: Option<String>,

    /// Read the diff from a file, or `-` for stdin.
    #[arg(long, value_name = "PATH", conflicts_with_all = ["pr", "range"])]
    diff: Option<String>,

    /// Post findings as inline comments on the PR (implies --pr).
    #[arg(long, requires = "pr")]
    comment: bool,

    /// owner/repo for --pr. Defaults to the origin remote.
    #[arg(long, value_name = "OWNER/REPO")]
    repo: Option<String>,

    /// GitHub token override (else GITHUB_TOKEN, else `aster login`).
    #[arg(long, value_name = "TOKEN")]
    token: Option<String>,

    /// Repo to index for evidence retrieval. Defaults to the current directory.
    #[arg(long, value_name = "DIR")]
    repo_root: Option<PathBuf>,

    /// Skip building the symbol index (faster, but verify runs with less evidence).
    #[arg(long)]
    no_index: bool,

    /// Only review files matching this glob (repeatable). Overrides aster.yaml
    /// `include`. e.g. --include "crates/aster-cli/**" --include "**/*.rs"
    #[arg(long = "include", short = 'i', value_name = "GLOB")]
    include: Vec<String>,

    /// Skip files matching this glob (repeatable). Added on top of aster.yaml
    /// `exclude` and the built-in defaults.
    #[arg(long = "exclude", short = 'x', value_name = "GLOB")]
    exclude: Vec<String>,

    /// Drop findings below this confidence (0.0-1.0). Falls back to aster.yaml.
    #[arg(long, value_name = "F")]
    min_confidence: Option<f32>,

    /// Emit findings as JSON instead of text.
    #[arg(long, conflicts_with = "tui")]
    json: bool,

    /// Stream structured NDJSON progress events to stdout, one per line
    /// (phases, hypotheses, verdicts, findings, usage). For editors and UIs.
    #[arg(long, conflicts_with_all = ["tui", "json", "comment"])]
    pub stream: bool,

    /// Browse findings in an interactive terminal UI.
    #[arg(long, conflicts_with = "comment")]
    pub tui: bool,
}

pub async fn run(args: ReviewArgs) -> Result<()> {
    let repo_root = args.repo_root.clone().or_else(|| env::current_dir().ok());

    let settings = crate::settings::Settings::load(repo_root.as_deref())?;
    let review = &settings.review;

    let llm = crate::provider::resolve(review, None)?;
    let ai_client = AiClient::new(llm.base_url, llm.api_key, llm.model);

    let (raw_diff, pr_target) = resolve_diff(&args).await?;
    // --include overrides the file's include list; --exclude adds to it.
    let include = if args.include.is_empty() {
        review.include.clone()
    } else {
        args.include.clone()
    };
    let mut exclude = review.exclude.clone();
    exclude.extend(args.exclude.iter().cloned());
    let filter = crate::settings::PathFilter::new(&include, &exclude)?;
    let diff = crate::settings::filter_diff(&raw_diff, &filter);
    if diff.trim().is_empty() {
        bail!("empty diff (nothing to review, or everything was filtered by include/exclude)");
    }

    let min_confidence = args
        .min_confidence
        .or(review.min_confidence)
        .unwrap_or_else(|| HarnessConfig::default().min_confidence);
    let analyzers = {
        let from_env = parse_analyzers();
        if from_env.is_empty() {
            review.analyzers.clone()
        } else {
            from_env
        }
    };
    let astgrep_rules =
        resolve_astgrep_rules(review.astgrep_rules.as_deref(), repo_root.as_deref());

    let input = ReviewInput {
        diff,
        base_branch: args.range.clone().unwrap_or_else(|| "base".into()),
        repo_name: pr_target
            .as_ref()
            .map(|(o, r, _)| format!("{o}/{r}"))
            .unwrap_or_else(|| env::var("ASTER_REPO").unwrap_or_else(|_| "local".into())),
        pr_number: pr_target.as_ref().map(|(_, _, n)| *n as i64),
        repo_root,
    };

    // Usage counters are shared behind an Arc, so this clone reports the same
    // totals after the client has been moved into the review job.
    let usage_handle = ai_client.clone();

    let job = Job {
        ai_client,
        config: HarnessConfig {
            hypothesis_model: env_or("ASTER_HYPOTHESIS_MODEL", review.hypothesis_model.as_deref()),
            verify_model: env_or("ASTER_VERIFY_MODEL", review.verify_model.as_deref()),
            min_confidence,
            analyzers,
            astgrep_rules,
            focus_areas: review.focus_areas.clone(),
            max_diff_bytes: review
                .max_diff_bytes
                .unwrap_or_else(|| HarnessConfig::default().max_diff_bytes),
            verify_concurrency: env::var("ASTER_VERIFY_CONCURRENCY")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or_else(|| HarnessConfig::default().verify_concurrency),
            ..HarnessConfig::default()
        },
        input,
        no_index: args.no_index,
    };

    // Only enter the ratatui alt-screen when stdout is a real terminal; otherwise
    // fall through to the plain streaming path so piped/CI runs still work.
    if args.tui && io::stdout().is_terminal() {
        return crate::tui::run(job, min_confidence).await;
    }

    // Structured NDJSON: emit one event per line to stdout so editors and UIs
    // can render the live hypothesize/verify/confirm feed and final usage.
    if args.stream {
        return run_stream(job, min_confidence, usage_handle).await;
    }

    // JSON and comment modes must keep stdout clean; run them silently.
    // The default text mode streams a live feed to stderr as the model works.
    let response = if args.json || args.comment {
        execute(job, &None).await?
    } else {
        run_streaming(job, min_confidence).await?
    };
    let findings: Vec<Finding> = response
        .findings
        .into_iter()
        .filter(|f| f.confidence.unwrap_or(1.0) >= min_confidence)
        .collect();

    if args.comment {
        let (owner, repo, pr) = pr_target.expect("--comment requires --pr");
        let token = config::resolve_github_token(args.token.as_deref())
            .context("no GitHub token; run `aster login` or set GITHUB_TOKEN")?;
        github::post_review(&owner, &repo, pr, &token, &findings).await?;
        println!(
            "Posted {} comment(s) to {owner}/{repo}#{pr}.",
            findings.len()
        );
    } else if args.json {
        println!("{}", serde_json::to_string_pretty(&findings)?);
    } else {
        print_findings(&response.summary, &findings);
    }
    print_usage(usage_handle.usage_snapshot());
    Ok(())
}

/// Runs a review and emits NDJSON events to stdout: one JSON object per progress event and a final `done` event.
async fn run_stream(job: Job, min_confidence: f32, usage_handle: AiClient) -> Result<()> {
    let mut out = io::stdout();

    // The diff first, so the UI can render the GitHub-style view immediately and
    // then attach findings as inline comments as they stream in.
    let diff_event = serde_json::json!({
        "type": "diff",
        "content": job.input.diff,
        "repo_name": job.input.repo_name,
        "base_branch": job.input.base_branch,
    });
    let _ = writeln!(out, "{diff_event}");
    let _ = out.flush();

    let (tx, rx) = mpsc::channel::<Progress>();
    let task = tokio::spawn(async move { execute(job, &Some(tx)).await });

    let mut emit = |event: &Progress| emit_event(&mut out, event, min_confidence);

    loop {
        while let Ok(event) = rx.try_recv() {
            emit(&event);
        }
        if task.is_finished() {
            while let Ok(event) = rx.try_recv() {
                emit(&event);
            }
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    let report = task.await.map_err(|e| anyhow::anyhow!(e))??;
    let u = usage_handle.usage_snapshot();
    let done = serde_json::json!({
        "type": "done",
        "summary": report.summary,
        "total": report.findings.len(),
        "usage": {
            "prompt_tokens": u.prompt_tokens,
            "completion_tokens": u.completion_tokens,
            "total_tokens": u.total_tokens,
            "requests": u.requests,
            "estimated_cost_usd": u.estimated_cost_usd,
            "estimated": u.estimated,
        },
    });
    let _ = writeln!(out, "{done}");
    let _ = out.flush();
    Ok(())
}

/// Serialize a single progress event as a one-line JSON object. The harness's
/// own `Done` is dropped here; `run_stream` emits a richer one with usage.
fn emit_event(out: &mut impl Write, event: &Progress, min_confidence: f32) {
    let value = match event {
        Progress::Phase(name) => serde_json::json!({ "type": "phase", "name": name }),
        Progress::Token { stage, delta } => {
            serde_json::json!({ "type": "token", "stage": stage, "delta": delta })
        }
        Progress::Hypothesized { count } => {
            serde_json::json!({ "type": "hypothesized", "count": count })
        }
        Progress::Verifying {
            index,
            total,
            title,
        } => serde_json::json!({
            "type": "verifying", "index": index, "total": total, "title": title,
        }),
        Progress::Confirmed(finding) => {
            if finding.confidence.unwrap_or(1.0) < min_confidence {
                return;
            }
            let mut v = serde_json::to_value(finding.as_ref()).unwrap_or_default();
            if let Some(obj) = v.as_object_mut() {
                obj.insert("type".into(), serde_json::json!("finding"));
            }
            v
        }
        Progress::Refuted { title, reason } => {
            serde_json::json!({ "type": "refuted", "title": title, "reason": reason })
        }
        Progress::Done { .. } => return,
    };
    let _ = writeln!(out, "{value}");
    let _ = out.flush();
}

/// Report token spend to stderr so it never pollutes stdout (JSON, pipes).
pub(crate) fn print_usage(u: aster_ai::UsageSnapshot) {
    if u.total_tokens == 0 {
        return;
    }
    let approx = if u.estimated { "~" } else { "" };
    let cost = u
        .estimated_cost_usd
        .map(|c| format!("  {DIM}·{RESET}  {GREEN}~${c:.4}{RESET}"))
        .unwrap_or_default();
    let note = if u.estimated {
        format!("  {DIM}(estimated){RESET}")
    } else {
        String::new()
    };
    eprintln!(
        "{DIM}⚡ {approx}{} tokens{RESET}  {DIM}·{RESET}  {}{} in {DIM}·{RESET} {}{} out  {DIM}·{RESET}  {} req{}{}",
        human(u.total_tokens),
        approx,
        human(u.prompt_tokens),
        approx,
        human(u.completion_tokens),
        u.requests,
        cost,
        note,
    );
}

/// Compact human-readable token counts: 1_234 -> "1.2k", 2_500_000 -> "2.5M".
fn human(n: u64) -> String {
    match n {
        0..=999 => n.to_string(),
        1_000..=999_999 => trim_zero(n as f64 / 1_000.0, 'k'),
        _ => trim_zero(n as f64 / 1_000_000.0, 'M'),
    }
}

fn trim_zero(v: f64, suffix: char) -> String {
    let s = format!("{v:.1}");
    let s = s.strip_suffix(".0").map(str::to_string).unwrap_or(s);
    format!("{s}{suffix}")
}

pub struct Job {
    pub ai_client: AiClient,
    pub config: HarnessConfig,
    pub input: ReviewInput,
    pub no_index: bool,
}

pub async fn execute(job: Job, sink: &ProgressSink) -> Result<ReviewReport> {
    let Job {
        ai_client,
        config,
        input,
        no_index,
    } = job;

    // Held until the review finishes so the sqlite pool (and its -wal/-shm
    // siblings) outlives indexing; dropped on return, cleaning the whole dir.
    let mut _index_dir: Option<TempDir> = None;
    let index = if no_index {
        None
    } else if let Some(root) = input.repo_root.clone() {
        emit(sink, Progress::Phase("Indexing repository".into()));
        let dir = tempfile::tempdir().context("creating temp dir for index")?;
        let db = dir.path().join("index.sqlite");
        let (index, count) = indexing::build_repo_index(&root, &db).await?;
        emit(sink, Progress::Phase(format!("Indexed {count} symbols")));
        _index_dir = Some(dir);
        Some(index)
    } else {
        None
    };

    let deps = ReviewDeps {
        ai_client: Arc::new(ai_client),
        index,
        config,
    };
    review_with_progress(&deps, input, sink).await
}

fn emit(sink: &ProgressSink, event: Progress) {
    if let Some(tx) = sink {
        let _ = tx.send(event);
    }
}

// ANSI helpers so the live feed reads like a real streaming session without
// pulling in a styling dependency for the plain (non-TUI) path.
const DIM: &str = "\x1b[2m";
const CYAN: &str = "\x1b[36m";
const GREEN: &str = "\x1b[32m";
const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
/// Brand orange, matching the desktop app's `--accent` (#f2764f).
const ORANGE: &str = "\x1b[38;2;242;118;79m";

async fn run_streaming(job: Job, min_confidence: f32) -> Result<ReviewReport> {
    // Concurrent verify streams interleave into noise; stream raw tokens only
    // when verification runs sequentially.
    let stream_verify = job.config.verify_concurrency <= 1;
    let (tx, rx) = mpsc::channel::<Progress>();
    let task = tokio::spawn(async move { execute(job, &Some(tx)).await });

    let mut err = io::stderr();
    let mut stage: Option<String> = None;
    let mut at_line_start = true;

    let mut drain = |event: Progress, stage: &mut Option<String>, at_line_start: &mut bool| {
        match event {
            Progress::Phase(name) => {
                if !*at_line_start {
                    let _ = writeln!(err);
                }
                let _ = writeln!(err, "{ORANGE}▶ {name}{RESET}");
                *stage = None;
                *at_line_start = true;
            }
            Progress::Token { stage: s, delta } => {
                // Only verify is gated; hypothesis is always a single stream.
                if s == "verify" && !stream_verify {
                    return;
                }
                if stage.as_deref() != Some(&s) {
                    if !*at_line_start {
                        let _ = writeln!(err);
                    }
                    let _ = write!(err, "{DIM}");
                    *stage = Some(s);
                }
                let _ = write!(err, "{delta}");
                let _ = err.flush();
                *at_line_start = delta.ends_with('\n');
            }
            Progress::Hypothesized { count } => {
                let _ = write!(err, "{RESET}");
                let _ = writeln!(err, "\n{DIM}  {count} candidate(s) to verify{RESET}");
                *stage = None;
                *at_line_start = true;
            }
            Progress::Verifying {
                index,
                total,
                title,
            } => {
                let _ = write!(err, "{RESET}");
                if !*at_line_start {
                    let _ = writeln!(err);
                }
                let _ = writeln!(err, "{DIM}  ⚖ [{index}/{total}] {title}{RESET}");
                *stage = None;
                *at_line_start = true;
            }
            Progress::Confirmed(f) => {
                if f.confidence.unwrap_or(1.0) < min_confidence {
                    return;
                }
                let _ = writeln!(
                    err,
                    "  {GREEN}✓{RESET} {} {DIM}{}:{}{RESET}",
                    f.title, f.file_path, f.line
                );
                *at_line_start = true;
            }
            Progress::Refuted { title, .. } => {
                let _ = writeln!(err, "  {DIM}✗ refuted: {title}{RESET}");
                *at_line_start = true;
            }
            Progress::Done { .. } => {}
        }
    };

    loop {
        while let Ok(event) = rx.try_recv() {
            drain(event, &mut stage, &mut at_line_start);
        }
        if task.is_finished() {
            while let Ok(event) = rx.try_recv() {
                drain(event, &mut stage, &mut at_line_start);
            }
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    let _ = writeln!(err, "{RESET}");
    task.await.map_err(|e| anyhow::anyhow!(e))?
}

type PrTarget = (String, String, u64);

async fn resolve_diff(args: &ReviewArgs) -> Result<(String, Option<PrTarget>)> {
    if let Some(pr) = args.pr {
        let (owner, repo) = match &args.repo {
            Some(slug) => {
                let (o, r) = slug.split_once('/').context("--repo must be owner/repo")?;
                (o.to_string(), r.to_string())
            }
            None => git::origin_repo().context("could not detect repo; pass --repo owner/repo")?,
        };
        let token = config::resolve_github_token(args.token.as_deref())
            .context("no GitHub token; run `aster login` or set GITHUB_TOKEN")?;
        let diff = github::fetch_pr_diff(&owner, &repo, pr, &token).await?;
        return Ok((diff, Some((owner, repo, pr))));
    }
    if let Some(path) = &args.diff {
        let diff = if path == "-" {
            let mut buf = String::new();
            io::stdin().read_to_string(&mut buf)?;
            buf
        } else {
            fs::read_to_string(path).with_context(|| format!("reading {path}"))?
        };
        return Ok((diff, None));
    }
    if let Some(range) = &args.range {
        return Ok((git::diff(Some(range))?, None));
    }
    let working = git::diff(None)?;
    if !working.trim().is_empty() {
        return Ok((working, None));
    }
    let range = git::default_range();
    let committed = git::diff(Some(&range))?;
    if !committed.trim().is_empty() {
        return Ok((committed, None));
    }
    Ok((git::diff(Some("HEAD~1..HEAD"))?, None))
}

fn resolve_astgrep_rules(cfg: Option<&str>, repo_root: Option<&std::path::Path>) -> Option<String> {
    let rel = cfg?;
    let path = match repo_root {
        Some(root) => root.join(rel),
        None => PathBuf::from(rel),
    };
    match std::fs::read_to_string(&path) {
        Ok(content) => Some(content),
        Err(e) => {
            tracing::warn!(path = %path.display(), error = %e, "could not read astgrep_rules; skipping ast-grep rules");
            None
        }
    }
}

fn parse_analyzers() -> Vec<String> {
    env::var("ASTER_ANALYZERS")
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect()
}

fn print_findings(summary: &str, findings: &[Finding]) {
    // Honor the informal color conventions: NO_COLOR disables, CLICOLOR_FORCE
    // forces on even when stdout isn't a TTY (e.g. captured in CI or a demo).
    let color = env::var_os("NO_COLOR").is_none()
        && (io::stdout().is_terminal() || env::var_os("CLICOLOR_FORCE").is_some());
    let paint = |code: &str, text: &str| {
        if color {
            format!("{code}{text}{RESET}")
        } else {
            text.to_string()
        }
    };

    let count = findings.len();
    let dot = if count == 0 {
        paint(GREEN, "✓")
    } else {
        paint(ORANGE, "✳")
    };
    println!();
    println!("  {dot} {}", paint(BOLD, summary));

    if findings.is_empty() {
        println!(
            "  {}\n",
            paint(DIM, "nothing survived verification — clean diff.")
        );
        return;
    }
    println!();

    for (i, f) in findings.iter().enumerate() {
        let badge = severity_badge(&f.severity, color);
        let conf = f
            .confidence
            .map(|c| paint(DIM, &format!("{:.0}%", c * 100.0)))
            .unwrap_or_default();
        let n = paint(DIM, &format!("{}/{}", i + 1, count));

        println!("  {badge} {}  {}  {}", paint(DIM, &f.category), n, conf);
        println!("  {}", paint(BOLD, &f.title));
        println!("  {}", paint(CYAN, &format!("{}:{}", f.file_path, f.line)));
        println!();
        for line in wrap(&f.description, 76) {
            println!("    {line}");
        }
        println!("  {} {}", paint(GREEN, "→ fix"), paint(DIM, &f.suggestion));
        println!();
    }
}

fn severity_badge(severity: &str, color: bool) -> String {
    if !color {
        return format!("[{}]", severity.to_uppercase());
    }
    let bg = match severity {
        "critical" => "\x1b[41m\x1b[97m", // red bg, bright white
        "high" => "\x1b[101m\x1b[30m",    // bright-red bg, black
        "medium" => "\x1b[43m\x1b[30m",   // yellow bg, black
        "low" => "\x1b[44m\x1b[97m",      // blue bg, bright white
        _ => "\x1b[100m\x1b[97m",         // gray bg
    };
    format!("{bg} {} {RESET}", severity.to_uppercase())
}

fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        if !line.is_empty() && line.len() + 1 + word.len() > width {
            lines.push(mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}
