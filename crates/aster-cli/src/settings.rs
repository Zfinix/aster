//! Review config (`aster.yaml`), loaded from the repo root or `~/.aster/`.

use std::path::Path;

use anyhow::{Context, Result};
use globset::{Glob, GlobSet, GlobSetBuilder};
use serde::Deserialize;

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub review: Review,
    pub permissions: aster_policy::PermissionsConfig,
    pub mcp: crate::mcp::McpSettings,
    pub agent: Agent,
}

/// Limits on one agent turn. Both are also settable per run via
/// `ASTER_MAX_TOOL_ROUNDS` and `ASTER_COMMAND_TIMEOUT`.
#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Agent {
    /// Tool call rounds before the agent is forced to answer with what it has.
    pub max_tool_rounds: Option<usize>,
    /// Seconds a `run_command` call may run before it is killed.
    pub command_timeout_secs: Option<u64>,
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
    /// Reasoning budget for thinking models: off, low, medium, or high.
    pub effort: Option<aster_ai::Effort>,
    /// Globs of files to review. Empty = everything (minus `exclude`).
    pub include: Vec<String>,
    /// Globs of files to never review.
    pub exclude: Vec<String>,
}

impl Settings {
    /// The global config, then the repo's, with the repo's on top. Malformed
    /// files error rather than being skipped, so a typo is never silent.
    pub fn load(repo_root: Option<&Path>) -> Result<Self> {
        let global = match dirs::home_dir().map(|h| h.join(".aster/aster.yaml")) {
            Some(path) if path.exists() => Some(parse(&path)?),
            _ => None,
        };
        let project = repo_root
            .and_then(|root| {
                ["aster.yaml", "aster.yml", ".aster.yaml"]
                    .iter()
                    .map(|name| root.join(name))
                    .find(|path| path.exists())
            })
            .map(|path| parse(&path))
            .transpose()?;

        match (global, project) {
            (Some(global), Some(project)) => Ok(global.overlaid_with(project)),
            (Some(only), None) | (None, Some(only)) => Ok(only),
            (None, None) => Ok(Self::default()),
        }
    }

    /// Layer `project` over `self`. Scalars are the project's when it sets
    /// them; permission lists union, since both files are grants and dropping
    /// the global one silently would widen or narrow access by accident.
    /// `mode` takes the stricter of the two rather than the nearer, so a
    /// project file cannot loosen a global `ask` just by omitting the key.
    fn overlaid_with(self, project: Settings) -> Settings {
        Settings {
            review: self.review.overlaid_with(project.review),
            permissions: merge_permissions(self.permissions, project.permissions),
            mcp: merge_mcp(self.mcp, project.mcp),
            agent: Agent {
                max_tool_rounds: project.agent.max_tool_rounds.or(self.agent.max_tool_rounds),
                command_timeout_secs: project
                    .agent
                    .command_timeout_secs
                    .or(self.agent.command_timeout_secs),
            },
        }
    }
}

/// Servers union by name, the project's definition winning a collision, so a
/// repo can point a shared server name at its own binary.
fn merge_mcp(
    mut global: crate::mcp::McpSettings,
    project: crate::mcp::McpSettings,
) -> crate::mcp::McpSettings {
    global.servers.extend(project.servers);
    global
}

fn merge_permissions(
    global: aster_policy::PermissionsConfig,
    project: aster_policy::PermissionsConfig,
) -> aster_policy::PermissionsConfig {
    fn union(mut a: Vec<String>, b: Vec<String>) -> Vec<String> {
        for item in b {
            if !a.contains(&item) {
                a.push(item);
            }
        }
        a
    }
    aster_policy::PermissionsConfig {
        mode: global.mode.stricter(project.mode),
        allow: union(global.allow, project.allow),
        deny: union(global.deny, project.deny),
        protected: union(global.protected, project.protected),
        secret_read: union(global.secret_read, project.secret_read),
        additional_directories: union(
            global.additional_directories,
            project.additional_directories,
        ),
        allow_exec: union(global.allow_exec, project.allow_exec),
        deny_exec: union(global.deny_exec, project.deny_exec),
        use_default_protected: global.use_default_protected && project.use_default_protected,
    }
}

impl Review {
    fn overlaid_with(self, project: Review) -> Review {
        Review {
            model: project.model.or(self.model),
            base_url: project.base_url.or(self.base_url),
            hypothesis_model: project.hypothesis_model.or(self.hypothesis_model),
            verify_model: project.verify_model.or(self.verify_model),
            min_confidence: project.min_confidence.or(self.min_confidence),
            max_diff_bytes: project.max_diff_bytes.or(self.max_diff_bytes),
            analyzers: pick(self.analyzers, project.analyzers),
            astgrep_rules: project.astgrep_rules.or(self.astgrep_rules),
            effort: project.effort.or(self.effort),
            focus_areas: pick(self.focus_areas, project.focus_areas),
            include: pick(self.include, project.include),
            exclude: pick(self.exclude, project.exclude),
        }
    }
}

/// Review lists replace rather than union: an `include` of `["src/**"]` in a
/// project means that and nothing else.
fn pick(global: Vec<String>, project: Vec<String>) -> Vec<String> {
    if project.is_empty() { global } else { project }
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
        // The legacy `ask` name is `manual`, which prompts for unmatched paths.
        assert!(matches!(
            p.evaluate(&Action::Edit {
                path: "docs/readme.md"
            }),
            Decision::Prompt { .. }
        ));
    }

    #[test]
    fn effort_parses_and_overlays_from_the_project_file() {
        let global: Settings = serde_yaml::from_str("review:\n  effort: high\n").expect("parse");
        assert_eq!(global.review.effort, Some(aster_ai::Effort::High));

        let project: Settings = serde_yaml::from_str("review:\n  effort: off\n").expect("parse");
        let merged = global.overlaid_with(project);
        assert_eq!(merged.review.effort, Some(aster_ai::Effort::Off));
    }

    #[test]
    fn effort_absent_leaves_the_client_default() {
        let s: Settings = serde_yaml::from_str("review: {}").expect("parse");
        assert_eq!(s.review.effort, None);
    }
}
