//! In-process file search on the `grep`/`ignore` crates, so the walk can skip
//! cloud placeholders, stop at the hit cap, and give up at a deadline.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use grep::regex::{RegexMatcher, RegexMatcherBuilder};
use grep::searcher::sinks::UTF8;
use grep::searcher::{BinaryDetection, SearcherBuilder};
use ignore::{WalkBuilder, WalkState};

const PER_FILE: usize = 3;

/// Past this the search returns what it has. A read that blocks on a network
/// mount must not hold the whole turn hostage.
const DEADLINE: Duration = Duration::from_secs(20);

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
pub fn search(repo_root: &Path, base: &Path, query: &str, max_hits: usize) -> Result<Vec<Hit>> {
    if query.trim().is_empty() {
        bail!("empty search query");
    }

    let build = |pattern: &str| RegexMatcherBuilder::new().case_smart(true).build(pattern);
    // A query that is not valid regex is almost always meant literally.
    let matcher = match build(query) {
        Ok(m) => m,
        Err(_) => build(&escape_regex(query))
            .with_context(|| format!("invalid search pattern: {query}"))?,
    };
    let matcher = Arc::new(matcher);
    let deadline = Instant::now() + DEADLINE;

    let hits = walk(repo_root, base, &matcher, max_hits, true, deadline);
    if !hits.is_empty() {
        return Ok(hits);
    }
    // Same reasoning as `find`: an ignored path is still part of the repo, so
    // a query that matches nothing else falls back to searching it.
    Ok(walk(repo_root, base, &matcher, max_hits, false, deadline))
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

fn relative(repo_root: &Path, path: &str) -> String {
    Path::new(path)
        .strip_prefix(repo_root)
        .unwrap_or(Path::new(path))
        .to_string_lossy()
        .into_owned()
}

fn walk(
    repo_root: &Path,
    base: &Path,
    matcher: &Arc<RegexMatcher>,
    max_hits: usize,
    respect_ignore: bool,
    deadline: Instant,
) -> Vec<Hit> {
    // Keyed by path so the parallel walk still returns a stable order.
    let hits: Arc<Mutex<BTreeMap<String, Vec<Hit>>>> = Arc::new(Mutex::new(BTreeMap::new()));
    let count = Arc::new(AtomicUsize::new(0));
    let stop = Arc::new(AtomicBool::new(false));

    let library = std::env::var_os("HOME").map(|home| PathBuf::from(home).join("Library"));
    let mut builder = WalkBuilder::new(base);
    builder
        .hidden(respect_ignore)
        .git_ignore(respect_ignore)
        .require_git(false)
        .git_exclude(respect_ignore)
        .filter_entry(move |entry| {
            !(is_offline(entry)
                || library.as_deref() == Some(entry.path())
                || (!respect_ignore && crate::is_skipped(entry)))
        });
    let walker = builder.build_parallel();

    let (done_tx, done_rx) = mpsc::channel();
    {
        let repo_root = repo_root.to_path_buf();
        let matcher = Arc::clone(matcher);
        let hits = Arc::clone(&hits);
        let stop = Arc::clone(&stop);
        thread::spawn(move || {
            walker.run(|| {
                let repo_root = repo_root.clone();
                let matcher = Arc::clone(&matcher);
                let hits = Arc::clone(&hits);
                let count = Arc::clone(&count);
                let stop = Arc::clone(&stop);
                let mut searcher = SearcherBuilder::new()
                    .binary_detection(BinaryDetection::quit(b'\0'))
                    .build();
                Box::new(move |result| {
                    if stop.load(Ordering::Relaxed) || count.load(Ordering::Relaxed) >= max_hits {
                        return WalkState::Quit;
                    }
                    let Ok(entry) = result else {
                        return WalkState::Continue;
                    };
                    if !entry.file_type().is_some_and(|t| t.is_file()) {
                        return WalkState::Continue;
                    }
                    let path = entry.path();
                    let rel = relative(&repo_root, &path.to_string_lossy());
                    let mut found: Vec<Hit> = Vec::new();
                    let _ = searcher.search_path(
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
                    WalkState::Continue
                })
            });
            let _ = done_tx.send(());
        });
    }

    // On timeout the walk thread is left to wind down on its own: a read
    // stuck in the kernel cannot be interrupted, only abandoned.
    if done_rx
        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .is_err()
    {
        stop.store(true, Ordering::Relaxed);
        tracing::debug!(base = %base.display(), "search hit its deadline");
    }

    let grouped = hits.lock().unwrap().clone();
    grouped.into_values().flatten().take(max_hits).collect()
}

/// Cloud placeholders (iCloud "optimise storage", OneDrive files on demand)
/// download on first read, which can block for minutes, so they are never opened.
fn is_offline(entry: &ignore::DirEntry) -> bool {
    let Ok(meta) = entry.metadata() else {
        return false;
    };
    #[cfg(target_os = "macos")]
    {
        use std::os::macos::fs::MetadataExt;
        const SF_DATALESS: u32 = 0x4000_0000;
        meta.st_flags() & SF_DATALESS != 0
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const OFFLINE: u32 = 0x1000;
        const RECALL_ON_OPEN: u32 = 0x4_0000;
        const RECALL_ON_DATA_ACCESS: u32 = 0x40_0000;
        meta.file_attributes() & (OFFLINE | RECALL_ON_OPEN | RECALL_ON_DATA_ACCESS) != 0
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        let _ = meta;
        false
    }
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

#[cfg(test)]
#[path = "tests/search_test.rs"]
mod tests;
