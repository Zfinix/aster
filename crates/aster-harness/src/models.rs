use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub struct ReviewInput {
    pub diff: String,
    pub base_branch: String,
    pub repo_name: String,
    pub pr_number: Option<i64>,
    pub repo_root: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct HarnessConfig {
    pub max_exploration_depth: u32,
    pub max_diff_bytes: usize,
    pub hypothesis_model: Option<String>,
    pub verify_model: Option<String>,
    pub analyzers: Vec<String>,
    pub astgrep_rules: Option<String>,
    pub verify_concurrency: usize,
    pub focus_areas: Vec<String>,
    pub min_confidence: f32,
    pub max_evidence_bytes: usize,
}

impl Default for HarnessConfig {
    fn default() -> Self {
        Self {
            max_exploration_depth: 5,
            max_diff_bytes: 200_000,
            hypothesis_model: None,
            verify_model: None,
            analyzers: Vec::new(),
            astgrep_rules: None,
            verify_concurrency: 8,
            focus_areas: Vec::new(),
            min_confidence: 0.5,
            max_evidence_bytes: 12_000,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CandidateSource {
    #[default]
    Model,
    Static,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Candidate {
    pub file: String,
    pub line: i32,
    pub defect_class: String,
    pub severity: String,
    pub title: String,
    pub failure_scenario: String,
    pub suggestion: String,
    #[serde(default)]
    pub code_snippet: Option<String>,
    #[serde(default)]
    pub source: CandidateSource,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CandidateList {
    #[serde(default)]
    pub candidates: Vec<Candidate>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Verdict {
    pub real: bool,
    #[serde(default)]
    pub confidence: f32,
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub explanation: String,
}
