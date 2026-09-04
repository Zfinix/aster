//! Chat-tool entry points over the embedded analyzers: structural search,
//! structural rewrite, and a security scan rendered for the model.

use std::path::Path;

use anyhow::Result;
use ast_grep_core::Pattern;
use ast_grep_language::{LanguageExt, SupportLang};

use crate::ALL_MODES;
use crate::ast_grep::{lang_of, source_files};
use crate::models::Severity;

const MAX_MATCHES: usize = 50;
const MAX_DIFF_LINES: usize = 200;
const MAX_FINDINGS: usize = 100;
const MAX_DIFF_FILE_LINES: usize = 2000;

fn lang_from_str(name: &str) -> Option<SupportLang> {
    Some(match name.trim().to_ascii_lowercase().as_str() {
        "rust" => SupportLang::Rust,
        "python" | "py" => SupportLang::Python,
        "javascript" | "js" => SupportLang::JavaScript,
        "typescript" | "ts" => SupportLang::TypeScript,
        "tsx" => SupportLang::Tsx,
        "go" => SupportLang::Go,
        "java" => SupportLang::Java,
        "c" => SupportLang::C,
        "cpp" | "c++" => SupportLang::Cpp,
        _ => return None,
    })
}

fn pattern_for(pattern: &str, lang: SupportLang) -> Pattern {
    Pattern::new(pattern, lang)
}

/// Find every match of an ast-grep pattern under `root`, one `file:line: text`
/// line per match. `language` restricts the scan; without it the language is
/// detected per file.
pub fn ast_grep_search(root: &Path, pattern: &str, language: Option<&str>) -> Result<String> {
    let only = match language {
        Some(name) => Some(
            lang_from_str(name).ok_or_else(|| anyhow::anyhow!("unsupported language {name}"))?,
        ),
        None => None,
    };
    let mut out = Vec::new();
    for file in source_files(root) {
        let Some(lang) = only.or_else(|| lang_of(&file)) else {
            continue;
        };
        let Ok(src) = std::fs::read_to_string(&file) else {
            continue;
        };
        let pat = pattern_for(pattern, lang);
        let tree = lang.ast_grep(&src);
        for m in tree.root().find_all(&pat) {
            if out.len() == MAX_MATCHES {
                out.push(format!(
                    "(more matches exist; showing the first {MAX_MATCHES})"
                ));
                return Ok(out.join("\n"));
            }
            let text = m.text().lines().next().unwrap_or("").trim().to_string();
            out.push(format!(
                "{}:{}: {text}",
                file.display(),
                m.start_pos().line() + 1
            ));
        }
    }
    if out.is_empty() {
        out.push("no matches".to_string());
    }
    Ok(out.join("\n"))
}

/// A computed ast_edit: the new source per changed file, plus the rendered
/// summary and diff. Nothing is written until [`ast_edit_commit`].
pub struct AstEditPlan {
    pub changes: Vec<(std::path::PathBuf, String)>,
    total: usize,
    diff_lines: Vec<String>,
}

impl AstEditPlan {
    /// The summary-plus-diff text the tool returns, also shown at approval time.
    pub fn preview(&self) -> String {
        let mut out = format!(
            "{} file(s) changed, {} match(es) replaced\n{}",
            self.changes.len(),
            self.total,
            self.changes
                .iter()
                .map(|(file, _)| file.display().to_string())
                .collect::<Vec<_>>()
                .join("\n")
        );
        if self.diff_lines.is_empty() {
            out.push_str("\n(diff too large; file list only)");
        } else {
            out.push('\n');
            out.push_str(&self.diff_lines.join("\n"));
        }
        out
    }
}

/// Compute every match of `pattern` and its `rewrite` (ast-grep rewrite
/// syntax, `$$$` metavariables) without writing anything.
pub fn ast_edit_plan(
    root: &Path,
    pattern: &str,
    rewrite: &str,
    language: Option<&str>,
) -> Result<AstEditPlan> {
    let only = match language {
        Some(name) => Some(
            lang_from_str(name).ok_or_else(|| anyhow::anyhow!("unsupported language {name}"))?,
        ),
        None => None,
    };
    let mut changes = Vec::new();
    let mut total = 0usize;
    let mut diff_lines = Vec::new();

    for file in source_files(root) {
        let Some(lang) = only.or_else(|| lang_of(&file)) else {
            continue;
        };
        let Ok(src) = std::fs::read_to_string(&file) else {
            continue;
        };
        let pat = pattern_for(pattern, lang);
        let tree = lang.ast_grep(&src);
        let edits: Vec<(usize, usize, Vec<u8>)> = tree
            .root()
            .find_all(&pat)
            .filter_map(|m| {
                let e = m.replace(&pat, rewrite)?;
                Some((e.position, e.deleted_length, e.inserted_text))
            })
            .collect();
        if edits.is_empty() {
            continue;
        }
        let new_src = apply_edits(&src, &edits);
        total += edits.len();
        push_diff(&mut diff_lines, &file, &src, &new_src);
        changes.push((file, new_src));
    }

    Ok(AstEditPlan {
        changes,
        total,
        diff_lines,
    })
}

/// Write a planned ast_edit to disk. Files with no matches are never touched.
pub fn ast_edit_commit(plan: &AstEditPlan) -> Result<String> {
    if plan.changes.is_empty() {
        return Ok("no matches; nothing changed".to_string());
    }
    for (file, new_src) in &plan.changes {
        std::fs::write(file, new_src)
            .map_err(|e| anyhow::anyhow!("writing {}: {e}", file.display()))?;
    }
    Ok(plan.preview())
}

/// Plan and apply in one step, for callers with no approval step in between.
pub fn ast_edit_apply(
    root: &Path,
    pattern: &str,
    rewrite: &str,
    language: Option<&str>,
) -> Result<String> {
    ast_edit_commit(&ast_edit_plan(root, pattern, rewrite, language)?)
}

fn apply_edits(src: &str, edits: &[(usize, usize, Vec<u8>)]) -> String {
    let mut bytes = src.as_bytes().to_vec();
    // Furthest-first so earlier offsets stay valid as later spans are removed.
    let mut ordered: Vec<&(usize, usize, Vec<u8>)> = edits.iter().collect();
    ordered.sort_by_key(|(pos, _, _)| std::cmp::Reverse(*pos));
    for (pos, len, text) in ordered {
        bytes.splice(*pos..*pos + *len, text.iter().copied());
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

fn push_diff(out: &mut Vec<String>, file: &Path, old: &str, new: &str) {
    let old_lines: Vec<&str> = old.lines().collect();
    let new_lines: Vec<&str> = new.lines().collect();
    if old_lines.len() > MAX_DIFF_FILE_LINES || new_lines.len() > MAX_DIFF_FILE_LINES {
        return;
    }
    let ops = lcs_ops(&old_lines, &new_lines);
    if ops.iter().all(|op| matches!(op, Op::Keep(_))) {
        return;
    }
    out.push(format!("--- {}\n+++ {}", file.display(), file.display()));
    for op in ops {
        if out.len() >= MAX_DIFF_LINES {
            out.push(format!("(diff cut at {MAX_DIFF_LINES} lines)"));
            return;
        }
        match op {
            Op::Keep(i) => out.push(format!("  {}", old_lines[i])),
            Op::Del(i) => out.push(format!("- {}", old_lines[i])),
            Op::Add(i) => out.push(format!("+ {}", new_lines[i])),
        }
    }
}

enum Op {
    Keep(usize),
    Del(usize),
    Add(usize),
}

fn lcs_ops(a: &[&str], b: &[&str]) -> Vec<Op> {
    let n = a.len();
    let m = b.len();
    let mut table = vec![0u32; (n + 1) * (m + 1)];
    let at = |i: usize, j: usize| i * (m + 1) + j;
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            table[at(i, j)] = if a[i] == b[j] {
                table[at(i + 1, j + 1)] + 1
            } else {
                table[at(i + 1, j)].max(table[at(i, j + 1)])
            };
        }
    }
    let mut ops = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if a[i] == b[j] {
            ops.push(Op::Keep(i));
            i += 1;
            j += 1;
        } else if table[at(i + 1, j)] >= table[at(i, j + 1)] {
            ops.push(Op::Del(i));
            i += 1;
        } else {
            ops.push(Op::Add(j));
            j += 1;
        }
    }
    while i < n {
        ops.push(Op::Del(i));
        i += 1;
    }
    while j < m {
        ops.push(Op::Add(j));
        j += 1;
    }
    ops
}

/// Run every available analyzer over `root` (or `scope` when given) and render
/// findings as `severity file:line: rule - message` lines.
pub fn security_scan(root: &Path, scope: Option<&Path>) -> Result<String> {
    let target = match scope {
        // Resolve against the repo, never the process CWD, and keep the scan
        // inside it: a `..` or absolute path would read outside the repo.
        Some(p) => {
            let joined = root.join(p);
            if !joined.exists() {
                anyhow::bail!("{} does not exist", p.display());
            }
            let resolved = joined.canonicalize().unwrap_or(joined);
            let root = root.canonicalize().unwrap_or_default();
            if !resolved.starts_with(&root) {
                anyhow::bail!("{} is outside the repository", p.display());
            }
            resolved
        }
        None => root.to_path_buf(),
    };
    let detected = crate::detect_with(&ALL_MODES, None);
    let mut findings = Vec::new();
    for analyzer in detected.active {
        findings.extend(analyzer.analyze(&target)?);
    }
    findings.sort_by(|a, b| a.file.cmp(&b.file).then(a.line.cmp(&b.line)));
    let mut out = Vec::new();
    for f in &findings {
        if out.len() == MAX_FINDINGS {
            out.push(format!(
                "({} more findings exist; showing the first {MAX_FINDINGS})",
                findings.len() - MAX_FINDINGS
            ));
            break;
        }
        out.push(format!(
            "{} {}:{}: {} - {}",
            match f.severity {
                Severity::Error => "error",
                Severity::Warning => "warning",
                Severity::Info => "info",
            },
            f.file,
            f.line,
            f.rule,
            f.message
        ));
    }
    if out.is_empty() {
        out.push("no findings".to_string());
    }
    if !detected.skipped.is_empty() {
        out.push(format!(
            "skipped (not installed): {}",
            detected.skipped.join(", ")
        ));
    }
    Ok(out.join("\n"))
}
