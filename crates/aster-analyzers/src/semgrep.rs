use std::path::Path;
use std::process::Command;

use serde_json::Value;

use crate::Analyzer;
use crate::models::{Finding, Severity};

/// Semgrep / opengrep. Semgrep is a Python tool with no Rust library, so it
/// must shell out; ast-grep is the other subprocess backend.
pub struct Semgrep;

const BINARIES: [&str; 2] = ["opengrep", "semgrep"];

impl Semgrep {
    fn binary(&self) -> Option<&'static str> {
        BINARIES.iter().copied().find(|b| which::which(b).is_ok())
    }
}

impl Analyzer for Semgrep {
    fn name(&self) -> &'static str {
        "semgrep"
    }

    fn available(&self) -> bool {
        self.binary().is_some()
    }

    fn analyze(&self, root: &Path) -> anyhow::Result<Vec<Finding>> {
        let binary = self
            .binary()
            .ok_or_else(|| anyhow::anyhow!("no semgrep/opengrep binary on PATH"))?;
        let output = Command::new(binary)
            .args(["scan", "--config", "auto", "--json", "--quiet"])
            .env("SEMGREP_SEND_METRICS", "off")
            .arg(root)
            .output()?;

        let json: Value = match serde_json::from_slice(&output.stdout) {
            Ok(json) => json,
            Err(err) => {
                let code = output.status.code().unwrap_or(-1);
                anyhow::bail!(
                    "semgrep exited with status {code} and produced unparseable output ({err}): {}",
                    truncate(&String::from_utf8_lossy(&output.stderr))
                );
            }
        };
        let findings = json["results"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|r| Finding {
                tool: "semgrep".to_string(),
                rule: r["check_id"].as_str().unwrap_or_default().to_string(),
                severity: severity(r["extra"]["severity"].as_str().unwrap_or_default()),
                file: r["path"].as_str().unwrap_or_default().to_string(),
                line: r["start"]["line"].as_u64().unwrap_or(0) as u32,
                message: r["extra"]["message"]
                    .as_str()
                    .unwrap_or_default()
                    .trim()
                    .to_string(),
            })
            .collect();
        Ok(findings)
    }
}

fn severity(s: &str) -> Severity {
    match s {
        "ERROR" => Severity::Error,
        "WARNING" => Severity::Warning,
        _ => Severity::Info,
    }
}

fn truncate(s: &str) -> String {
    const MAX: usize = 2000;
    match s.char_indices().nth(MAX) {
        Some((idx, _)) => format!("{}...", &s[..idx]),
        None => s.to_string(),
    }
}
