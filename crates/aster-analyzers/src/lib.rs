#![forbid(unsafe_code)]

pub mod models;

mod ast_grep;
mod semgrep;
mod tools;

use std::path::Path;

pub use ast_grep::AstGrep;
pub use models::{Finding, Severity};
pub use semgrep::Semgrep;
pub use tools::{
    AstEditPlan, ast_edit_apply, ast_edit_commit, ast_edit_plan, ast_grep_search, security_scan,
};

#[cfg(test)]
#[path = "tools_tests.rs"]
mod tools_tests;

pub trait Analyzer: Send + Sync {
    fn name(&self) -> &'static str;
    fn available(&self) -> bool;
    fn analyze(&self, root: &Path) -> anyhow::Result<Vec<Finding>>;
}

pub struct Detected {
    pub active: Vec<Box<dyn Analyzer>>,
    pub skipped: Vec<&'static str>,
}

pub const ALL_MODES: [&str; 2] = ["semgrep", "ast-grep"];

/// Detect only the named backends that are also available.
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
