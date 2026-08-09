//! Project instruction files. `AGENTS.md` is the cross-tool standard, so a
//! repo set up for another agent works here unchanged; the sibling files other
//! agents read are loaded too.
//!
//! The spec puts nested `AGENTS.md` files through a monorepo, nearest to the
//! edited file winning. Preloading all of them would spend context on rules
//! for directories a turn never touches, so only the root files are injected
//! and the rest are advertised as paths for the agent to read on demand.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use ignore::WalkBuilder;

/// Read in this order, so the file a repo most likely maintains for Aster is
/// read last and reads as the final word.
pub const INSTRUCTION_FILES: &[&str] = &["CLAUDE.md", "AGENTS.md", "ASTER.md"];
/// One oversized file must not crowd out the others.
const MAX_FILE_CHARS: usize = 12_000;
/// Total budget for root instructions. These sit in every request, so this is
/// a standing context cost rather than a one-off.
const MAX_TOTAL_CHARS: usize = 24_000;
/// Nested files listed by path. Beyond this the index itself is the cost the
/// bodies were meant to avoid.
const MAX_NESTED_LISTED: usize = 40;
/// How deep to look for nested instruction files.
const MAX_DEPTH: usize = 8;
/// Stop walking after this many entries; outside a repo (a launch from `~`,
/// say) the tree is effectively unbounded and this runs before the UI is up.
const MAX_WALK_ENTRIES: usize = 50_000;

#[derive(Debug, Default, Clone)]
pub struct Instructions {
    /// Loaded in full: the repo-wide contract.
    root: Vec<Loaded>,
    /// Advertised by path only, read when a turn works in that directory.
    nested: Vec<PathBuf>,
}

#[derive(Debug, Clone)]
struct Loaded {
    label: String,
    body: String,
    truncated: bool,
}

/// Load the root instruction files for `repo_root` plus any global ones, and
/// index the nested files without reading them.
pub fn discover(repo_root: &Path) -> Instructions {
    Instructions {
        root: load_root(repo_root),
        nested: find_nested(repo_root),
    }
}

fn load_root(repo_root: &Path) -> Vec<Loaded> {
    let mut files = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut budget = MAX_TOTAL_CHARS;

    for (label, path) in root_candidates(repo_root) {
        if budget == 0 {
            break;
        }
        let Ok(raw) = fs::read_to_string(&path) else {
            continue;
        };
        let body = raw.trim();
        if body.is_empty() {
            continue;
        }
        // The spec's own migration advice is to symlink the old name to
        // AGENTS.md, so the same text under two names is common.
        if !seen.insert(body.to_string()) {
            continue;
        }
        let (body, truncated) = clamp(body, MAX_FILE_CHARS.min(budget));
        budget = budget.saturating_sub(body.chars().count());
        files.push(Loaded {
            label,
            body,
            truncated,
        });
    }
    files
}

/// Global files first, then the repo's, so the repo has the last word.
fn root_candidates(repo_root: &Path) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    if let Ok(home) = crate::persist::home() {
        for name in INSTRUCTION_FILES {
            out.push((format!("{name} (global)"), home.join(name)));
        }
    }
    for name in INSTRUCTION_FILES {
        out.push(((*name).to_string(), repo_root.join(name)));
    }
    out
}

/// Instruction files below the root, respecting `.gitignore`. Only `AGENTS.md`
/// nests by spec; the other names are root-level conventions.
fn find_nested(repo_root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let walk = WalkBuilder::new(repo_root)
        .hidden(true)
        .git_ignore(true)
        .require_git(false)
        .max_depth(Some(MAX_DEPTH))
        .build();
    for entry in walk.take(MAX_WALK_ENTRIES).flatten() {
        if found.len() >= MAX_NESTED_LISTED {
            break;
        }
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        if entry.file_name() != "AGENTS.md" {
            continue;
        }
        let Ok(relative) = entry.path().strip_prefix(repo_root) else {
            continue;
        };
        // The root file is already loaded in full.
        if relative.parent().is_some_and(|p| p.as_os_str().is_empty()) {
            continue;
        }
        found.push(relative.to_path_buf());
    }
    found.sort();
    found
}

fn clamp(body: &str, cap: usize) -> (String, bool) {
    if body.chars().count() <= cap {
        return (body.to_string(), false);
    }
    (body.chars().take(cap).collect(), true)
}

impl Instructions {
    pub fn is_empty(&self) -> bool {
        self.root.is_empty() && self.nested.is_empty()
    }

    /// The root files that were loaded, for the startup banner.
    #[allow(dead_code)] // used in tests
    pub fn labels(&self) -> Vec<String> {
        self.root.iter().map(|f| f.label.clone()).collect()
    }

    #[allow(dead_code)] // used in tests
    pub fn nested(&self) -> &[PathBuf] {
        &self.nested
    }

    /// The instruction file governing `path`: the nearest one at or above it,
    /// which is the precedence the spec defines.
    pub fn nearest(&self, path: &Path) -> Option<&Path> {
        self.nested
            .iter()
            .filter(|file| {
                file.parent()
                    .is_some_and(|dir| path.starts_with(dir) && !dir.as_os_str().is_empty())
            })
            .max_by_key(|file| file.components().count())
            .map(PathBuf::as_path)
    }

    /// The system-prompt section, or `None` when the repo has no instructions.
    pub fn render(&self) -> Option<String> {
        if self.is_empty() {
            return None;
        }
        let mut out = String::from(
            "## Project instructions\n\n\
             These come from this repository. Follow them: where they conflict with \
             your defaults, they win. Where they conflict with the user's direct \
             request, the user wins.\n",
        );
        for file in &self.root {
            out.push_str(&format!("\n### {}\n\n{}\n", file.label, file.body));
            if file.truncated {
                out.push_str("\n[truncated; read the file directly if you need the rest]\n");
            }
        }
        if !self.nested.is_empty() {
            out.push_str(
                "\n### Directory instructions\n\n\
                 These directories carry their own instructions, not loaded here. \
                 Before you edit or add a file under one, `read_file` the nearest \
                 listed file at or above it; the nearest one wins over the rules \
                 above.\n",
            );
            for path in &self.nested {
                out.push_str(&format!("- {}\n", path.display()));
            }
            if self.nested.len() >= MAX_NESTED_LISTED {
                out.push_str("\nThe list is capped; `find_files AGENTS.md` for the rest.\n");
            }
        }
        Some(out)
    }
}

#[cfg(test)]
#[path = "tests/instructions_test.rs"]
mod tests;
