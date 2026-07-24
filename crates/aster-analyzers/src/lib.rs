#![forbid(unsafe_code)]

pub mod models;

mod ast_grep;
mod semgrep;

use std::path::Path;

pub use ast_grep::AstGrep;
pub use models::{Finding, Severity};
pub use semgrep::Semgrep;

/// A static-analysis backend. `available()` decides whether it can run in the
/// current environment (e.g. its binary is on PATH).
pub trait Analyzer: Send + Sync {
    fn name(&self) -> &'static str;
    fn available(&self) -> bool;
    fn analyze(&self, root: &Path) -> anyhow::Result<Vec<Finding>>;
}

pub struct Detected {
    pub active: Vec<Box<dyn Analyzer>>,
    pub skipped: Vec<&'static str>,
}

/// Every backend name, for callers that want to enable them all.
pub const ALL_MODES: [&str; 2] = ["semgrep", "ast-grep"];

/// Detect only the named backends that are also available. `astgrep_rules` is
/// the ast-grep rule YAML (from `aster.yaml`), threaded into the ast-grep
/// backend so its rules come from config, not just the env.
pub fn detect_with(enabled: &[&str], astgrep_rules: Option<&str>) -> Detected {
    let mut active = Vec::new();
    let mut skipped = Vec::new();
    for analyzer in registry(astgrep_rules) {
        if !enabled.contains(&analyzer.name()) {
            continue;
        }
        if analyzer.available() {
            active.push(analyzer);
        } else {
            skipped.push(analyzer.name());
        }
    }
    Detected { active, skipped }
}

fn registry(astgrep_rules: Option<&str>) -> Vec<Box<dyn Analyzer>> {
    vec![
        Box::new(Semgrep),
        Box::new(AstGrep::new(astgrep_rules.map(str::to_string))),
    ]
}
