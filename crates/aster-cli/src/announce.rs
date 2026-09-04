//! Release announcements: short, user-facing lines generated per release,
//! fetched from the latest GitHub release and shown once per id.

use clap::Args;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const REPO: &str = "Zfinix/aster";
const CACHE_TTL: Duration = Duration::from_secs(24 * 60 * 60);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_ITEMS: usize = 5;
const MAX_TEXT_CHARS: usize = 200;

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Announcement {
    pub id: String,
    pub text: String,
}

#[derive(Serialize, Deserialize)]
struct Feed {
    items: Vec<Announcement>,
}

#[derive(Serialize, Deserialize)]
struct FetchCache {
    fetched_at: u64,
    items: Vec<Announcement>,
}

#[derive(Args, Debug)]
pub struct AnnounceArgs {
    /// Record these comma-separated announcement ids as seen.
    #[arg(long, value_delimiter = ',')]
    dismiss: Vec<String>,
    /// Accepted for callers that ask for machine-readable output; the
    /// command always prints JSON.
    #[arg(long)]
    json: bool,
}

/// Undismissed announcements for this user, newest release first. Silent on
/// any failure: an announcement is never worth an error.
pub async fn pending() -> Vec<Announcement> {
    let items = fetch().await.unwrap_or_default();
    let dismissed = dismissed_ids();
    items
        .into_iter()
        .filter(|a| !dismissed.contains(&a.id))
        .collect()
}

async fn fetch() -> Option<Vec<Announcement>> {
    if std::env::var_os("ASTER_NO_UPDATE_CHECK").is_some() {
        return None;
    }
    if let Some(cache) = read_fetch_cache()
        && now().saturating_sub(cache.fetched_at) < CACHE_TTL.as_secs()
    {
        return Some(cache.items);
    }
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .user_agent(concat!("aster/", env!("CARGO_PKG_VERSION")))
        .build()
        .ok()?;
    let feed: Feed = client
        .get(format!(
            "https://github.com/{REPO}/releases/latest/download/announcements.json"
        ))
        .timeout(REQUEST_TIMEOUT)
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?
        .json()
        .await
        .ok()?;
    let items = sanitize(feed.items);
    write_fetch_cache(&items);
    Some(items)
}

fn sanitize(items: Vec<Announcement>) -> Vec<Announcement> {
    items
        .into_iter()
        .filter(|a| !a.id.is_empty() && !a.text.is_empty())
        .map(|a| Announcement {
            id: a.id,
            text: a.text.chars().take(MAX_TEXT_CHARS).collect(),
        })
        .take(MAX_ITEMS)
        .collect()
}

fn dismissed_path() -> Option<std::path::PathBuf> {
    Some(dirs::home_dir()?.join(".aster/announcements-dismissed.json"))
}

fn dismissed_ids() -> Vec<String> {
    dismissed_path()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// Record ids as seen. Already-dismissed ids are kept, so a re-dismiss is a
/// no-op and the store never shrinks behind a concurrent reader.
pub fn dismiss(ids: &[String]) {
    let Some(path) = dismissed_path() else { return };
    let mut all = dismissed_ids();
    for id in ids {
        if !id.is_empty() && !all.contains(id) {
            all.push(id.clone());
        }
    }
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(text) = serde_json::to_string_pretty(&all) {
        let _ = std::fs::write(path, text);
    }
}

fn fetch_cache_path() -> Option<std::path::PathBuf> {
    Some(dirs::home_dir()?.join(".aster/announcements.json"))
}

fn read_fetch_cache() -> Option<FetchCache> {
    let text = std::fs::read_to_string(fetch_cache_path()?).ok()?;
    serde_json::from_str(&text).ok()
}

fn write_fetch_cache(items: &[Announcement]) {
    let Some(path) = fetch_cache_path() else {
        return;
    };
    let cache = FetchCache {
        fetched_at: now(),
        items: items.to_vec(),
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(text) = serde_json::to_string_pretty(&cache) {
        let _ = std::fs::write(path, text);
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// `aster announce` prints undismissed announcements as JSON for the serve
/// backend and the VS Code extension; `--dismiss` records ids as seen.
pub async fn run(args: AnnounceArgs) -> anyhow::Result<()> {
    if !args.dismiss.is_empty() {
        dismiss(&args.dismiss);
        return Ok(());
    }
    let items = pending().await;
    let out = json!({ "items": items });
    println!("{out}");
    Ok(())
}

#[cfg(test)]
#[path = "tests/announce_test.rs"]
mod tests;
