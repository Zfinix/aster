//! Review config (`aster.yaml`), loaded from the repo root or `~/.config/aster/`.

use std::path::Path;

use anyhow::{Context, Result};
use globset::{Glob, GlobSet, GlobSetBuilder};
use serde::Deserialize;

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub review: Review,
    pub permissions: aster_policy::PermissionsConfig,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Review {
    /// Fallback model when a stage override is unset.
    pub model: Option<String>,
    /// OpenAI-compatible endpoint.
    pub base_url: Option<String>,
    pub hypothesis_model: Option<String>,
    pub verify_model: Option<String>,
    /// Drop findings below this confidence (0.0-1.0).
    pub min_confidence: Option<f32>,
    pub max_diff_bytes: Option<usize>,
    /// Static analyzer backends, e.g. ["semgrep"].
    pub analyzers: Vec<String>,
    /// Repo-relative path to an ast-grep rule YAML for the `ast-grep` backend.
    pub astgrep_rules: Option<String>,
    /// Defect classes to bias the hypothesis pass toward.
    pub focus_areas: Vec<String>,
    /// Globs of files to review. Empty = everything (minus `exclude`).
    pub include: Vec<String>,
    /// Globs of files to never review.
    pub exclude: Vec<String>,
}

impl Settings {
    /// Load from `repo_root`, else the global config dir, else defaults. Malformed files error.
    pub fn load(repo_root: Option<&Path>) -> Result<Self> {
        if let Some(root) = repo_root {
            for name in ["aster.yaml", "aster.yml", ".aster.yaml"] {
                let path = root.join(name);
                if path.exists() {
                    return parse(&path);
                }
            }
        }
        if let Some(global) = dirs::config_dir().map(|d| d.join("aster/aster.yaml"))
            && global.exists()
        {
            return parse(&global);
        }
        Ok(Self::default())
    }
}

fn parse(path: &Path) -> Result<Settings> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    serde_yaml::from_str(&text).with_context(|| format!("parsing {}", path.display()))
}

/// Compiled include/exclude matcher for file paths.
pub struct PathFilter {
    include: Option<GlobSet>,
    exclude: GlobSet,
}

/// Generated/vendored files always excluded, on top of the user's `exclude`.
const DEFAULT_EXCLUDE: &[&str] = &[
    "**/*.lock",
    "**/package-lock.json",
    "**/pnpm-lock.yaml",
    "**/yarn.lock",
    "**/composer.lock",
    "**/Gemfile.lock",
    "**/Cargo.lock",
    "**/Pipfile.lock",
    "**/poetry.lock",
    "**/requirements.txt",
    "**/*.min.js",
    "**/*.min.css",
    "**/*.min.css.map",
    "**/*.min.js.map",
    "**/*.map",
    "**/*.snap",
    "**/dist/**",
    "**/build/**",
    "**/out/**",
    "**/node_modules/**",
    "**/vendor/**",
    "**/.git/**",
    "**/.hg/**",
    "**/.svn/**",
    "**/.DS_Store",
    "**/Thumbs.db",
    "**/*.class",
    "**/target/**",
    "**/*.pyc",
];

impl PathFilter {
    /// Empty `include` means everything. `exclude` is unioned with [`DEFAULT_EXCLUDE`].
    pub fn new(include: &[String], exclude: &[String]) -> Result<Self> {
        let mut excludes: Vec<String> = DEFAULT_EXCLUDE.iter().map(|s| s.to_string()).collect();
        excludes.extend(exclude.iter().cloned());
        Ok(Self {
            include: if include.is_empty() {
                None
            } else {
                Some(build(include)?)
            },
            exclude: build(&excludes)?,
        })
    }

    /// True when a path matches `include` (or include is empty) and no `exclude`.
    pub fn allows(&self, path: &str) -> bool {
        if self.exclude.is_match(path) {
            return false;
        }
        match &self.include {
            Some(set) => set.is_match(path),
            None => true,
        }
    }
}

fn build(patterns: &[String]) -> Result<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    for p in patterns {
        builder.add(Glob::new(p).with_context(|| format!("invalid glob: {p}"))?);
    }
    builder.build().context("building glob set")
}

/// Keep only the per-file sections of a unified diff whose target path passes `filter`.
pub fn filter_diff(diff: &str, filter: &PathFilter) -> String {
    let mut out = String::new();
    let mut keep = true;
    for line in diff.lines() {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            keep = rest
                .split_whitespace()
                .nth(1)
                .and_then(|b| b.strip_prefix("b/"))
                .map(|path| filter.allows(path))
                .unwrap_or(true);
        }
        if keep {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use aster_policy::{Action, Decision, Policy};

    #[test]
    fn permissions_absent_defaults_to_permissive_edits() {
        let s: Settings = serde_yaml::from_str("review: {}").expect("parse");
        let p = Policy::compile(&s.permissions).expect("compile");
        assert_eq!(
            p.evaluate(&Action::Edit {
                path: "src/main.rs"
            }),
            Decision::Allow
        );
        assert!(matches!(
            p.evaluate(&Action::Edit {
                path: ".git/hooks/pre-commit"
            }),
            Decision::Deny { .. }
        ));
    }

    #[test]
    fn permissions_block_parses_and_compiles() {
        let yaml = "\
permissions:
  mode: ask
  deny: [\"**/*.pem\"]
  allow: [\"src/**\"]
";
        let s: Settings = serde_yaml::from_str(yaml).expect("parse permissions block");
        let p = Policy::compile(&s.permissions).expect("compile");
        assert!(matches!(
            p.evaluate(&Action::Edit {
                path: "certs/key.pem"
            }),
            Decision::Deny { .. }
        ));
        // Ask mode falls through to a prompt for unmatched paths.
        assert!(matches!(
            p.evaluate(&Action::Edit {
                path: "docs/readme.md"
            }),
            Decision::Prompt { .. }
        ));
    }
}
