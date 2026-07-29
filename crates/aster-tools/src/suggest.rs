//! Near-miss path suggestions, so a wrong guess points at the right file.

use std::path::Path;

use ignore::WalkBuilder;

/// Entries walked before giving up; a suggestion is a courtesy, not a search.
const WALK_CAP: usize = 20_000;

/// Repo-relative paths that resemble `missing`, best first. Matching is on the
/// last component: a wrong guess is usually the right file name in the wrong
/// directory.
pub fn suggest(repo_root: &Path, missing: &str, max: usize) -> Vec<String> {
    let target = missing
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or(missing)
        .to_lowercase();
    if target.is_empty() {
        return Vec::new();
    }
    let wanted: Vec<&str> = missing.split('/').filter(|c| !c.is_empty()).collect();

    let mut scored: Vec<(u8, usize, String)> = Vec::new();
    for entry in WalkBuilder::new(repo_root)
        .hidden(true)
        .git_ignore(true)
        .require_git(false)
        .git_exclude(true)
        .build()
        .flatten()
        .take(WALK_CAP)
    {
        let path = entry.path();
        let Ok(rel) = path.strip_prefix(repo_root) else {
            continue;
        };
        if rel.as_os_str().is_empty() {
            continue;
        }
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        let Some(score) = score(&name.to_lowercase(), &target) else {
            continue;
        };
        let rel = rel.to_string_lossy().into_owned();
        let shared = wanted.iter().filter(|c| rel.contains(**c)).count();
        scored.push((score, usize::MAX - shared, rel));
    }

    scored.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
    scored.truncate(max);
    scored.into_iter().map(|(_, _, path)| path).collect()
}

/// Lower is closer. `None` means too far apart to be worth suggesting.
fn score(name: &str, target: &str) -> Option<u8> {
    if name == target {
        return Some(0);
    }
    if name.contains(target) || target.contains(name) {
        return Some(1);
    }
    let stem = |s: &str| s.split('.').next().unwrap_or(s).to_string();
    if stem(name) == stem(target) {
        return Some(2);
    }
    None
}

#[cfg(test)]
#[path = "suggest_tests.rs"]
mod tests;
