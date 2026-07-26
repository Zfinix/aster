//! SEARCH/REPLACE edit blocks shared by `aster fix` and the chat agent's `edit_file` tool.
//! Exact-match, apply-once semantics keep model edits auditable.

use std::fs;
use std::path::{Path, PathBuf};

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
