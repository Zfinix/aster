//! Update check against the GitHub releases feed, cached for a day so a
//! launch normally costs no network round trip.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;

const REPO: &str = "Zfinix/aster";
const CACHE_TTL: Duration = Duration::from_secs(24 * 60 * 60);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_CHANGELOG: usize = 10;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateInfo {
    pub current: String,
    pub latest: String,
    pub url: String,
    pub changelog: Vec<String>,
}

#[derive(Serialize, Deserialize)]
struct Cache {
    checked_at: u64,
    update: Option<UpdateInfo>,
}

/// The newer release to announce, if one exists. Silent on any failure:
/// an update notice is never worth an error at startup.
pub async fn check() -> Option<UpdateInfo> {
    if std::env::var_os("ASTER_NO_UPDATE_CHECK").is_some() {
        return None;
    }
    let current = env!("CARGO_PKG_VERSION");
    if let Some(cache) = read_cache()
        && now().saturating_sub(cache.checked_at) < CACHE_TTL.as_secs()
    {
        return cache.update.filter(|u| is_newer(&u.latest, current));
    }
    let update = match fetch(current).await {
        Ok(update) => update,
        Err(e) => {
            tracing::debug!("update check failed: {e:#}");
            return None;
        }
    };
    write_cache(&Cache {
        checked_at: now(),
        update: update.clone(),
    });
    update
}

fn client() -> anyhow::Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(15))
        .user_agent(concat!("aster/", env!("CARGO_PKG_VERSION")))
        .build()?)
}

async fn fetch(current: &str) -> anyhow::Result<Option<UpdateInfo>> {
    let client = client()?;
    let releases: Vec<Value> = client
        .get(format!(
            "https://api.github.com/repos/{REPO}/releases?per_page=20"
        ))
        .timeout(REQUEST_TIMEOUT)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let latest = releases
        .iter()
        .filter_map(|r| {
            let tag = r.get("tag_name")?.as_str()?;
            Some((version_triple(tag)?, r))
        })
        .max_by_key(|(version, _)| *version);
    let Some((latest_version, release)) = latest else {
        return Ok(None);
    };
    let Some(current_version) = version_triple(current) else {
        return Ok(None);
    };
    if latest_version <= current_version {
        return Ok(None);
    }

    let tag = release["tag_name"].as_str().unwrap_or_default().to_string();
    let url = release["html_url"].as_str().unwrap_or_default().to_string();
    let mut changelog: Vec<String> = release["body"]
        .as_str()
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .take(MAX_CHANGELOG)
        .map(str::to_string)
        .collect();
    if changelog.is_empty()
        && let Some(current_tag) = tag_for(&releases, current_version)
    {
        changelog = compare_commits(&client, &current_tag, &tag)
            .await
            .unwrap_or_default();
    }
    Ok(Some(UpdateInfo {
        current: current.to_string(),
        latest: trimmed_version(&tag),
        url,
        changelog,
    }))
}

async fn compare_commits(
    client: &reqwest::Client,
    from: &str,
    to: &str,
) -> anyhow::Result<Vec<String>> {
    let compare: Value = client
        .get(format!(
            "https://api.github.com/repos/{REPO}/compare/{from}...{to}"
        ))
        .timeout(REQUEST_TIMEOUT)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let mut subjects: Vec<String> = compare["commits"]
        .as_array()
        .map(|commits| {
            commits
                .iter()
                .filter_map(|c| c["commit"]["message"].as_str())
                .filter_map(|m| m.lines().next())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    subjects.reverse();
    let total = compare["total_commits"]
        .as_u64()
        .unwrap_or(subjects.len() as u64) as usize;
    if total > MAX_CHANGELOG {
        subjects.truncate(MAX_CHANGELOG);
        subjects.push(format!("+{} more commits", total - MAX_CHANGELOG));
    }
    Ok(subjects)
}

fn tag_for(releases: &[Value], version: (u64, u64, u64)) -> Option<String> {
    releases.iter().find_map(|r| {
        let tag = r.get("tag_name")?.as_str()?;
        (version_triple(tag)? == version).then(|| tag.to_string())
    })
}

/// The numeric triple in a tag or version string, however it is prefixed
/// (`v0.2.0`, `cli-v0.3.0`, `0.3.0`).
pub(crate) fn version_triple(text: &str) -> Option<(u64, u64, u64)> {
    let start = text.find(|c: char| c.is_ascii_digit())?;
    let mut parts = text[start..].split('.');
    Some((
        leading_number(parts.next()?)?,
        parts.next().and_then(leading_number).unwrap_or(0),
        parts.next().and_then(leading_number).unwrap_or(0),
    ))
}

fn leading_number(part: &str) -> Option<u64> {
    let digits: String = part.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

fn is_newer(candidate: &str, current: &str) -> bool {
    match (version_triple(candidate), version_triple(current)) {
        (Some(a), Some(b)) => a > b,
        _ => false,
    }
}

fn trimmed_version(tag: &str) -> String {
    match tag.find(|c: char| c.is_ascii_digit()) {
        Some(start) => tag[start..].to_string(),
        None => tag.to_string(),
    }
}

fn cache_path() -> Option<std::path::PathBuf> {
    Some(dirs::home_dir()?.join(".aster/update-check.json"))
}

fn read_cache() -> Option<Cache> {
    let text = std::fs::read_to_string(cache_path()?).ok()?;
    serde_json::from_str(&text).ok()
}

fn write_cache(cache: &Cache) {
    let Some(path) = cache_path() else { return };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(text) = serde_json::to_string_pretty(cache) {
        let _ = std::fs::write(path, text);
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
#[path = "tests/update_test.rs"]
mod tests;
