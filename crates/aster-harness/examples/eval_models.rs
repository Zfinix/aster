//! Benchmark candidate hypothesis models on recall, latency, and cost.
//!
//! Pulls OpenRouter's live catalog, keeps the cheap text models, runs the real
//! hypothesis prompt on a diff with several planted bugs, and ranks by how many
//! it catches (then speed, then price). This is the Rust reproduction of the
//! numbers in docs/BENCHMARKS.md.
//!
//! Usage:
//!   ASTER_API_KEY=... cargo run -p aster-harness --example eval_models
//!
//! Env knobs (optional):
//!   ASTER_BASE_URL     default https://openrouter.ai/api/v1
//!   EVAL_PRICE_CAP     max $/token for the prompt side (default 1e-6 = $1/M)
//!   EVAL_MAX_MODELS    cap the fan-out (default 120)
//!   EVAL_CONCURRENCY   parallel calls (default 16)

use std::collections::HashSet;
use std::time::Instant;

use anyhow::{Context, Result};
use aster_harness::{CandidateList, prompts};
use futures_util::stream::{self, StreamExt};
use serde::Deserialize;
use serde_json::json;

/// Bugs planted in the fixture diff: (id, new-file line, keyword signatures). A
/// bug counts as found if a candidate lands within one line or names a keyword.
const BUGS: &[(&str, i32, &[&str])] = &[
    (
        "sql-injection",
        4,
        &["inject", "sql", "format!", "sanit", "parameteri"],
    ),
    (
        "unwrap-panic",
        5,
        &["unwrap", "panic", "none", "empty result"],
    ),
    (
        "off-by-one",
        11,
        &[
            "off-by-one",
            "out of bounds",
            "0..=",
            "index",
            "bounds",
            "overflow",
        ],
    ),
    (
        "race-toctou",
        17,
        &[
            "race",
            "toctou",
            "check-then-act",
            "concurren",
            "atomic",
            "double spend",
            "double-spend",
        ],
    ),
    (
        "file-leak",
        24,
        &["leak", "not closed", "flush", "close", "handle"],
    ),
    (
        "ignored-result",
        25,
        &[
            "ignored",
            "swallow",
            "discard",
            "let _",
            "error is",
            "unhandled",
            "silently",
        ],
    ),
    (
        "auth-bypass",
        30,
        &[
            "|| true",
            "always true",
            "always returns",
            "bypass",
            "authoriz",
            "hardcod",
        ],
    ),
];

const DIFF: &str = include_str!("fixtures/multibug.diff");
/// Substrings that mark a model as not a general code reviewer: embeddings,
/// classifiers, audio/media, and OpenRouter's meta-routers (which forward to
/// other models and muddy the ranking).
const SKIP: &[&str] = &[
    "embed",
    "moderation",
    "tts",
    "whisper",
    "guard",
    "rerank",
    "-vision",
    "sonar-reasoning",
    "openrouter/",
    "lyria",
    "content-safety",
];

#[derive(Deserialize)]
struct ModelsResp {
    data: Vec<ModelInfo>,
}

#[derive(Deserialize)]
struct ModelInfo {
    id: String,
    #[serde(default)]
    context_length: Option<u64>,
    #[serde(default)]
    pricing: Pricing,
    #[serde(default)]
    architecture: Architecture,
}

#[derive(Deserialize, Default)]
struct Pricing {
    #[serde(default)]
    prompt: String,
}

#[derive(Deserialize, Default)]
struct Architecture {
    #[serde(default)]
    input_modalities: Option<Vec<String>>,
    #[serde(default)]
    output_modalities: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct ChatResp {
    #[serde(default)]
    choices: Vec<Choice>,
    #[serde(default)]
    usage: Option<Usage>,
    #[serde(default)]
    provider: Option<String>,
}

#[derive(Deserialize)]
struct Choice {
    message: Msg,
}

#[derive(Deserialize)]
struct Msg {
    #[serde(default)]
    content: Option<String>,
}

#[derive(Deserialize, Default)]
struct Usage {
    #[serde(default)]
    cost: Option<f64>,
}

struct Score {
    model: String,
    provider: String,
    secs: f64,
    cost: Option<f64>,
    candidates: usize,
    recall: usize,
}

#[tokio::main]
async fn main() -> Result<()> {
    let _ = dotenvy::dotenv();
    let key = std::env::var("ASTER_API_KEY")
        .or_else(|_| std::env::var("OPEN_ROUTER_API_KEY"))
        .context("set ASTER_API_KEY (or OPEN_ROUTER_API_KEY)")?;
    let base = std::env::var("ASTER_BASE_URL")
        .unwrap_or_else(|_| "https://openrouter.ai/api/v1".to_string());
    let price_cap: f64 = env_or("EVAL_PRICE_CAP", 1e-6);
    let max_models: usize = env_or("EVAL_MAX_MODELS", 120);
    let concurrency: usize = env_or("EVAL_CONCURRENCY", 16);

    let http = reqwest::Client::new();
    let user_prompt = prompts::hypothesis_user_prompt("local", &[], DIFF);

    let models = fetch_models(&http, &base, &key, price_cap, max_models).await?;
    println!(
        "Recall eval: {} cheap text models, {} planted bugs, {}-way parallel\n",
        models.len(),
        BUGS.len(),
        concurrency
    );

    let scores: Vec<Score> = stream::iter(models)
        .map(|model| {
            let http = &http;
            let base = &base;
            let key = &key;
            let user_prompt = &user_prompt;
            async move {
                match eval_one(http, base, key, user_prompt, &model).await {
                    Ok(score) => {
                        println!(
                            "  {}/{}  {:44} {:5.1}s  {}  {}c",
                            score.recall,
                            BUGS.len(),
                            score.model,
                            score.secs,
                            fmt_cost(score.cost),
                            score.candidates
                        );
                        Some(score)
                    }
                    Err(e) => {
                        println!("  skip {model:44} {e}");
                        None
                    }
                }
            }
        })
        .buffer_unordered(concurrency)
        .filter_map(|s| async move { s })
        .collect()
        .await;

    let mut ranked = scores;
    ranked.sort_by(|a, b| {
        b.recall
            .cmp(&a.recall)
            .then(
                a.secs
                    .partial_cmp(&b.secs)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
            .then(
                cost_key(a.cost)
                    .partial_cmp(&cost_key(b.cost))
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
    });

    let total: f64 = ranked.iter().filter_map(|s| s.cost).sum();
    println!(
        "\n=== {} models evaluated. Total spend ~${total:.4} ===",
        ranked.len()
    );
    println!(
        "\n=== TOP 20 by recall (of {}), then latency, then cost ===",
        BUGS.len()
    );
    for (i, s) in ranked.iter().take(20).enumerate() {
        println!(
            "{:2}. {}/{}  {:44} {:5.1}s  {}  [{}]",
            i + 1,
            s.recall,
            BUGS.len(),
            s.model,
            s.secs,
            fmt_cost(s.cost),
            s.provider
        );
    }
    Ok(())
}

async fn fetch_models(
    http: &reqwest::Client,
    base: &str,
    key: &str,
    price_cap: f64,
    max_models: usize,
) -> Result<Vec<String>> {
    let resp: ModelsResp = http
        .get(format!("{base}/models"))
        .bearer_auth(key)
        .send()
        .await
        .context("fetching model list")?
        .json()
        .await
        .context("parsing model list")?;

    let mut cheap: Vec<(String, f64)> = resp
        .data
        .into_iter()
        .filter_map(|m| {
            if SKIP.iter().any(|s| m.id.contains(s)) {
                return None;
            }
            if !modality_ok(&m.architecture.output_modalities)
                || !modality_ok(&m.architecture.input_modalities)
            {
                return None;
            }
            if m.context_length.unwrap_or(0) < 8000 {
                return None;
            }
            let price = m.pricing.prompt.parse::<f64>().ok()?;
            (price <= price_cap).then_some((m.id, price))
        })
        .collect();

    cheap.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
    cheap.truncate(max_models);
    Ok(cheap.into_iter().map(|(id, _)| id).collect())
}

/// A modality list is acceptable if it's absent (assume text) or contains text.
fn modality_ok(m: &Option<Vec<String>>) -> bool {
    m.as_ref()
        .map(|v| v.iter().any(|s| s == "text"))
        .unwrap_or(true)
}

async fn eval_one(
    http: &reqwest::Client,
    base: &str,
    key: &str,
    user_prompt: &str,
    model: &str,
) -> Result<Score> {
    let body = json!({
        "model": model,
        "temperature": 0.1,
        "messages": [
            {"role": "system", "content": prompts::HYPOTHESIS_SYSTEM_PROMPT},
            {"role": "user", "content": user_prompt},
        ],
    });

    let started = Instant::now();
    let resp = http
        .post(format!("{base}/chat/completions"))
        .bearer_auth(key)
        .json(&body)
        .send()
        .await?;
    let status = resp.status();
    let text = resp.text().await?;
    if !status.is_success() {
        anyhow::bail!("HTTP {status}");
    }
    let secs = started.elapsed().as_secs_f64();

    let parsed: ChatResp = serde_json::from_str(&text).context("parsing chat response")?;
    let content = parsed
        .choices
        .into_iter()
        .next()
        .and_then(|c| c.message.content)
        .unwrap_or_default();
    let candidates = extract_candidates(&content);
    Ok(Score {
        model: model.to_string(),
        provider: parsed.provider.unwrap_or_else(|| "?".into()),
        secs,
        cost: parsed.usage.and_then(|u| u.cost),
        recall: recall(&candidates),
        candidates: candidates.len(),
    })
}

/// Pull the first JSON object out of a possibly-fenced model response and read
/// its candidates. Returns empty on any parse failure.
fn extract_candidates(content: &str) -> Vec<aster_harness::Candidate> {
    let (Some(start), Some(end)) = (content.find('{'), content.rfind('}')) else {
        return Vec::new();
    };
    serde_json::from_str::<CandidateList>(&content[start..=end])
        .map(|l| l.candidates)
        .unwrap_or_default()
}

fn recall(candidates: &[aster_harness::Candidate]) -> usize {
    let mut hits: HashSet<&str> = HashSet::new();
    for c in candidates {
        let text = format!(
            "{} {} {} {}",
            c.title,
            c.failure_scenario,
            c.suggestion,
            c.code_snippet.as_deref().unwrap_or("")
        )
        .to_lowercase();
        for (id, line, keywords) in BUGS {
            if (c.line - line).abs() <= 1 || keywords.iter().any(|k| text.contains(k)) {
                hits.insert(id);
            }
        }
    }
    hits.len()
}

fn fmt_cost(cost: Option<f64>) -> String {
    cost.map(|c| format!("${c:.5}"))
        .unwrap_or_else(|| "  n/a ".into())
}

fn cost_key(cost: Option<f64>) -> f64 {
    cost.unwrap_or(9.0)
}

fn env_or<T: std::str::FromStr>(key: &str, default: T) -> T {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}
