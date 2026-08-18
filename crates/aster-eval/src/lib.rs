//! Grades aster against its own recorded sessions: how many model round-trips
//! a turn costs, how well the model batches, and which tools answer nothing.

#![forbid(unsafe_code)]

mod live;
mod report;
mod stats;
mod turn;

pub use live::{Case, ModelRun, default_cases, render_eval, render_live, repo_root, sweep};
pub use report::{Delta, ModelStat, Report, ToolStat, render, render_comparison};
pub use stats::Dist;
pub use turn::{Call, Turn, barren, turns};

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use aster_persist::SessionTranscript;
use chrono::{DateTime, Duration, Utc};

#[derive(Debug, Clone, Default)]
pub struct Filter {
    /// Only sessions created at or after this instant.
    pub since: Option<DateTime<Utc>>,
    /// Only sessions recorded against this model.
    pub model: Option<String>,
}

impl Filter {
    pub fn since_days(days: i64) -> Self {
        Self {
            since: Some(Utc::now() - Duration::days(days)),
            model: None,
        }
    }

    fn keeps(&self, transcript: &SessionTranscript) -> bool {
        if self.since.is_some_and(|at| transcript.meta.created_at < at) {
            return false;
        }
        match &self.model {
            Some(want) => transcript.meta.model.as_deref() == Some(want.as_str()),
            None => true,
        }
    }
}

/// Analyze every session transcript under `dir`, recursively, so one call can
/// cover a single project or the whole sessions root.
pub fn analyze(dir: &Path, filter: &Filter) -> Result<Report> {
    let mut sessions = 0;
    let mut all = Vec::new();
    for path in transcripts(dir)? {
        let transcript = match SessionTranscript::load(&path) {
            Ok(transcript) => transcript,
            // One unreadable file should not sink a sweep over hundreds.
            Err(e) => {
                eprintln!("skipping {}: {e:#}", path.display());
                continue;
            }
        };
        if !filter.keeps(&transcript) {
            continue;
        }
        sessions += 1;
        all.extend(turns(&transcript));
    }
    Ok(Report::build(sessions, &all))
}

fn transcripts(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut found = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let entries = std::fs::read_dir(&current)
            .with_context(|| format!("reading {}", current.display()))?;
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
                found.push(path);
            }
        }
    }
    found.sort();
    Ok(found)
}

#[cfg(test)]
#[path = "tests/lib_test.rs"]
mod tests;
