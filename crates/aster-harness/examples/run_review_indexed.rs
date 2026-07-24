//! Run the review harness on a real diff *with* an in-memory symbol index, so
//! the verify stage receives retrieved evidence instead of reasoning blind.
//!
//! It walks the repo root, extracts symbols (tree-sitter-tags), builds a
//! SQLite/FTS index, then runs the same hypothesize -> retrieve -> verify -> shape
//! pipeline with `index: Some(..)` wired in.
//!
//! Usage:
//!   cargo run -p aster-harness --example run_review_indexed -- <diff-file | ->
//!
//! Config via env (same as run_review, plus):
//!   ASTER_REPO_ROOT  repo to index (default: current dir)
//!   ASTER_INDEX_DB   sqlite path for the index (default: a temp file)

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use aster_ai::AiClient;
use aster_harness::{HarnessConfig, ReviewDeps, ReviewInput, review};
use aster_index::{IndexSymbol, SqliteCodeIndex};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("aster_harness=debug,info")
        .init();

    let path = std::env::args().nth(1).unwrap_or_else(|| "-".to_string());
    let diff = read_diff(&path)?;
    if diff.trim().is_empty() {
        anyhow::bail!("empty diff; pass a diff file or pipe one on stdin");
    }

    let _ = dotenvy::dotenv();

    let ai_client = AiClient::from_env()?;

    let repo_root = std::env::var("ASTER_REPO_ROOT")
        .map(PathBuf::from)
        .or_else(|_| std::env::current_dir())?;

    let db_path = std::env::var("ASTER_INDEX_DB")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir().join("aster-index.sqlite"));
    let _ = std::fs::remove_file(&db_path);

    let index = SqliteCodeIndex::open(&db_path).await?;
    let symbols = collect_symbols(&repo_root);
    let indexed = index.build_from_symbols("local", "HEAD", &symbols).await?;
    tracing::info!(indexed, files_root = %repo_root.display(), "built symbol index");

    let analyzers: Vec<String> = std::env::var("ASTER_ANALYZERS")
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect();

    let deps = ReviewDeps {
        ai_client: Arc::new(ai_client),
        index: Some(index),
        config: HarnessConfig {
            analyzers,
            hypothesis_model: std::env::var("ASTER_HYPOTHESIS_MODEL").ok(),
            verify_model: std::env::var("ASTER_VERIFY_MODEL").ok(),
            ..HarnessConfig::default()
        },
    };

    let input = ReviewInput {
        diff,
        base_branch: "HEAD~1".to_string(),
        repo_name: std::env::var("ASTER_REPO").unwrap_or_else(|_| "local".to_string()),
        pr_number: None,
        repo_root: Some(repo_root),
    };

    let started = std::time::Instant::now();
    let response = review(&deps, input).await?;
    let elapsed = started.elapsed();

    println!("\n=== {} ===", response.summary);
    println!(
        "critical {} | high {} | medium {} | low {} | info {}",
        response.critical_severity_count,
        response.high_severity_count,
        response.medium_severity_count,
        response.low_severity_count,
        response.info_severity_count,
    );
    for (i, f) in response.findings.iter().enumerate() {
        println!(
            "\n[{}] {} ({}/{}) {}:{}  conf={:.2}",
            i + 1,
            f.title,
            f.severity,
            f.category,
            f.file_path,
            f.line,
            f.confidence.unwrap_or(0.0),
        );
        println!("    scenario: {}", f.description);
        println!("    fix: {}", f.suggestion);
    }
    println!("\nfinished in {:.1}s", elapsed.as_secs_f64());
    Ok(())
}

/// Walk the repo, skipping VCS/build dirs, and extract symbols from every file
/// the extractor recognizes.
fn collect_symbols(root: &Path) -> Vec<IndexSymbol> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if path.is_dir() {
                if matches!(name.as_ref(), "target" | ".git" | "node_modules") {
                    continue;
                }
                stack.push(path);
                continue;
            }
            let Ok(content) = std::fs::read_to_string(&path) else {
                continue;
            };
            let rel = path.strip_prefix(root).unwrap_or(&path);
            let rel = rel.to_string_lossy().to_string();
            for s in symbol_extractor::extract_symbols(&content, &rel) {
                out.push(IndexSymbol {
                    file_path: s.file_path,
                    name: s.symbol_name,
                    kind: s.symbol_kind,
                    start_line: s.start_line.map(i64::from),
                    end_line: s.end_line.map(i64::from),
                    snippet: s.code_snippet,
                });
            }
        }
    }
    out
}

fn read_diff(path: &str) -> anyhow::Result<String> {
    if path == "-" {
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf)?;
        Ok(buf)
    } else {
        Ok(std::fs::read_to_string(path)?)
    }
}
