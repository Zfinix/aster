//! Glob path lookup: find files and directories by name instead of by content.

use std::path::Path;

use anyhow::{Context, Result, bail};
use globset::{Glob, GlobMatcher};
use ignore::WalkBuilder;

/// Files and directories under `base` whose repo-relative path or name matches
/// `pattern`, up to `max_hits`; directories end with `/`. A `.gitignore` entry
/// hides a path from the fast pass but must not make it unreachable, so an
/// empty filtered pass is retried without it.
pub fn find(repo_root: &Path, base: &Path, pattern: &str, max_hits: usize) -> Result<String> {
    let pattern = pattern.trim();
    if pattern.is_empty() {
        bail!("empty file pattern");
    }
    let matchers = matchers(pattern)?;

    let hits = walk(repo_root, base, &matchers, max_hits, true);
    if !hits.is_empty() {
        return Ok(hits.join("\n"));
    }

    let ignored = walk(repo_root, base, &matchers, max_hits, false);
    if ignored.is_empty() {
        return Ok(no_match(repo_root, pattern));
    }
    Ok(format!(
        "{}\n\n(ignored by .gitignore; matched only because nothing else did)",
        ignored.join("\n")
    ))
}

/// Collect matching repo-relative paths, sorted. `respect_ignore` drives the
/// two passes in [`find`]; the unfiltered one still skips [`crate::SKIP_DIRS`].
fn walk(
    repo_root: &Path,
    base: &Path,
    matchers: &[GlobMatcher],
    max_hits: usize,
    respect_ignore: bool,
) -> Vec<String> {
    let mut builder = WalkBuilder::new(base);
    builder
        .hidden(respect_ignore)
        .git_ignore(respect_ignore)
        .require_git(false)
        .git_exclude(respect_ignore);
    if !respect_ignore {
        builder.filter_entry(|entry| !crate::is_skipped(entry));
    }

    let mut hits: Vec<String> = Vec::new();
    for entry in builder.build().flatten() {
        if hits.len() >= max_hits {
            break;
        }
        let is_dir = entry.file_type().is_some_and(|t| t.is_dir());
        if entry.depth() == 0 || !(is_dir || entry.file_type().is_some_and(|t| t.is_file())) {
            continue;
        }
        let path = entry.path();
        let rel = path.strip_prefix(repo_root).unwrap_or(path);
        let name = path.file_name().unwrap_or_default();
        if matchers.iter().any(|m| m.is_match(rel) || m.is_match(name)) {
            let rel = rel.to_string_lossy();
            hits.push(match is_dir {
                true => format!("{rel}/"),
                false => rel.into_owned(),
            });
        }
    }
    hits.sort();
    hits
}

const MAX_NO_MATCH_SUGGESTIONS: usize = 5;

/// A wrong glob is usually a near-miss on a real name, so answer with the
/// closest ones instead of a dead end.
fn no_match(repo_root: &Path, pattern: &str) -> String {
    let target: String = pattern
        .rsplit('/')
        .next()
        .unwrap_or(pattern)
        .chars()
        .filter(|c| !matches!(c, '*' | '?' | '[' | ']' | '{' | '}'))
        .collect();
    let nearby = match target.len() < 2 {
        true => Vec::new(),
        false => crate::suggest(repo_root, &target, MAX_NO_MATCH_SUGGESTIONS),
    };
    if nearby.is_empty() {
        return "no files matched".into();
    }
    format!(
        "no files matched `{pattern}`. Files with similar names:\n{}",
        nearby
            .iter()
            .map(|p| format!("  {p}"))
            .collect::<Vec<_>>()
            .join("\n")
    )
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
#[path = "tests/find_test.rs"]
mod tests;
