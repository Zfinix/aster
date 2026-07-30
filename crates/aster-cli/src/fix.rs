//! `aster fix`: turn review findings into SEARCH/REPLACE edits. Dry-run by default; `--apply` writes.

use std::io::{IsTerminal, Read};
use std::path::{Path, PathBuf};
use std::{env, fs, io};

use anyhow::{Context, Result, bail};
use aster_ai::AiClient;
use aster_models::Finding;
use aster_policy::{Action, Decision, Policy};
use clap::Args;
use serde::Serialize;

use crate::edits;
use crate::term::{DIM, GREEN, RED, RESET};

const FIX_SYSTEM_PROMPT: &str = include_str!("../prompts/aster-fix.md");
const FIX_TEMPERATURE: f32 = 0.0;
/// Files larger than this are sent as a window around the finding, not whole.
const MAX_WHOLE_FILE_BYTES: usize = 60_000;
const WINDOW_LINES: usize = 150;

#[derive(Args)]
pub struct FixArgs {
    /// JSON array of findings (from `aster review --json`), from PATH or `-`
    /// for stdin, which is the default so a review can be piped straight in.
    #[arg(long, value_name = "PATH", default_value = "-")]
    findings_json: String,

    /// Write the edits to the working tree (otherwise print a dry-run preview).
    #[arg(long)]
    apply: bool,

    /// Model override (else ASTER_MODEL, aster.yaml, default).
    #[arg(long, value_name = "MODEL")]
    model: Option<String>,

    /// Repository root the file paths are relative to. Defaults to the current directory.
    #[arg(long, value_name = "DIR")]
    repo_root: Option<PathBuf>,
}

/// Per-finding outcome, also the `--json` wire shape.
#[derive(Debug, Serialize)]
struct FixResult {
    file_path: String,
    line: i32,
    title: String,
    /// "applied" | "preview" | "cannot_fix" | "blocked" | "error"
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    patch: Option<String>,
}

pub async fn run(args: FixArgs) -> Result<()> {
    let repo_root = args
        .repo_root
        .clone()
        .or_else(|| env::current_dir().ok())
        .context("could not determine the repository root")?;

    let settings = crate::settings::Settings::load(Some(&repo_root))?;
    let client = crate::provider::resolve_client(&settings, args.model.as_deref())?;
    let policy = Policy::compile(&settings.permissions)?;

    let json = crate::json_mode();
    let findings = read_findings(&args.findings_json)?;
    if findings.is_empty() {
        bail!("no findings to fix (the findings JSON was an empty array)");
    }

    let mut results = Vec::with_capacity(findings.len());
    for finding in &findings {
        let result = fix_one(&client, &repo_root, &policy, finding, args.apply).await;
        if !json {
            print_result(&result);
        }
        results.push(result);
    }

    if json {
        println!("{}", serde_json::to_string(&results)?);
    } else {
        crate::review::print_usage(client.usage_snapshot());
    }

    let failed = results.iter().filter(|r| r.status == "error").count();
    if failed == results.len() {
        bail!("no finding could be fixed");
    }
    Ok(())
}

fn read_findings(source: &str) -> Result<Vec<Finding>> {
    let raw = if source == "-" {
        if io::stdin().is_terminal() {
            bail!(
                "no findings on stdin. Pipe a review in (`aster review --json | aster fix`) or pass --findings-json <PATH>"
            );
        }
        let mut buf = String::new();
        io::stdin()
            .read_to_string(&mut buf)
            .context("reading findings from stdin")?;
        buf
    } else {
        fs::read_to_string(source).with_context(|| format!("reading {source}"))?
    };
    serde_json::from_str(&raw)
        .context("parsing --findings-json: expected a JSON array of finding objects")
}

async fn fix_one(
    client: &AiClient,
    repo_root: &Path,
    policy: &Policy,
    finding: &Finding,
    apply: bool,
) -> FixResult {
    let base = FixResult {
        file_path: finding.file_path.clone(),
        line: finding.line,
        title: finding.title.clone(),
        status: "error".into(),
        reason: None,
        patch: None,
    };
    match try_fix(client, repo_root, policy, finding, apply).await {
        Ok(result) => result,
        Err(e) => FixResult {
            reason: Some(format!("{e:#}")),
            ..base
        },
    }
}

async fn try_fix(
    client: &AiClient,
    repo_root: &Path,
    policy: &Policy,
    finding: &Finding,
    apply: bool,
) -> Result<FixResult> {
    let (path, content) = edits::read_repo_file(repo_root, &finding.file_path)?;

    let excerpt = excerpt_for(&content, finding.line as usize);
    let user = fix_request(
        finding,
        excerpt.as_deref().unwrap_or(&content),
        excerpt.is_some(),
    );

    let reply = client
        .complete(FIX_SYSTEM_PROMPT, &user, FIX_TEMPERATURE)
        .await?;
    let trimmed = reply.trim();
    if let Some(why) = trimmed.strip_prefix("CANNOT_FIX:") {
        return Ok(FixResult {
            file_path: finding.file_path.clone(),
            line: finding.line,
            title: finding.title.clone(),
            status: "cannot_fix".into(),
            reason: Some(why.trim().to_string()),
            patch: None,
        });
    }

    let blocks = edits::parse_blocks(&reply)?;
    let mut updated = content.clone();
    let mut patch = String::new();
    for block in &blocks {
        updated = edits::apply_block(&updated, block)?;
        patch.push_str(&edits::preview(block));
    }

    if apply {
        // `fix` is non-interactive: edits needing approval (mode `ask`) or denied are blocked, not written.
        if let Decision::Deny { reason } | Decision::Prompt { preview: reason } =
            policy.evaluate(&Action::Edit {
                path: &finding.file_path,
            })
        {
            return Ok(FixResult {
                file_path: finding.file_path.clone(),
                line: finding.line,
                title: finding.title.clone(),
                status: "blocked".into(),
                reason: Some(reason),
                patch: Some(patch),
            });
        }
        fs::write(&path, &updated).with_context(|| format!("writing {}", path.display()))?;
    }
    Ok(FixResult {
        file_path: finding.file_path.clone(),
        line: finding.line,
        title: finding.title.clone(),
        status: if apply { "applied" } else { "preview" }.into(),
        reason: None,
        patch: Some(patch),
    })
}

fn fix_request(finding: &Finding, code: &str, is_excerpt: bool) -> String {
    let mut msg = format!(
        "Finding to fix:\n\
         - title: {}\n\
         - severity: {}\n\
         - location: {}:{}\n\
         - description: {}\n",
        finding.title, finding.severity, finding.file_path, finding.line, finding.description,
    );
    if !finding.suggestion.is_empty() {
        msg.push_str(&format!("- suggested fix: {}\n", finding.suggestion));
    }
    let scope = if is_excerpt {
        format!(
            "\nAn excerpt of `{}` around the finding (the full file is larger):\n",
            finding.file_path
        )
    } else {
        format!("\nThe full content of `{}`:\n", finding.file_path)
    };
    msg.push_str(&scope);
    msg.push_str(code);
    msg
}

/// Lines around the finding for large files; None when the whole file fits.
fn excerpt_for(content: &str, line: usize) -> Option<String> {
    if content.len() <= MAX_WHOLE_FILE_BYTES {
        return None;
    }
    let lines: Vec<&str> = content.lines().collect();
    let center = line.saturating_sub(1).min(lines.len().saturating_sub(1));
    let start = center.saturating_sub(WINDOW_LINES);
    let end = (center + WINDOW_LINES).min(lines.len());
    Some(lines[start..end].join("\n"))
}

fn print_result(r: &FixResult) {
    let (badge, color) = match r.status.as_str() {
        "applied" => ("✓ applied", GREEN),
        "preview" => ("± preview", DIM),
        "cannot_fix" => ("∅ cannot fix", DIM),
        "blocked" => ("⊘ blocked", RED),
        _ => ("✗ error", RED),
    };
    println!(
        "{color}{badge}{RESET}  {}  {DIM}{}:{}{RESET}",
        r.title, r.file_path, r.line
    );
    if let Some(reason) = &r.reason {
        println!("  {DIM}{reason}{RESET}");
    }
    if let Some(patch) = &r.patch {
        for line in patch.lines() {
            let color = match line.as_bytes().first() {
                Some(b'+') => GREEN,
                Some(b'-') => RED,
                _ => RESET,
            };
            println!("  {color}{line}{RESET}");
        }
    }
}
