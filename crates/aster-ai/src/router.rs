//! Model router: picks a real model id from OpenRouter's live benchmark data
//! when the configured model is `auto`. One `/benchmarks` call supplies coding,
//! agentic, and intelligence indices plus pricing; results are cached under the
//! Aster data dir with a TTL so a session start is usually network-free.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

/// The model value that routes through live rankings instead of a pinned id.
pub const AUTO_MODEL: &str = "auto";

/// Rankings older than this are refetched. A day keeps the 500/day data-API
/// quota irrelevant while still tracking a moving leaderboard.
const CACHE_TTL: Duration = Duration::from_secs(24 * 60 * 60);
/// Hard cap on cached entries; the benchmark feed is larger than we need.
const MAX_CACHED_MODELS: usize = 200;
const FETCH_TIMEOUT_SECS: u64 = 15;
/// Blended $/M ceiling for the cheap tier.
const CHEAP_PRICE_CEILING: f64 = 0.30;
/// Blended $/M ceiling for the balanced tier.
const BALANCED_PRICE_CEILING: f64 = 2.00;

const BENCHMARKS_URL: &str = "https://openrouter.ai/api/v1/benchmarks";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    Cheap,
    Balanced,
    Strong,
}

impl Tier {
    pub const ALL: [Tier; 3] = [Tier::Cheap, Tier::Balanced, Tier::Strong];

    pub fn as_str(self) -> &'static str {
        match self {
            Tier::Cheap => "cheap",
            Tier::Balanced => "balanced",
            Tier::Strong => "strong",
        }
    }

    pub fn parse(raw: &str) -> Option<Tier> {
        let lowered = raw.trim().to_ascii_lowercase();
        Tier::ALL.into_iter().find(|t| t.as_str() == lowered)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pick {
    pub model: String,
    pub tier: Tier,
    pub coding_index: f64,
    pub agentic_index: f64,
    /// Blended $/M tokens at a 3:1 prompt:completion mix.
    pub blended_price_per_m: f64,
    /// Where the pick came from, for the one-line note surfaces print.
    pub from_cache: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct Cache {
    fetched_at_secs: u64,
    entries: Vec<Entry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Entry {
    slug: String,
    coding_index: f64,
    agentic_index: f64,
    blended_price_per_m: f64,
}

#[derive(Deserialize)]
struct BenchmarksResponse {
    data: Vec<BenchmarkRow>,
}

#[derive(Deserialize)]
struct BenchmarkRow {
    model_permaslug: String,
    #[serde(default)]
    coding_index: Option<f64>,
    #[serde(default)]
    agentic_index: Option<f64>,
    #[serde(default)]
    pricing: Option<Pricing>,
}

#[derive(Deserialize)]
struct Pricing {
    #[serde(default)]
    prompt: Option<String>,
    #[serde(default)]
    completion: Option<String>,
}

pub fn cache_path(home: &Path) -> PathBuf {
    home.join("model-rankings.json")
}

/// Resolve `auto` to a concrete model id. Cache first, then the network; a
/// failure anywhere falls back to `fallback` so a data-API outage never stops
/// a session from starting.
pub fn resolve_auto(api_key: &str, tier: Tier, cache: &Path, fallback: &str) -> Result<Pick> {
    if let Some(pick) = pick_from_cache(cache, tier) {
        return Ok(pick);
    }
    match fetch_entries(api_key) {
        Ok(entries) if !entries.is_empty() => {
            let _ = write_cache(cache, &entries);
            Ok(pick_from_entries(&entries, tier, false)
                .unwrap_or_else(|| offline_pick(tier, fallback)))
        }
        _ => Ok(offline_pick(tier, fallback)),
    }
}

fn offline_pick(tier: Tier, fallback: &str) -> Pick {
    Pick {
        model: fallback.to_string(),
        tier,
        coding_index: 0.0,
        agentic_index: 0.0,
        blended_price_per_m: 0.0,
        from_cache: false,
    }
}

fn pick_from_cache(cache: &Path, tier: Tier) -> Option<Pick> {
    let raw = std::fs::read_to_string(cache).ok()?;
    let parsed: Cache = serde_json::from_str(&raw).ok()?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?;
    if now.saturating_sub(Duration::from_secs(parsed.fetched_at_secs)) > CACHE_TTL {
        return None;
    }
    pick_from_entries(&parsed.entries, tier, true)
}

fn pick_from_entries(entries: &[Entry], tier: Tier, from_cache: bool) -> Option<Pick> {
    let best = match tier {
        // Strong: raw capability, price no object.
        Tier::Strong => entries
            .iter()
            .filter(|e| e.coding_index > 0.0)
            .max_by(|a, b| score_strong(a).total_cmp(&score_strong(b))),
        // Cheap: best coding under the price ceiling.
        Tier::Cheap => entries
            .iter()
            .filter(|e| e.coding_index > 0.0 && e.blended_price_per_m <= CHEAP_PRICE_CEILING)
            .max_by(|a, b| a.coding_index.total_cmp(&b.coding_index)),
        // Balanced: best coding under a mid-range ceiling, so it lands between
        // the cheap pick and the strong one instead of on whatever is free.
        Tier::Balanced => entries
            .iter()
            .filter(|e| e.coding_index > 0.0 && e.blended_price_per_m <= BALANCED_PRICE_CEILING)
            .max_by(|a, b| a.coding_index.total_cmp(&b.coding_index)),
    }?;
    Some(Pick {
        model: best.slug.clone(),
        tier,
        coding_index: best.coding_index,
        agentic_index: best.agentic_index,
        blended_price_per_m: best.blended_price_per_m,
        from_cache,
    })
}

fn score_strong(e: &Entry) -> f64 {
    0.7 * e.coding_index + 0.3 * e.agentic_index
}

/// Runs on its own thread: callers sit inside tokio runtimes, and a blocking
/// client dropped on a runtime worker panics.
fn fetch_entries(api_key: &str) -> Result<Vec<Entry>> {
    let key = api_key.to_string();
    std::thread::scope(|scope| {
        scope
            .spawn(move || fetch_entries_blocking(&key))
            .join()
            .unwrap_or_else(|_| Err(anyhow::anyhow!("rankings fetch thread panicked")))
    })
}

fn fetch_entries_blocking(api_key: &str) -> Result<Vec<Entry>> {
    let rows: BenchmarksResponse = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(FETCH_TIMEOUT_SECS))
        .build()
        .context("building the rankings http client")?
        .get(BENCHMARKS_URL)
        .query(&[("max_results", MAX_CACHED_MODELS.to_string())])
        .bearer_auth(api_key)
        .send()
        .context("fetching model benchmarks from OpenRouter")?
        .error_for_status()
        .context("OpenRouter rejected the benchmarks request")?
        .json()
        .context("parsing the benchmarks response")?;
    Ok(rows
        .data
        .into_iter()
        .filter_map(row_to_entry)
        .take(MAX_CACHED_MODELS)
        .collect())
}

fn row_to_entry(row: BenchmarkRow) -> Option<Entry> {
    let coding_index = row.coding_index?;
    let pricing = row.pricing?;
    let prompt: f64 = pricing.prompt?.parse().ok()?;
    let completion: f64 = pricing.completion?.parse().ok()?;
    // Prices arrive as $/token strings; a 3:1 prompt:completion mix is the
    // shape agent traffic actually has.
    let blended = (3.0 * prompt + completion) / 4.0 * 1_000_000.0;
    Some(Entry {
        slug: row.model_permaslug,
        coding_index,
        agentic_index: row.agentic_index.unwrap_or(0.0),
        blended_price_per_m: blended,
    })
}

fn write_cache(cache: &Path, entries: &[Entry]) -> Result<()> {
    if let Some(parent) = cache.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    let fetched_at_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let payload = Cache {
        fetched_at_secs,
        entries: entries.to_vec(),
    };
    std::fs::write(cache, serde_json::to_string(&payload)?)
        .with_context(|| format!("writing {}", cache.display()))
}

/// Every tier's pick, for `aster models recommend`. One fetch serves all three.
pub fn recommend(api_key: &str, cache: &Path) -> Result<Vec<Pick>> {
    let entries = match read_cache_entries(cache) {
        Some(entries) => entries,
        None => {
            let entries = fetch_entries(api_key)?;
            let _ = write_cache(cache, &entries);
            entries
        }
    };
    if entries.is_empty() {
        bail!("OpenRouter returned no benchmark data; try again later");
    }
    Ok(Tier::ALL
        .iter()
        .filter_map(|tier| pick_from_entries(&entries, *tier, false))
        .collect())
}

fn read_cache_entries(cache: &Path) -> Option<Vec<Entry>> {
    let raw = std::fs::read_to_string(cache).ok()?;
    let parsed: Cache = serde_json::from_str(&raw).ok()?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?;
    (now.saturating_sub(Duration::from_secs(parsed.fetched_at_secs)) <= CACHE_TTL)
        .then_some(parsed.entries)
}

#[cfg(test)]
#[path = "router_tests.rs"]
mod tests;
