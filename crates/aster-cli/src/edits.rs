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
        0 => {
            bail!("search text not found in the file (it must match exactly, whitespace included)")
        }
        1 => Ok(content.replacen(&block.search, &block.replace, 1)),
        n => bail!("search text matches {n} locations; add surrounding lines to make it unique"),
    }
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

/// `~` and `~/rest` become the home directory. A bare `~user` is left alone;
/// resolving another account's home is not something the agent should guess at.
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
/// [`resolve_in_repo`] cannot: canonicalizing a missing path always fails. The
/// nearest existing ancestor is canonicalized instead, so a symlinked directory
/// still cannot place the new file outside the repo.
pub fn resolve_new_in_repo(repo_root: &Path, path: &str) -> Result<PathBuf> {
    let root = repo_root
        .canonicalize()
        .with_context(|| format!("resolving repo root {}", repo_root.display()))?;
    let relative = Path::new(path);
    if !relative
        .components()
        .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
    {
        bail!("path must be repo-relative and stay inside the repository: {path}");
    }

    let target = root.join(relative);
    let mut existing = target.as_path();
    while !existing.exists() {
        existing = existing
            .parent()
            .with_context(|| format!("no directory to create {path} in"))?;
    }
    if !existing
        .canonicalize()
        .with_context(|| format!("resolving {}", existing.display()))?
        .starts_with(&root)
    {
        bail!("path escapes the repository: {path}");
    }
    Ok(target)
}

pub fn read_repo_file(repo_root: &Path, path: &str) -> Result<(PathBuf, String)> {
    let resolved = resolve_in_repo(repo_root, path)?;
    let content =
        fs::read_to_string(&resolved).with_context(|| format!("reading {}", resolved.display()))?;
    Ok((resolved, content))
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
mod tests {
    use super::*;

    const REPLY: &str = "<<<<<<< SEARCH\nlet a = 1;\n=======\nlet a = 2;\n>>>>>>> REPLACE\n";

    #[test]
    fn resolve_anywhere_classifies_in_repo_and_outside() {
        let repo = tempfile::tempdir().unwrap();
        fs::write(repo.path().join("inside.rs"), "").unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("elsewhere.txt"), "").unwrap();

        let (_, scope) = resolve_anywhere(repo.path(), "inside.rs").unwrap();
        assert_eq!(scope, Scope::InRepo);

        let path = outside.path().join("elsewhere.txt");
        let (resolved, scope) = resolve_anywhere(repo.path(), &path.to_string_lossy()).unwrap();
        assert_eq!(scope, Scope::Outside);
        assert_eq!(resolved, path.canonicalize().unwrap());
    }

    #[test]
    fn resolve_anywhere_expands_a_leading_tilde() {
        let repo = tempfile::tempdir().unwrap();
        let home = dirs::home_dir().unwrap();
        let (resolved, scope) = resolve_anywhere(repo.path(), "~").unwrap();
        assert_eq!(resolved, home.canonicalize().unwrap());
        assert_eq!(scope, Scope::Outside);
    }

    #[test]
    fn resolve_new_accepts_a_missing_nested_path() {
        let repo = tempfile::tempdir().unwrap();
        let target = resolve_new_in_repo(repo.path(), "src/deep/new.rs").unwrap();
        assert!(target.ends_with("src/deep/new.rs"));
        assert!(!target.exists());
    }

    #[test]
    fn resolve_new_rejects_paths_leaving_the_repo() {
        let repo = tempfile::tempdir().unwrap();
        for path in ["../outside.rs", "src/../../outside.rs", "/etc/passwd"] {
            assert!(resolve_new_in_repo(repo.path(), path).is_err(), "{path}");
        }
    }

    #[test]
    fn parse_blocks_single_block() {
        let blocks = parse_blocks(REPLY).unwrap();
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].search, "let a = 1;");
        assert_eq!(blocks[0].replace, "let a = 2;");
    }

    #[test]
    fn parse_blocks_tolerates_surrounding_prose() {
        let reply = format!("Here is the fix:\n```\n{REPLY}```\ndone");
        assert_eq!(parse_blocks(&reply).unwrap().len(), 1);
    }

    #[test]
    fn parse_blocks_unterminated_fails() {
        assert!(parse_blocks("<<<<<<< SEARCH\nx\n=======\ny\n").is_err());
    }

    #[test]
    fn apply_block_replaces_unique_match() {
        let block = EditBlock {
            search: "b".into(),
            replace: "c".into(),
        };
        assert_eq!(apply_block("a b", &block).unwrap(), "a c");
    }

    #[test]
    fn apply_block_ambiguous_match_fails() {
        let block = EditBlock {
            search: "a".into(),
            replace: "c".into(),
        };
        assert!(apply_block("a a", &block).is_err());
    }
}
