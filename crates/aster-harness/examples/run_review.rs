//! Run the review harness on a real diff.
//!
//! Usage:
//!   cargo run -p aster-harness --example run_review -- <diff-file | ->
//!
//! Config via env (any OpenAI-compatible provider):
//!   ASTER_API_KEY          required (falls back to OPEN_ROUTER_API_KEY)
//!   ASTER_BASE_URL         default https://openrouter.ai/api/v1
//!   ASTER_MODEL            default openai/gpt-4o-mini (used when a stage
//!                          override below is unset)
//!   ASTER_HYPOTHESIS_MODEL optional; model for the high-recall hypothesis pass
//!   ASTER_VERIFY_MODEL     optional; independent model for adversarial verify
//!   ASTER_REPO             repo name for the summary (default "local")

use std::io::Read;
use std::sync::Arc;

use aster_ai::AiClient;
use aster_harness::{HarnessConfig, ReviewDeps, ReviewInput, review};

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

    // Load .env so the documented `cargo run --example run_review` works with a
    // configured provider, matching the CLI's behavior.
    let _ = dotenvy::dotenv();

    let ai_client = AiClient::from_env()?;

    // Runtime analyzer toggle: ASTER_ANALYZERS="semgrep,ast-grep" (empty = LLM only).
    let analyzers: Vec<String> = std::env::var("ASTER_ANALYZERS")
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect();
    let repo_root = std::env::var("ASTER_REPO_ROOT")
        .ok()
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::current_dir().ok());

    let deps = ReviewDeps {
        ai_client: Arc::new(ai_client),
        index: None,
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
        repo_root,
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

fn read_diff(path: &str) -> anyhow::Result<String> {
    if path == "-" {
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf)?;
        Ok(buf)
    } else {
        Ok(std::fs::read_to_string(path)?)
    }
}
