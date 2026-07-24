use std::path::Path;
use std::sync::{Arc, Mutex};

use grep::regex::RegexMatcher;
use grep::searcher::Searcher;
use grep::searcher::sinks::UTF8;
use ignore::{WalkBuilder, WalkState};

pub use crate::models::GrepHit;

pub fn grep(root: &Path, pattern: &str, max_results: usize) -> anyhow::Result<Vec<GrepHit>> {
    let matcher = Arc::new(RegexMatcher::new(pattern)?);
    let hits: Arc<Mutex<Vec<GrepHit>>> = Arc::new(Mutex::new(Vec::new()));

    WalkBuilder::new(root).build_parallel().run(|| {
        let matcher = Arc::clone(&matcher);
        let hits = Arc::clone(&hits);
        Box::new(move |result| {
            if hits.lock().unwrap().len() >= max_results {
                return WalkState::Quit;
            }
            let Ok(entry) = result else {
                return WalkState::Continue;
            };
            if !entry.file_type().is_some_and(|t| t.is_file()) {
                return WalkState::Continue;
            }
            let display = entry.path().display().to_string();

            let mut found: Vec<GrepHit> = Vec::new();
            let _ = Searcher::new().search_path(
                matcher.as_ref(),
                entry.path(),
                UTF8(|line, text| {
                    found.push(GrepHit {
                        path: display.clone(),
                        line,
                        text: text.trim_end().to_string(),
                    });
                    Ok(true)
                }),
            );

            if found.is_empty() {
                return WalkState::Continue;
            }
            let mut guard = hits.lock().unwrap();
            guard.extend(found);
            if guard.len() >= max_results {
                WalkState::Quit
            } else {
                WalkState::Continue
            }
        })
    });

    let mut out = Arc::try_unwrap(hits)
        .map(|m| m.into_inner().unwrap())
        .unwrap_or_default();
    out.truncate(max_results);
    Ok(out)
}
