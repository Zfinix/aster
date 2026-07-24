use std::path::{Path, PathBuf};

use ast_grep_config::{GlobalRules, RuleConfig, Severity as SgSeverity, from_yaml_string};
use ast_grep_language::{LanguageExt, SupportLang};
use aster_models::Language;

use crate::Analyzer;
use crate::models::{Finding, Severity};

/// Embedded ast-grep (no `sg` binary). Rules come from the YAML passed at
/// construction, else the file named by `ASTER_ASTGREP_RULES`.
#[derive(Default)]
pub struct AstGrep {
    rules_yaml: Option<String>,
}

impl AstGrep {
    pub fn new(rules_yaml: Option<String>) -> Self {
        Self { rules_yaml }
    }
}

impl Analyzer for AstGrep {
    fn name(&self) -> &'static str {
        "ast-grep"
    }

    fn available(&self) -> bool {
        true
    }

    fn analyze(&self, root: &Path) -> anyhow::Result<Vec<Finding>> {
        let yaml = match &self.rules_yaml {
            Some(yaml) => yaml.clone(),
            None => match std::env::var_os("ASTER_ASTGREP_RULES") {
                Some(path) => std::fs::read_to_string(&path)
                    .map_err(|e| anyhow::anyhow!("reading ASTER_ASTGREP_RULES {path:?}: {e}"))?,
                None => return Ok(Vec::new()),
            },
        };
        self.scan(root, &yaml)
    }
}

impl AstGrep {
    pub fn scan(&self, root: &Path, rules_yaml: &str) -> anyhow::Result<Vec<Finding>> {
        let globals = GlobalRules::default();
        let rules: Vec<RuleConfig<SupportLang>> = from_yaml_string(rules_yaml, &globals)
            .map_err(|e| anyhow::anyhow!("parsing ast-grep rules: {e}"))?;
        if rules.is_empty() {
            return Ok(Vec::new());
        }

        let mut findings = Vec::new();
        for file in source_files(root) {
            let Some(lang) = lang_of(&file) else { continue };
            let Ok(src) = std::fs::read_to_string(&file) else {
                continue;
            };
            let tree = lang.ast_grep(&src);
            let display = file.to_string_lossy().into_owned();

            for rule in rules.iter().filter(|r| r.language == lang) {
                for m in tree.root().find_all(&rule.matcher) {
                    let message = {
                        let msg = rule.get_message(&m);
                        if msg.is_empty() {
                            rule.message.clone()
                        } else {
                            msg
                        }
                    };
                    findings.push(Finding {
                        tool: "ast-grep".to_string(),
                        rule: rule.id.clone(),
                        severity: severity(&rule.severity),
                        file: display.clone(),
                        // ast-grep positions are 0-based rows.
                        line: m.start_pos().line() as u32 + 1,
                        message,
                    });
                }
            }
        }
        Ok(findings)
    }
}

fn lang_of(path: &Path) -> Option<SupportLang> {
    Some(match Language::from_path(path)? {
        Language::Rust => SupportLang::Rust,
        Language::Python => SupportLang::Python,
        Language::JavaScript => SupportLang::JavaScript,
        Language::TypeScript => SupportLang::TypeScript,
        Language::Tsx => SupportLang::Tsx,
        Language::Go => SupportLang::Go,
        Language::Java => SupportLang::Java,
        Language::C => SupportLang::C,
        Language::Cpp => SupportLang::Cpp,
        Language::Ruby
        | Language::CSharp
        | Language::Php
        | Language::Swift
        | Language::Kotlin
        | Language::Scala => return None,
    })
}

fn source_files(root: &Path) -> Vec<PathBuf> {
    fn skip_dir(name: &str) -> bool {
        matches!(name, "target" | "node_modules" | "dist" | "build" | ".venv")
            || name.starts_with('.')
    }
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            match entry.file_type() {
                Ok(t) if t.is_dir() => {
                    if !skip_dir(&name) {
                        stack.push(path);
                    }
                }
                Ok(t) if t.is_file() => out.push(path),
                _ => {}
            }
        }
    }
    out
}

fn severity(s: &SgSeverity) -> Severity {
    match s {
        SgSeverity::Error => Severity::Error,
        SgSeverity::Warning => Severity::Warning,
        _ => Severity::Info,
    }
}
