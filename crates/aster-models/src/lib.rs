#![forbid(unsafe_code)]
//! Aster's domain model types.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    pub file_path: String,
    pub line: i32,
    #[serde(default)]
    pub start_line: Option<i32>,
    #[serde(default)]
    pub side: Option<String>,
    pub severity: String,
    pub category: String,
    pub title: String,
    pub description: String,
    pub suggestion: String,
    pub code_snippet: Option<String>,
    pub confidence: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewReport {
    pub summary: String,
    pub findings: Vec<Finding>,
    pub positive_feedback: Vec<String>,
    pub total_issues: usize,
    pub critical_severity_count: usize,
    pub high_severity_count: usize,
    pub medium_severity_count: usize,
    pub low_severity_count: usize,
    pub info_severity_count: usize,
}

impl ReviewReport {
    pub fn new(summary: String, findings: Vec<Finding>, positive_feedback: Vec<String>) -> Self {
        let total_issues = findings.len();
        let by = |s: &str| findings.iter().filter(|f| f.severity == s).count();
        let (critical, high, medium, low, info) = (
            by("critical"),
            by("high"),
            by("medium"),
            by("low"),
            by("info"),
        );
        Self {
            summary,
            findings,
            positive_feedback,
            total_issues,
            critical_severity_count: critical,
            high_severity_count: high,
            medium_severity_count: medium,
            low_severity_count: low,
            info_severity_count: info,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Symbol {
    pub file_path: String,
    pub symbol_name: Option<String>,
    pub symbol_kind: Option<String>,
    pub start_line: Option<i32>,
    pub end_line: Option<i32>,
    pub code_snippet: Option<String>,
}

/// Canonical extension-to-language mapping shared across symbol extraction, index, and ast-grep.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Language {
    Rust,
    Python,
    JavaScript,
    TypeScript,
    Tsx,
    Go,
    Java,
    C,
    Cpp,
    Ruby,
    CSharp,
    Php,
    Swift,
    Kotlin,
    Scala,
}

impl Language {
    pub fn from_extension(ext: &str) -> Option<Self> {
        Some(match ext.to_ascii_lowercase().as_str() {
            "rs" => Self::Rust,
            "py" | "pyw" | "pyi" => Self::Python,
            "js" | "jsx" | "mjs" | "cjs" => Self::JavaScript,
            "ts" | "mts" | "cts" => Self::TypeScript,
            "tsx" => Self::Tsx,
            "go" => Self::Go,
            "java" => Self::Java,
            "c" | "h" => Self::C,
            "cc" | "cpp" | "cxx" | "hpp" | "hxx" | "hh" => Self::Cpp,
            "rb" => Self::Ruby,
            "cs" => Self::CSharp,
            "php" => Self::Php,
            "swift" => Self::Swift,
            "kt" | "kts" => Self::Kotlin,
            "scala" | "sc" | "sbt" => Self::Scala,
            _ => return None,
        })
    }

    pub fn from_path(path: &std::path::Path) -> Option<Self> {
        Self::from_extension(path.extension()?.to_str()?)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::Python => "python",
            Self::JavaScript => "javascript",
            Self::TypeScript => "typescript",
            Self::Tsx => "tsx",
            Self::Go => "go",
            Self::Java => "java",
            Self::C => "c",
            Self::Cpp => "cpp",
            Self::Ruby => "ruby",
            Self::CSharp => "csharp",
            Self::Php => "php",
            Self::Swift => "swift",
            Self::Kotlin => "kotlin",
            Self::Scala => "scala",
        }
    }
}

pub const SKIP_DIRS: &[&str] = &[
    ".git",
    ".hg",
    ".svn",
    ".venv",
    "build",
    "dist",
    "node_modules",
    "out",
    "target",
    "vendor",
];
