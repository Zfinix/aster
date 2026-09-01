//! Tiered file search: `rg` → embedded `grep`/`ignore` → hand-rolled walker.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::{SKIP_DIRS, ToolProbe};

/// Matches taken from one file before moving on. Without it a single hot file
/// spends the whole hit budget and the caller never sees the other matches.
const PER_FILE: usize = 3;

/// One match: repo-relative path, 1-based line number, and the line's text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub path: String,
    pub line: usize,
    pub text: String,
}

/// Search `base`, a directory or a single file, for `query`, at most [`PER_FILE`]
/// matches per file and `max_hits` overall. Smart-case: a query with any uppercase
/// letter is taken literally.
pub fn search(
    probe: &ToolProbe,
    repo_root: &Path,
    base: &Path,
    query: &str,
    max_hits: usize,
) -> Result<Vec<Hit>> {
    if query.trim().is_empty() {
        bail!("empty search query");
    }

    let hits = run(probe, repo_root, base, query, max_hits, true)?;
    if !hits.is_empty() {
        return Ok(hits);
    }
    // Same reasoning as `find`: an ignored path is still part of the repo, so
    // a query that matches nothing else falls back to searching it.
    run(probe, repo_root, base, query, max_hits, false)
}

fn run(
    probe: &ToolProbe,
    repo_root: &Path,
    base: &Path,
    query: &str,
    max_hits: usize,
    respect_ignore: bool,
) -> Result<Vec<Hit>> {
    match &probe.rg {
        Some(rg) => search_with_rg(rg, repo_root, base, query, max_hits, respect_ignore),
        None => search_embedded(repo_root, base, query, max_hits, respect_ignore),
    }
}

/// Render hits as text for the model, grouped by file with `context` lines
/// either side. Grouping keeps the path off every line, so the context fits
/// in the tool-result budget.
pub fn render(repo_root: &Path, hits: &[Hit], context: usize) -> String {
    if hits.is_empty() {
        return "no matches".into();
    }

    let mut by_file: Vec<(&str, Vec<usize>)> = Vec::new();
    for hit in hits {
        match by_file.iter_mut().find(|(path, _)| *path == hit.path) {
            Some((_, lines)) => lines.push(hit.line),
            None => by_file.push((&hit.path, vec![hit.line])),
        }
    }

    let mut out = String::new();
    for (path, lines) in &by_file {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(path);
        out.push('\n');
        match fs::read_to_string(repo_root.join(path)) {
            Ok(content) => {
                let source: Vec<&str> = content.lines().collect();
                render_windows(&mut out, &source, lines, context);
            }
            // Unreadable now (deleted, binary, no permission): the hit still
            // stands, so show what the search itself returned.
            Err(_) => {
                for line in lines {
                    let text = hits
                        .iter()
                        .find(|h| h.path == *path && h.line == *line)
                        .map(|h| h.text.as_str())
                        .unwrap_or_default();
                    out.push_str(&format!("> {line}  {text}\n"));
                }
            }
        }
    }
    out.trim_end().to_string()
}

/// Write each match with its surrounding lines, merging windows that overlap
/// and separating the ones that do not.
fn render_windows(out: &mut String, source: &[&str], lines: &[usize], context: usize) {
    let mut windows: Vec<(usize, usize)> = Vec::new();
    for &line in lines {
        let start = line.saturating_sub(context).max(1);
        let end = (line + context).min(source.len());
        match windows.last_mut() {
            Some((_, prev_end)) if start <= *prev_end + 1 => *prev_end = (*prev_end).max(end),
            _ => windows.push((start, end)),
        }
    }

    for (i, (start, end)) in windows.iter().enumerate() {
        if i > 0 {
            out.push_str("--\n");
        }
        for no in *start..=*end {
            let Some(text) = source.get(no - 1) else {
                continue;
            };
            let marker = if lines.contains(&no) { '>' } else { ' ' };
            out.push_str(&format!("{marker} {no}  {text}\n"));
        }
    }
}

/// Shell out to `rg`. Respects `.gitignore` natively.
fn search_with_rg(
    rg: &Path,
    repo_root: &Path,
    base: &Path,
    query: &str,
    max_hits: usize,
    respect_ignore: bool,
) -> Result<Vec<Hit>> {
    let run = |literal: bool| {
        let mut cmd = Command::new(rg);
        cmd.args([
            "--line-number",
            // rg drops the path when it is given a single file, which leaves
            // `parse_rg_line` nothing to split on and silently loses every hit.
            "--with-filename",
            "--no-heading",
            "--smart-case",
            "--color",
            "never",
            "--max-count",
            &PER_FILE.to_string(),
        ]);
        if literal {
            cmd.arg("--fixed-strings");
        }
        if !respect_ignore {
            cmd.arg("--no-ignore").arg("--hidden");
            for dir in SKIP_DIRS {
                cmd.arg("--glob").arg(format!("!{dir}/"));
            }
        }
        cmd.arg(query)
            .arg(base)
            .current_dir(repo_root)
            .output()
            .context("running rg")
    };

    let mut output = run(false)?;
    // A query that is not valid regex is almost always meant literally.
    if !output.status.success() && !output.status.code().is_some_and(|c| c == 1) {
        output = run(true)?;
    }
    if !output.status.success() && !output.status.code().is_some_and(|c| c == 1) {
        bail!(
            "rg failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout
        .lines()
        .filter_map(|line| parse_rg_line(repo_root, line))
        .take(max_hits)
        .collect())
}

/// Split rg's `path:line:text` at the first `:<digits>:`, so paths containing
/// a colon do not shift the fields.
fn parse_rg_line(repo_root: &Path, line: &str) -> Option<Hit> {
    let mut from = 0;
    while let Some(offset) = line[from..].find(':') {
        let at = from + offset;
        let rest = &line[at + 1..];
        let end = rest.find(':')?;
        if let Ok(no) = rest[..end].parse::<usize>() {
            return Some(Hit {
                path: relative(repo_root, &line[..at]),
                line: no,
                text: rest[end + 1..].trim_end().to_string(),
            });
        }
        from = at + 1;
    }
    None
}

fn relative(repo_root: &Path, path: &str) -> String {
    Path::new(path)
        .strip_prefix(repo_root)
        .unwrap_or(Path::new(path))
        .to_string_lossy()
        .into_owned()
}

/// Embedded ripgrep via the `grep` and `ignore` crates. Respects
/// `.gitignore` and walks in parallel.
fn search_embedded(
    repo_root: &Path,
    base: &Path,
    query: &str,
    max_hits: usize,
    respect_ignore: bool,
) -> Result<Vec<Hit>> {
    use grep::regex::RegexMatcherBuilder;
    use grep::searcher::Searcher;
    use grep::searcher::sinks::UTF8;
    use ignore::WalkBuilder;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    let build = |pattern: &str| RegexMatcherBuilder::new().case_smart(true).build(pattern);
    // A query that is not valid regex is almost always meant literally.
    let matcher = match build(query) {
        Ok(m) => m,
        Err(_) => build(&escape_regex(query))
            .with_context(|| format!("invalid search pattern: {query}"))?,
    };
    let matcher = Arc::new(matcher);
    // Keyed by path so the parallel walk still returns a stable order.
    let hits: Arc<Mutex<BTreeMap<String, Vec<Hit>>>> = Arc::new(Mutex::new(BTreeMap::new()));
    let count = Arc::new(AtomicUsize::new(0));

    let mut builder = WalkBuilder::new(base);
    builder
        .hidden(respect_ignore)
        .git_ignore(respect_ignore)
        .require_git(false)
        .git_exclude(respect_ignore);
    if !respect_ignore {
        builder.filter_entry(|entry| !crate::is_skipped(entry));
    }
    builder.build_parallel().run(|| {
        let matcher = Arc::clone(&matcher);
        let hits = Arc::clone(&hits);
        let count = Arc::clone(&count);
        Box::new(move |result| {
            if count.load(Ordering::Relaxed) >= max_hits {
                return ignore::WalkState::Quit;
            }
            let Ok(entry) = result else {
                return ignore::WalkState::Continue;
            };
            if !entry.file_type().is_some_and(|t| t.is_file()) {
                return ignore::WalkState::Continue;
            }
            let path = entry.path();
            let rel = relative(repo_root, &path.to_string_lossy());
            let mut found: Vec<Hit> = Vec::new();
            let _ = Searcher::new().search_path(
                matcher.as_ref(),
                path,
                UTF8(|line_no, text| {
                    found.push(Hit {
                        path: rel.clone(),
                        line: line_no as usize,
                        text: text.trim_end().to_string(),
                    });
                    Ok(found.len() < PER_FILE)
                }),
            );
            if !found.is_empty() {
                count.fetch_add(found.len(), Ordering::Relaxed);
                hits.lock().unwrap().insert(rel, found);
            }
            ignore::WalkState::Continue
        })
    });

    let grouped = Arc::try_unwrap(hits)
        .map(|m| m.into_inner().unwrap())
        .unwrap_or_default();
    Ok(grouped.into_values().flatten().take(max_hits).collect())
}

fn escape_regex(query: &str) -> String {
    const META: &[char] = &[
        '\\', '.', '+', '*', '?', '(', ')', '|', '[', ']', '{', '}', '^', '$', '#', '&', '-', '~',
    ];
    let mut out = String::with_capacity(query.len() * 2);
    for ch in query.chars() {
        if META.contains(&ch) {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

/// Hand-rolled fallback: `fs::read_dir` + substring match. No regex, no
/// `.gitignore`, but zero external dependencies beyond `std`.
#[allow(dead_code)]
fn search_manual(repo_root: &Path, base: &Path, query: &str, max_hits: usize) -> Result<Vec<Hit>> {
    let needle = query.to_lowercase();
    let mut hits = Vec::new();
    let mut stack = vec![base.to_path_buf()];
    while let Some(current) = stack.pop() {
        if hits.len() >= max_hits {
            break;
        }
        let Ok(entries) = fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if path.is_dir() {
                if !name.starts_with('.') && !SKIP_DIRS.contains(&name.as_str()) {
                    stack.push(path);
                }
                continue;
            }
            let rel = relative(repo_root, &path.to_string_lossy());
            let Ok(content) = fs::read_to_string(&path) else {
                continue;
            };
            let mut in_file = 0;
            for (no, line) in content.lines().enumerate() {
                if line.to_lowercase().contains(&needle) {
                    hits.push(Hit {
                        path: rel.clone(),
                        line: no + 1,
                        text: line.trim_end().to_string(),
                    });
                    in_file += 1;
                    if in_file >= PER_FILE || hits.len() >= max_hits {
                        break;
                    }
                }
            }
            if hits.len() >= max_hits {
                break;
            }
        }
    }
    Ok(hits)
}

#[cfg(test)]
#[path = "tests/search_test.rs"]
mod tests;
