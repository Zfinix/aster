//! Tiered file search: `rg` → embedded `grep`/`ignore` → hand-rolled walker.

use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::ToolProbe;

/// Directories never worth walking; mirrors the review path filter's defaults.
const SKIP_DIRS: &[&str] = &[
    ".git",
    "target",
    "node_modules",
    "dist",
    "build",
    "out",
    "vendor",
    ".hg",
    ".svn",
];

/// Search `base` for `query`, returning `path:line: text` lines up to
/// `max_hits`. Tries `rg`, then embedded ripgrep, then a hand-rolled walker.
pub fn search(
    probe: &ToolProbe,
    repo_root: &Path,
    base: &Path,
    query: &str,
    max_hits: usize,
) -> Result<String> {
    if query.trim().is_empty() {
        bail!("empty search query");
    }

    if let Some(rg) = &probe.rg {
        return search_with_rg(rg, repo_root, base, query, max_hits);
    }
    search_embedded(repo_root, base, query, max_hits)
}

/// Shell out to `rg`. Respects `.gitignore` natively.
fn search_with_rg(
    rg: &Path,
    repo_root: &Path,
    base: &Path,
    query: &str,
    max_hits: usize,
) -> Result<String> {
    let output = Command::new(rg)
        .args([
            "--line-number",
            "--no-heading",
            "--color",
            "never",
            "--max-count",
            &max_hits.to_string(),
        ])
        .arg(query)
        .arg(base)
        .current_dir(repo_root)
        .output()
        .context("running rg")?;

    if !output.status.success() && !output.status.code().is_some_and(|c| c == 1) {
        bail!(
            "rg failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().take(max_hits).collect();
    if lines.is_empty() {
        return Ok("no matches".into());
    }
    Ok(lines.join("\n"))
}

/// Embedded ripgrep via the `grep` and `ignore` crates. Respects
/// `.gitignore` and walks in parallel.
fn search_embedded(repo_root: &Path, base: &Path, query: &str, max_hits: usize) -> Result<String> {
    use grep::regex::RegexMatcher;
    use grep::searcher::Searcher;
    use grep::searcher::sinks::UTF8;
    use ignore::WalkBuilder;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    let matcher = Arc::new(
        RegexMatcher::new(query).with_context(|| format!("invalid regex pattern: {query}"))?,
    );
    let hits: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let count = Arc::new(AtomicUsize::new(0));

    WalkBuilder::new(base)
        .hidden(true)
        .git_ignore(true)
        .require_git(false)
        .git_exclude(true)
        .build_parallel()
        .run(|| {
            let matcher = Arc::clone(&matcher);
            let hits = Arc::clone(&hits);
            let count = Arc::clone(&count);
            Box::new(move |result| {
                if count.load(Ordering::Relaxed) >= max_hits {
                    return ignore::WalkState::Quit;
                }
                let Ok(entry) = result else {
                    return ignore::WalkState::Continue;
                };
                if !entry.file_type().is_some_and(|t| t.is_file()) {
                    return ignore::WalkState::Continue;
                }
                let path = entry.path();
                let rel = path
                    .strip_prefix(repo_root)
                    .unwrap_or(path)
                    .to_string_lossy()
                    .into_owned();
                let mut found: Vec<String> = Vec::new();
                let _ = Searcher::new().search_path(
                    matcher.as_ref(),
                    path,
                    UTF8(|line_no, text| {
                        found.push(format!("{rel}:{}: {}", line_no, text.trim_end()));
                        Ok(true)
                    }),
                );
                if !found.is_empty() {
                    let mut guard = hits.lock().unwrap();
                    for hit in &found {
                        if count.load(Ordering::Relaxed) >= max_hits {
                            break;
                        }
                        guard.push(hit.clone());
                        count.fetch_add(1, Ordering::Relaxed);
                    }
                    if count.load(Ordering::Relaxed) >= max_hits {
                        return ignore::WalkState::Quit;
                    }
                }
                ignore::WalkState::Continue
            })
        });

    let out = Arc::try_unwrap(hits)
        .map(|m| m.into_inner().unwrap())
        .unwrap_or_default();
    if out.is_empty() {
        return Ok("no matches".into());
    }
    Ok(out.join("\n"))
}

/// Hand-rolled fallback: `fs::read_dir` + substring match. No regex, no
/// `.gitignore`, but zero external dependencies beyond `std`.
#[allow(dead_code)]
fn search_manual(repo_root: &Path, base: &Path, query: &str, max_hits: usize) -> Result<String> {
    let needle = query.to_lowercase();
    let mut hits = Vec::new();
    let mut stack = vec![base.to_path_buf()];
    while let Some(current) = stack.pop() {
        if hits.len() >= max_hits {
            break;
        }
        let Ok(entries) = fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if path.is_dir() {
                if !name.starts_with('.') && !SKIP_DIRS.contains(&name.as_str()) {
                    stack.push(path);
                }
                continue;
            }
            let rel = path
                .strip_prefix(repo_root)
                .unwrap_or(&path)
                .to_string_lossy()
                .into_owned();
            let Ok(content) = fs::read_to_string(&path) else {
                continue;
            };
            for (no, line) in content.lines().enumerate() {
                if line.to_lowercase().contains(&needle) {
                    hits.push(format!("{rel}:{}: {}", no + 1, line.trim()));
                    if hits.len() >= max_hits {
                        break;
                    }
                }
            }
            if hits.len() >= max_hits {
                break;
            }
        }
    }
    if hits.is_empty() {
        return Ok("no matches".into());
    }
    Ok(hits.join("\n"))
}

#[cfg(test)]
#[path = "search_tests.rs"]
mod tests;
