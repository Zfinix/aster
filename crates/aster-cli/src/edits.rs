//! SEARCH/REPLACE edit blocks shared by `aster fix` and the chat agent's `edit_file` tool.
//! Exact-match, apply-once semantics keep model edits auditable.

use std::fs;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail};

#[derive(Debug, Clone)]
pub struct EditBlock {
    pub search: String,
    pub replace: String,
}

const SEARCH_MARK: &str = "<<<<<<< SEARCH";
const DIVIDER_MARK: &str = "=======";
const REPLACE_MARK: &str = ">>>>>>> REPLACE";

/// Parse SEARCH/REPLACE blocks from a reply, tolerating surrounding prose and fences.
pub fn parse_blocks(reply: &str) -> Result<Vec<EditBlock>> {
    let mut blocks = Vec::new();
    let mut search: Option<Vec<&str>> = None;
    let mut replace: Option<Vec<&str>> = None;

    for line in reply.lines() {
        let trimmed = line.trim_end();
        if trimmed == SEARCH_MARK {
            if search.is_some() {
                bail!("malformed edit: nested SEARCH block");
            }
            search = Some(Vec::new());
        } else if trimmed == DIVIDER_MARK && search.is_some() && replace.is_none() {
            replace = Some(Vec::new());
        } else if trimmed == REPLACE_MARK {
            let (Some(s), Some(r)) = (search.take(), replace.take()) else {
                bail!("malformed edit: REPLACE marker without a SEARCH block");
            };
            blocks.push(EditBlock {
                search: s.join("\n"),
                replace: r.join("\n"),
            });
        } else if let Some(r) = replace.as_mut() {
            r.push(line);
        } else if let Some(s) = search.as_mut() {
            s.push(line);
        }
    }
    if search.is_some() || replace.is_some() {
        bail!("malformed edit: unterminated SEARCH/REPLACE block");
    }
    if blocks.is_empty() {
        bail!("no SEARCH/REPLACE blocks in the reply");
    }
    Ok(blocks)
}

/// Apply one block to `content`. The search text must match exactly once.
pub fn apply_block(content: &str, block: &EditBlock) -> Result<String> {
    let hits = content.matches(&block.search).count();
    match hits {
        0 => match closest_region(content, &block.search) {
            Some(region) => bail!(
                "search text not found in the file (it must match exactly, \
                 whitespace included). {region}"
            ),
            None => bail!(
                "search text not found in the file (it must match exactly, \
                 whitespace included); nothing similar found either, so check \
                 the path and re-read the file"
            ),
        },
        1 => Ok(content.replacen(&block.search, &block.replace, 1)),
        n => bail!("search text matches {n} locations; add surrounding lines to make it unique"),
    }
}

/// Extra file lines shown either side of the best match, so the caller can
/// re-anchor without another read.
const REGION_CONTEXT_LINES: usize = 5;
/// Bigram-similarity floor below which a "closest" line is just noise.
const MIN_SIMILARITY: f64 = 0.5;

/// The file region most similar to a failed search, rendered with line
/// numbers. Retrying a mismatched edit verbatim was the most common wasted
/// round in transcript studies; embedding the real text removes the extra
/// read the retry depends on.
fn closest_region(content: &str, search: &str) -> Option<String> {
    let anchor = search.lines().find(|l| !l.trim().is_empty())?.trim();
    let lines: Vec<&str> = content.lines().collect();
    let (best, score) = lines
        .iter()
        .enumerate()
        .map(|(i, line)| (i, similarity(anchor, line.trim())))
        .max_by(|a, b| a.1.total_cmp(&b.1))?;
    if score < MIN_SIMILARITY {
        return None;
    }
    let span = search.lines().count() + REGION_CONTEXT_LINES;
    let start = best.saturating_sub(REGION_CONTEXT_LINES);
    let end = (best + span).min(lines.len());
    let mut region = format!("Closest match in the file (lines {}-{}):\n", start + 1, end);
    for (i, line) in lines[start..end].iter().enumerate() {
        region.push_str(&format!("{:>5} | {line}\n", start + i + 1));
    }
    region.push_str("Re-issue the edit copying the exact text from this snippet.");
    Some(region)
}

/// Dice coefficient over character bigrams; whitespace-insensitive enough to
/// survive indentation drift, cheap enough to run per line.
fn similarity(a: &str, b: &str) -> f64 {
    let bigrams = |s: &str| -> Vec<(char, char)> {
        let chars: Vec<char> = s.chars().filter(|c| !c.is_whitespace()).collect();
        chars.windows(2).map(|w| (w[0], w[1])).collect()
    };
    let (a, b) = (bigrams(a), bigrams(b));
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let mut b_pool = b.clone();
    let shared = a
        .iter()
        .filter(|bg| {
            b_pool
                .iter()
                .position(|x| x == *bg)
                .inspect(|&i| {
                    b_pool.swap_remove(i);
                })
                .is_some()
        })
        .count();
    (2.0 * shared as f64) / (a.len() + b.len()) as f64
}

/// Resolve `path` inside `repo_root`, rejecting anything that escapes it (`..`, symlinks, absolute).
pub fn resolve_in_repo(repo_root: &Path, path: &str) -> Result<PathBuf> {
    let root = repo_root
        .canonicalize()
        .with_context(|| format!("resolving repo root {}", repo_root.display()))?;
    let joined = root.join(path);
    let resolved = joined
        .canonicalize()
        .with_context(|| format!("no such file in the repo: {path}"))?;
    if !resolved.starts_with(&root) {
        bail!("path escapes the repository: {path}");
    }
    Ok(resolved)
}

/// Where a resolved path landed relative to the repository.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// Inside the repo. The policy's globs apply to the repo-relative path.
    InRepo,
    /// Outside the repo. Reachable only with the user's per-path approval.
    Outside,
}

/// Resolve `path` to something that exists, expanding a leading `~`. Unlike
/// [`resolve_in_repo`] this accepts paths outside the repository and reports
/// where they landed, so the caller can gate them instead of failing outright.
pub fn resolve_anywhere(repo_root: &Path, path: &str) -> Result<(PathBuf, Scope)> {
    let root = repo_root
        .canonicalize()
        .with_context(|| format!("resolving repo root {}", repo_root.display()))?;
    let expanded = expand_home(path);
    let joined = if expanded.is_absolute() {
        expanded
    } else {
        root.join(expanded)
    };
    let resolved = joined
        .canonicalize()
        .with_context(|| format!("no such file or directory: {path}"))?;
    let scope = if resolved.starts_with(&root) {
        Scope::InRepo
    } else {
        Scope::Outside
    };
    Ok((resolved, scope))
}

/// Whether `path` resolves to something that exists, under the same rules as
/// [`resolve_anywhere`]. Lets a caller answer a wrong guess with a hint
/// instead of an error.
pub fn exists_anywhere(repo_root: &Path, path: &str) -> bool {
    let expanded = expand_home(path);
    if expanded.is_absolute() {
        return expanded.exists();
    }
    repo_root.join(expanded).exists()
}

/// `~` and `~/rest` become the home directory. A bare `~user` is left alone;
/// resolving another account's home is not something the agent should guess at.
/// A path with the home directory written back as `~`, for prompts and errors.
pub fn display_home(path: &Path) -> String {
    let shown = path.display().to_string();
    match dirs::home_dir() {
        Some(home) => match path.strip_prefix(&home) {
            Ok(rest) => format!("~/{}", rest.display()),
            Err(_) => shown,
        },
        None => shown,
    }
}

pub fn expand_home(path: &str) -> PathBuf {
    let Some(rest) = path.strip_prefix('~') else {
        return PathBuf::from(path);
    };
    if !rest.is_empty() && !rest.starts_with('/') {
        return PathBuf::from(path);
    }
    match dirs::home_dir() {
        Some(home) => home.join(rest.trim_start_matches('/')),
        None => PathBuf::from(path),
    }
}

/// Resolve `path` for a file that does not exist yet, so it can be created.
/// [`resolve_anywhere`] cannot: canonicalizing a missing path always fails. The
/// nearest existing ancestor is canonicalized instead and the missing tail
/// rebuilt under it, so a symlink cannot move the write out of the directory
/// the caller gates on.
pub fn resolve_new_anywhere(repo_root: &Path, path: &str) -> Result<(PathBuf, Scope)> {
    let root = repo_root
        .canonicalize()
        .with_context(|| format!("resolving repo root {}", repo_root.display()))?;
    let expanded = expand_home(path);
    let joined = if expanded.is_absolute() {
        expanded
    } else {
        root.join(expanded)
    };

    let mut existing = joined.as_path();
    while !existing.exists() {
        existing = existing
            .parent()
            .with_context(|| format!("no directory to create {path} in"))?;
    }
    let mut target = existing
        .canonicalize()
        .with_context(|| format!("resolving {}", existing.display()))?;
    // The tail is folded onto the canonical anchor rather than joined: a `..`
    // in a part that does not exist yet has nothing to resolve against, and
    // joining it would leave the escape for `starts_with` to miss.
    for part in joined
        .strip_prefix(existing)
        .unwrap_or(Path::new(""))
        .components()
    {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                target.pop();
            }
            other => target.push(other),
        }
    }

    let scope = if target.starts_with(&root) {
        Scope::InRepo
    } else {
        Scope::Outside
    };
    Ok((target, scope))
}

pub fn read_repo_file(repo_root: &Path, path: &str) -> Result<(PathBuf, String)> {
    let resolved = resolve_in_repo(repo_root, path)?;
    let content =
        fs::read_to_string(&resolved).with_context(|| format!("reading {}", resolved.display()))?;
    Ok((resolved, content))
}

/// As [`read_repo_file`], but reports where the path landed instead of failing
/// on anything outside the repository.
pub fn read_file_anywhere(repo_root: &Path, path: &str) -> Result<(PathBuf, Scope, String)> {
    let (resolved, scope) = resolve_anywhere(repo_root, path)?;
    let content =
        fs::read_to_string(&resolved).with_context(|| format!("reading {}", resolved.display()))?;
    Ok((resolved, scope, content))
}

/// A compact ±diff preview of one block, for dry-runs and logs.
pub fn preview(block: &EditBlock) -> String {
    let mut out = String::new();
    for line in block.search.lines() {
        out.push_str("- ");
        out.push_str(line);
        out.push('\n');
    }
    for line in block.replace.lines() {
        out.push_str("+ ");
        out.push_str(line);
        out.push('\n');
    }
    out
}

#[cfg(test)]
#[path = "tests/edits_test.rs"]
mod tests;
