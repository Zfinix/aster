//! Glob file lookup: find paths by name instead of by content.

use std::path::Path;

use anyhow::{Context, Result, bail};
use globset::{Glob, GlobMatcher};
use ignore::WalkBuilder;

/// Find files under `base` whose repo-relative path or file name matches
/// `pattern`, returning repo-relative paths up to `max_hits`.
pub fn find(repo_root: &Path, base: &Path, pattern: &str, max_hits: usize) -> Result<String> {
    let pattern = pattern.trim();
    if pattern.is_empty() {
        bail!("empty file pattern");
    }
    let matchers = matchers(pattern)?;

    let mut hits: Vec<String> = Vec::new();
    for entry in WalkBuilder::new(base)
        .hidden(true)
        .git_ignore(true)
        .require_git(false)
        .git_exclude(true)
        .build()
        .flatten()
    {
        if hits.len() >= max_hits {
            break;
        }
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let path = entry.path();
        let rel = path.strip_prefix(repo_root).unwrap_or(path);
        let name = path.file_name().unwrap_or_default();
        if matchers.iter().any(|m| m.is_match(rel) || m.is_match(name)) {
            hits.push(rel.to_string_lossy().into_owned());
        }
    }

    if hits.is_empty() {
        return Ok("no files matched".into());
    }
    hits.sort();
    Ok(hits.join("\n"))
}

/// A bare name like `chat.rs` should match at any depth, so every pattern
/// without a leading `**/` gets that variant too.
fn matchers(pattern: &str) -> Result<Vec<GlobMatcher>> {
    let build = |p: &str| {
        Glob::new(p)
            .map(|g| g.compile_matcher())
            .with_context(|| format!("invalid file pattern: {p}"))
    };
    let mut out = vec![build(pattern)?];
    if !pattern.starts_with("**/") {
        out.push(build(&format!("**/{pattern}"))?);
    }
    Ok(out)
}

#[cfg(test)]
#[path = "find_tests.rs"]
mod tests;
