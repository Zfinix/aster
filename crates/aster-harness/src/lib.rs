#![forbid(unsafe_code)]

pub mod indexing;
pub mod models;
pub mod progress;
pub mod prompts;

use std::path::Path;
use std::sync::Arc;

use anyhow::Result;
use aster_ai::AiClient;
use aster_index::SqliteCodeIndex;
use aster_models::{Finding, ReviewReport};
use futures_util::StreamExt;

pub use models::{Candidate, CandidateList, CandidateSource, HarnessConfig, ReviewInput, Verdict};
pub use progress::{Progress, ProgressSink};

pub struct ReviewDeps {
    pub ai_client: Arc<AiClient>,
    pub index: Option<SqliteCodeIndex>,
    pub config: HarnessConfig,
}

pub async fn review(deps: &ReviewDeps, input: ReviewInput) -> Result<ReviewReport> {
    review_with_progress(deps, input, &None).await
}

pub async fn review_with_progress(
    deps: &ReviewDeps,
    input: ReviewInput,
    sink: &ProgressSink,
) -> Result<ReviewReport> {
    let diff = truncate(&input.diff, deps.config.max_diff_bytes);

    progress::emit(sink, Progress::Phase("Hypothesizing".into()));
    let mut candidates = hypothesize(deps, &input.repo_name, &diff, sink).await?;
    tracing::info!(
        count = candidates.len(),
        "hypothesis pass produced candidates"
    );
    progress::emit(
        sink,
        Progress::Hypothesized {
            count: candidates.len(),
        },
    );

    let static_candidates = analyzer_candidates(&input, &deps.config);
    tracing::info!(
        count = static_candidates.len(),
        "static analyzers produced candidates"
    );
    candidates.extend(static_candidates);

    let total = candidates.len();
    progress::emit(
        sink,
        Progress::Phase(format!("Verifying {total} candidate(s)")),
    );

    // Verification is I/O-bound on the model call, so verify candidates
    // concurrently (bounded) rather than one at a time. Progress events
    // interleave as calls finish; the final findings are re-sorted to a stable,
    // hypothesis-order list before shaping so output is deterministic.
    let concurrency = deps.config.verify_concurrency.max(1);
    let repo_root = input.repo_root.as_deref();
    let min_confidence = deps.config.min_confidence;
    let diff_ref = diff.as_str();
    let ordered: Vec<(usize, Finding)> = futures_util::stream::iter(
        candidates.into_iter().enumerate(),
    )
    .map(|(i, candidate)| async move {
        progress::emit(
            sink,
            Progress::Verifying {
                index: i + 1,
                total,
                title: candidate.title.clone(),
            },
        );

        // Every candidate, whether hypothesized or produced by a static
        // analyzer, goes through the same adversarial verify so it earns
        // a real confidence and clears the same gate. Provenance is kept
        // on the candidate but no longer changes the output path.
        let evidence = retrieve_evidence(deps, repo_root, diff_ref, &candidate).await;
        match verify(deps, &candidate, &evidence, sink).await {
            Ok(verdict) if verdict.real && verdict.confidence >= min_confidence => {
                let finding = shape(candidate, verdict);
                progress::emit(sink, Progress::Confirmed(Box::new(finding.clone())));
                Some((i, finding))
            }
            Ok(verdict) if verdict.real => {
                let reason = format!(
                    "confidence {:.2} below threshold {:.2}",
                    verdict.confidence, min_confidence
                );
                tracing::debug!(title = %candidate.title, reason, "refuted on low confidence");
                progress::emit(
                    sink,
                    Progress::Refuted {
                        title: candidate.title,
                        reason,
                    },
                );
                None
            }
            Ok(verdict) => {
                tracing::debug!(title = %candidate.title, reason = %verdict.reason, "refuted");
                progress::emit(
                    sink,
                    Progress::Refuted {
                        title: candidate.title,
                        reason: verdict.reason,
                    },
                );
                None
            }
            Err(e) => {
                tracing::warn!(title = %candidate.title, error = %e, "verify failed; dropping");
                progress::emit(
                    sink,
                    Progress::Refuted {
                        title: candidate.title,
                        reason: format!("verify failed: {e}"),
                    },
                );
                None
            }
        }
    })
    .buffer_unordered(concurrency)
    .filter_map(|r| async move { r })
    .collect()
    .await;

    let findings = shape_report(ordered);

    let summary = format!(
        "Reviewed {} against {}: {} finding(s) survived adversarial verification.",
        input.repo_name,
        input.base_branch,
        findings.len()
    );
    progress::emit(
        sink,
        Progress::Done {
            summary: summary.clone(),
            total: findings.len(),
        },
    );
    Ok(ReviewReport::new(summary, findings, Vec::new()))
}

fn analyzer_candidates(input: &ReviewInput, config: &HarnessConfig) -> Vec<Candidate> {
    if config.analyzers.is_empty() {
        return Vec::new();
    }
    let Some(root) = input.repo_root.as_ref() else {
        tracing::warn!("analyzers requested but no repo_root; skipping static pass");
        return Vec::new();
    };
    let names: Vec<&str> = config.analyzers.iter().map(String::as_str).collect();
    let detected = aster_analyzers::detect_with(&names, config.astgrep_rules.as_deref());
    if !detected.skipped.is_empty() {
        tracing::warn!(skipped = ?detected.skipped, "analyzers unavailable in this environment");
    }
    let mut out = Vec::new();
    for analyzer in detected.active {
        match analyzer.analyze(root) {
            Ok(findings) => out.extend(findings.into_iter().map(finding_to_candidate)),
            Err(e) => tracing::warn!(analyzer = analyzer.name(), error = %e, "analyzer failed"),
        }
    }
    out
}

fn finding_to_candidate(f: aster_analyzers::Finding) -> Candidate {
    let severity = match f.severity {
        aster_analyzers::Severity::Error => "high",
        aster_analyzers::Severity::Warning => "medium",
        aster_analyzers::Severity::Info => "low",
    };
    Candidate {
        file: f.file,
        line: f.line as i32,
        defect_class: f.tool.clone(),
        severity: severity.to_string(),
        title: f.rule,
        failure_scenario: f.message,
        suggestion: format!("flagged by {}", f.tool),
        code_snippet: None,
        source: CandidateSource::Static,
    }
}

async fn hypothesize(
    deps: &ReviewDeps,
    repo: &str,
    diff: &str,
    sink: &ProgressSink,
) -> Result<Vec<Candidate>> {
    let content = complete(
        deps,
        deps.config.hypothesis_model.as_deref(),
        prompts::HYPOTHESIS_SYSTEM_PROMPT,
        &prompts::hypothesis_user_prompt(repo, &deps.config.focus_areas, diff),
        sink,
        "hypothesize",
    )
    .await?;
    let json = extract_json(&content);
    let list: CandidateList = match serde_json::from_str(&json) {
        Ok(list) => list,
        // A dropped stream leaves the array unterminated with complete objects
        // inside. Salvage those rather than failing the whole review.
        Err(e) => match salvage_candidates(&json) {
            Some(list) => {
                tracing::warn!(
                    salvaged = list.candidates.len(),
                    "candidate JSON was truncated; recovered complete entries"
                );
                list
            }
            None => {
                return Err(anyhow::anyhow!(
                    "failed to parse candidates: {e}; raw: {json}"
                ));
            }
        },
    };
    let raw = list.candidates.len();
    let kept: Vec<Candidate> = list
        .candidates
        .into_iter()
        .filter(|c| !c.failure_scenario.trim().is_empty())
        .collect();
    if raw > kept.len() {
        tracing::debug!(
            dropped = raw - kept.len(),
            "candidates dropped by scenario gate"
        );
    }
    // A genuinely clean diff and a model that returned an unexpected shape both
    // land here as zero candidates. The bad-shape case is loud (parse error
    // above), but an empty `candidates` array is silent, so surface it.
    if raw == 0 {
        tracing::warn!(
            raw_len = content.len(),
            "hypothesis pass produced zero candidates; diff may be clean or the model returned an empty set"
        );
    }
    Ok(kept)
}

const EVIDENCE_WINDOW: i32 = 25;
const MAX_REFERENCE_HITS: usize = 8;

/// Assemble the minimal working set for a candidate: the changed hunk it points
/// at, a source window, the enclosing symbol, and (when an index is present) the
/// definition, its callers, and any tests that reference it. This is retrieval,
/// not stuffing: each section is bounded and the whole thing is capped to
/// `max_evidence_bytes`.
async fn retrieve_evidence(
    deps: &ReviewDeps,
    repo_root: Option<&Path>,
    diff: &str,
    candidate: &Candidate,
) -> String {
    let mut out = String::new();

    if let Some(hunk) = diff_for_file(diff, &candidate.file) {
        out.push_str("--- changed hunk (what the diff touched) ---\n");
        out.push_str(&hunk);
        out.push('\n');
    }

    if let Some(window) = source_window(repo_root, &candidate.file, candidate.line) {
        out.push_str(&format!(
            "--- {} (around line {}) ---\n",
            candidate.file, candidate.line
        ));
        out.push_str(&window);
        out.push('\n');
    }

    if let Some(index) = deps.index.as_ref() {
        let symbols = index
            .symbols_in_file(&candidate.file)
            .await
            .unwrap_or_default();
        if let Some(sym) = enclosing_symbol(&symbols, candidate.line) {
            out.push_str(&format!(
                "--- enclosing symbol: {} {} @ {}:{} ---\n",
                sym.kind.as_deref().unwrap_or("symbol"),
                sym.name,
                sym.path,
                sym.start_line.unwrap_or(0)
            ));

            if is_simple_ident(&sym.name) {
                // Definition of the symbol, which may live in another file (e.g.
                // a type the scenario depends on).
                if let Ok(defs) = index.find_symbol(&sym.name, 3).await {
                    for def in defs.iter().filter(|d| d.path != candidate.file) {
                        if let Some(snippet) = def.snippet.as_deref() {
                            out.push_str(&format!(
                                "--- definition: {} @ {}:{} ---\n{}\n",
                                def.name,
                                def.path,
                                def.start_line.unwrap_or(0),
                                first_lines(snippet, 20)
                            ));
                        }
                    }
                }

                // Callers and tests, retrieved from the index's snippet FTS
                // rather than walking the whole repo per candidate. Skipped for
                // common/short identifiers, where a name match is mostly noise.
                if !is_common_ident(&sym.name) {
                    let (callers, tests) =
                        references_via_index(index, &sym.name, &candidate.file).await;
                    if !callers.is_empty() {
                        out.push_str("--- callers / references ---\n");
                        for hit in callers {
                            out.push_str(&hit);
                            out.push('\n');
                        }
                    }
                    if !tests.is_empty() {
                        out.push_str("--- tests referencing this symbol ---\n");
                        for hit in tests {
                            out.push_str(&hit);
                            out.push('\n');
                        }
                    }
                }
            }
        }
    }

    clamp_bytes(out, deps.config.max_evidence_bytes)
}

/// Find the symbol whose span contains `line`, falling back to the nearest.
fn enclosing_symbol(
    symbols: &[aster_index::SymbolHit],
    line: i32,
) -> Option<&aster_index::SymbolHit> {
    let line = line as i64;
    symbols
        .iter()
        .filter(|s| s.start_line.unwrap_or(i64::MAX) <= line && line <= s.end_line.unwrap_or(0))
        .min_by_key(|s| s.end_line.unwrap_or(0) - s.start_line.unwrap_or(0))
        .or_else(|| {
            symbols
                .iter()
                .min_by_key(|s| (s.start_line.unwrap_or(0) - line).abs())
        })
}

/// Find symbols whose name or body references `name` via the index's snippet
/// FTS, splitting them into callers and tests. This replaces a per-candidate
/// full-repo grep walk: symbol-granularity, not exact line, but no filesystem
/// traversal. The definition symbol itself is excluded (surfaced separately).
async fn references_via_index(
    index: &SqliteCodeIndex,
    name: &str,
    def_file: &str,
) -> (Vec<String>, Vec<String>) {
    // Quote the name so it is a phrase literal, never an FTS operator (AND/OR)
    // or split oddly on punctuation.
    let hits = index
        .lexical(&format!("\"{name}\""), 32)
        .await
        .unwrap_or_default();

    let mut callers = Vec::new();
    let mut tests = Vec::new();
    for hit in hits {
        let path = hit.path.replace('\\', "/");
        // The definition of this exact symbol is not a reference to it.
        if path.ends_with(def_file) && hit.name == name {
            continue;
        }
        let line = hit.start_line.unwrap_or(0);
        let first = hit
            .snippet
            .as_deref()
            .map(|s| first_lines(s, 1))
            .unwrap_or_default();
        let rendered = format!("{}:{}: {} — {}", path, line, hit.name, first.trim());
        if is_test_path(&path) {
            if tests.len() < MAX_REFERENCE_HITS {
                tests.push(rendered);
            }
        } else if callers.len() < MAX_REFERENCE_HITS {
            callers.push(rendered);
        }
        if callers.len() >= MAX_REFERENCE_HITS && tests.len() >= MAX_REFERENCE_HITS {
            break;
        }
    }
    (callers, tests)
}

/// Identifiers too common or too short for a reference search to be meaningful:
/// a name match on `get`/`new`/`run` is mostly noise, so skip caller retrieval.
fn is_common_ident(name: &str) -> bool {
    if name.len() < 4 {
        return true;
    }
    matches!(
        name.to_ascii_lowercase().as_str(),
        "main"
            | "self"
            | "next"
            | "from"
            | "into"
            | "iter"
            | "name"
            | "value"
            | "build"
            | "clone"
            | "drop"
            | "default"
            | "unwrap"
            | "result"
            | "error"
            | "index"
            | "parse"
            | "push"
            | "insert"
            | "get"
            | "set"
    )
}

fn is_test_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    let in_test_dir = lower.split('/').any(|seg| {
        matches!(seg, "test" | "tests" | "spec" | "specs" | "__tests__") || seg.starts_with("test_")
    });
    in_test_dir
        || lower.contains("_test.")
        || lower.contains(".test.")
        || lower.contains(".spec.")
        || lower.contains("_spec.")
}

/// Only identifiers we can safely turn into a `\b..\b` regex without escaping or
/// risking a pathological pattern.
fn is_simple_ident(name: &str) -> bool {
    !name.is_empty()
        && name.chars().all(|c| c.is_alphanumeric() || c == '_')
        && name
            .chars()
            .next()
            .is_some_and(|c| c.is_alphabetic() || c == '_')
}

fn first_lines(text: &str, n: usize) -> String {
    text.lines().take(n).collect::<Vec<_>>().join("\n")
}

fn clamp_bytes(mut s: String, max: usize) -> String {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s.truncate(end);
    s.push_str("\n... [evidence truncated]\n");
    s
}

/// Extract the unified-diff section for a single file: the hunks under the
/// header whose `+++ b/<path>` matches `file`. Returns None if the file is not
/// present in the diff.
fn diff_for_file(diff: &str, file: &str) -> Option<String> {
    let target = file.replace('\\', "/");
    let mut collecting = false;
    // Once inside a hunk body, `+++ `/`--- ` lines are added/removed *content*
    // (a source line whose text starts with `++ `/`-- `), not file headers, so
    // they must not be reinterpreted. `@@` opens a hunk; `diff --git` closes the
    // file section and resets header parsing.
    let mut in_hunk = false;
    let mut out = String::new();
    for line in diff.lines() {
        if line.starts_with("diff --git ") {
            if collecting {
                break;
            }
            in_hunk = false;
            continue;
        }
        if line.starts_with("@@") {
            in_hunk = true;
            if collecting {
                out.push_str(line);
                out.push('\n');
            }
            continue;
        }
        if !in_hunk {
            if let Some(path) = line.strip_prefix("+++ ") {
                let path = path.trim_start_matches("b/").trim();
                collecting = paths_match(path, &target);
                continue;
            }
            if line.starts_with("--- ") {
                continue;
            }
        }
        if collecting {
            out.push_str(line);
            out.push('\n');
        }
    }
    if out.trim().is_empty() {
        None
    } else {
        Some(out)
    }
}

/// Match diff paths on component boundaries so `foo.rs` does not match
/// `myfoo.rs`. A suffix only counts when it aligns to a `/` in the longer path.
fn paths_match(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    let suffix_ok = |long: &str, short: &str| {
        long.strip_suffix(short)
            .is_some_and(|prefix| prefix.ends_with('/'))
    };
    suffix_ok(a, b) || suffix_ok(b, a)
}

fn source_window(repo_root: Option<&Path>, file: &str, line: i32) -> Option<String> {
    let path = repo_root?.join(file);
    let content = std::fs::read_to_string(&path).ok()?;
    let lines: Vec<&str> = content.lines().collect();
    if lines.is_empty() {
        return None;
    }
    let start = (line - EVIDENCE_WINDOW).max(1) as usize;
    let end = ((line + EVIDENCE_WINDOW) as usize).min(lines.len());
    let mut out = String::new();
    for (i, text) in lines.iter().enumerate().take(end).skip(start - 1) {
        let n = i + 1;
        let marker = if n as i32 == line { ">" } else { " " };
        out.push_str(&format!("{marker}{n:>5} | {text}\n"));
    }
    Some(out)
}

async fn verify(
    deps: &ReviewDeps,
    candidate: &Candidate,
    evidence: &str,
    sink: &ProgressSink,
) -> Result<Verdict> {
    let content = complete(
        deps,
        deps.config.verify_model.as_deref(),
        prompts::VERIFY_SYSTEM_PROMPT,
        &prompts::verify_user_prompt(
            &candidate.title,
            &candidate.defect_class,
            &candidate.failure_scenario,
            candidate.code_snippet.as_deref().unwrap_or(""),
            evidence,
        ),
        sink,
        "verify",
    )
    .await?;
    let json = extract_json(&content);
    serde_json::from_str(&json)
        .map_err(|e| anyhow::anyhow!("failed to parse verdict: {e}; raw: {json}"))
}

fn shape(candidate: Candidate, verdict: Verdict) -> Finding {
    let description = if verdict.explanation.trim().is_empty() {
        candidate.failure_scenario
    } else {
        verdict.explanation
    };
    Finding {
        file_path: candidate.file,
        line: candidate.line,
        start_line: None,
        side: Some("right".to_string()),
        severity: candidate.severity,
        category: candidate.defect_class,
        title: candidate.title,
        description,
        suggestion: candidate.suggestion,
        code_snippet: candidate.code_snippet,
        confidence: Some(verdict.confidence),
    }
}

fn severity_weight(severity: &str) -> i32 {
    match severity {
        "critical" => 5,
        "high" => 4,
        "medium" => 3,
        "low" => 2,
        _ => 1,
    }
}

fn finding_rank(f: &Finding) -> f32 {
    severity_weight(&f.severity) as f32 * f.confidence.unwrap_or(0.5)
}

/// Two findings describe the same defect if they sit on the same file and line
/// and their titles clearly overlap. Title overlap (not category) is the signal,
/// so a defect surfaced by both a static analyzer and the model (different
/// categories, similar wording) collapses, while two genuinely different bugs on
/// the same line (different wording) are both kept.
fn same_defect(a: &Finding, b: &Finding) -> bool {
    a.file_path == b.file_path && a.line == b.line && titles_overlap(&a.title, &b.title)
}

/// Loose title similarity: the two share at least two meaningful (>=4 char)
/// words, or one title's word set is contained in the other's.
fn titles_overlap(a: &str, b: &str) -> bool {
    let words = |s: &str| -> std::collections::HashSet<String> {
        s.to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| w.len() >= 4)
            .map(str::to_string)
            .collect()
    };
    let (wa, wb) = (words(a), words(b));
    if wa.is_empty() || wb.is_empty() {
        return false;
    }
    let shared = wa.intersection(&wb).count();
    shared >= 2 || shared == wa.len().min(wb.len())
}

/// Phase 4 (SHAPE): dedup then rank. Findings describing the same defect (see
/// `same_defect`) collapse to the higher-ranked survivor, so a bug found by both
/// a static analyzer and the model reports once, while distinct bugs on the same
/// line are preserved. The result is ordered by `severity x confidence`.
fn shape_report(mut ordered: Vec<(usize, Finding)>) -> Vec<Finding> {
    // Restore hypothesis order first so dedup is deterministic regardless of
    // verification timing.
    ordered.sort_by_key(|(i, _)| *i);

    let mut deduped: Vec<Finding> = Vec::new();
    for (_, finding) in ordered {
        if let Some(existing) = deduped.iter_mut().find(|e| same_defect(e, &finding)) {
            if finding_rank(&finding) > finding_rank(existing) {
                *existing = finding;
            }
        } else {
            deduped.push(finding);
        }
    }

    deduped.sort_by(|a, b| {
        finding_rank(b)
            .partial_cmp(&finding_rank(a))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    deduped
}

async fn complete(
    deps: &ReviewDeps,
    model: Option<&str>,
    system: &str,
    user: &str,
    sink: &ProgressSink,
    stage: &str,
) -> Result<String> {
    let model = model.unwrap_or(&deps.ai_client.model);
    match deps
        .ai_client
        .complete_stream_with(model, system, user, 0.0, |delta| {
            progress::emit(
                sink,
                Progress::Token {
                    stage: stage.to_string(),
                    delta: delta.to_string(),
                },
            );
        })
        .await
    {
        Ok(content) => Ok(content),
        // Not every OpenAI-compatible endpoint honors `stream`. Fall back to a
        // single-shot request so the pipeline still works, just without tokens.
        Err(e) => {
            tracing::debug!(stage, error = %e, "stream failed; falling back to non-streaming");
            deps.ai_client.complete_with(model, system, user, 0.0).await
        }
    }
}

fn truncate(diff: &str, max_bytes: usize) -> String {
    if diff.len() <= max_bytes {
        return diff.to_string();
    }
    // Slice on a char boundary so a multi-byte codepoint at the cut point does
    // not panic. Walk back from max_bytes until we land on a boundary.
    let mut end = max_bytes;
    while end > 0 && !diff.is_char_boundary(end) {
        end -= 1;
    }
    format!(
        "{}\n... [diff truncated at {} bytes]",
        &diff[..end],
        max_bytes
    )
}

/// Recover complete `Candidate` objects from truncated candidate JSON (a
/// stream that died mid-response leaves the `candidates` array unterminated).
/// Walks the array with a string- and escape-aware depth counter, parsing each
/// balanced top-level object individually and dropping the incomplete tail.
/// Returns `None` when nothing whole could be recovered.
fn salvage_candidates(json: &str) -> Option<CandidateList> {
    let key = json.find("\"candidates\"")?;
    let array_start = json[key..].find('[')? + key;

    let mut candidates = Vec::new();
    let mut depth = 0usize;
    let mut object_start = None;
    let mut in_string = false;
    let mut escaped = false;

    for (i, c) in json[array_start..].char_indices() {
        let pos = array_start + i;
        if in_string {
            match c {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match c {
            '"' => in_string = true,
            '{' => {
                if depth == 0 {
                    object_start = Some(pos);
                }
                depth += 1;
            }
            '}' => {
                depth = depth.saturating_sub(1);
                if depth == 0
                    && let Some(start) = object_start.take()
                    && let Ok(c) = serde_json::from_str::<Candidate>(&json[start..=pos])
                {
                    candidates.push(c);
                }
            }
            ']' if depth == 0 => break,
            _ => {}
        }
    }

    if candidates.is_empty() {
        None
    } else {
        Some(CandidateList { candidates })
    }
}

// Models often wrap JSON in markdown fences or prose.
fn extract_json(content: &str) -> String {
    let trimmed = content.trim();
    let body = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .map(|s| s.trim_end_matches("```").trim())
        .unwrap_or(trimmed);
    let start = body.find(['{', '[']);
    let end = body.rfind(['}', ']']);
    match (start, end) {
        (Some(s), Some(e)) if e >= s => body[s..=e].to_string(),
        _ => body.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aster_models::Finding;

    fn finding(file: &str, line: i32, category: &str, severity: &str, conf: f32) -> Finding {
        Finding {
            file_path: file.to_string(),
            line,
            start_line: None,
            side: Some("right".to_string()),
            severity: severity.to_string(),
            category: category.to_string(),
            title: format!("{category} @ {file}:{line}"),
            description: String::new(),
            suggestion: String::new(),
            code_snippet: None,
            confidence: Some(conf),
        }
    }

    #[test]
    fn truncate_never_splits_utf8() {
        // A string of multi-byte chars; cutting at an arbitrary byte would panic
        // without the char-boundary walk.
        let s = "é".repeat(1000);
        for max in [1, 2, 3, 101, 999] {
            let _ = truncate(&s, max);
        }
    }

    #[test]
    fn salvage_candidates_recovers_complete_objects_from_truncated_array() {
        // The exact failure shape from production: the stream died after the
        // second object, so the array (and root object) never close.
        let truncated = r#"{"candidates":[{"file":"a.rs","line":208,"defect_class":"correctness","severity":"critical","title":"t1","failure_scenario":"s1","suggestion":"f1","code_snippet":"c1"},{"file":"b.rs","line":300,"defect_class":"correctness","severity":"critical","title":"t2","failure_scenario":"s2","suggestion":"f2"},{"file":"c.rs","line":1,"defect_class":"perf","severity":"low","title":"t3","failure_sc"#;
        let list = salvage_candidates(truncated).unwrap();
        assert_eq!(list.candidates.len(), 2);
        assert_eq!(list.candidates[0].file, "a.rs");
        assert_eq!(list.candidates[1].line, 300);
    }

    #[test]
    fn salvage_candidates_ignores_braces_inside_strings() {
        let tricky = r#"{"candidates":[{"file":"a.rs","line":1,"defect_class":"x","severity":"low","title":"has } and { and \" quote","failure_scenario":"s","suggestion":"f"},{"file":"b.rs","#;
        let list = salvage_candidates(tricky).unwrap();
        assert_eq!(list.candidates.len(), 1);
        assert_eq!(list.candidates[0].title, "has } and { and \" quote");
    }

    #[test]
    fn salvage_candidates_returns_none_when_nothing_whole() {
        assert!(salvage_candidates(r#"{"candidates":[{"file":"a.rs","li"#).is_none());
        assert!(salvage_candidates("not json at all").is_none());
    }

    #[test]
    fn extract_json_strips_fences_and_prose() {
        let fenced = "```json\n{\"a\":1}\n```";
        assert_eq!(extract_json(fenced), "{\"a\":1}");
        let prose = "Sure, here it is: {\"a\":1} hope that helps";
        assert_eq!(extract_json(prose), "{\"a\":1}");
    }

    #[test]
    fn diff_for_file_extracts_matching_section() {
        let diff = "diff --git a/src/foo.rs b/src/foo.rs\n\
                    --- a/src/foo.rs\n\
                    +++ b/src/foo.rs\n\
                    @@ -1,2 +1,2 @@\n\
                    -let x = 1;\n\
                    +let x = 2;\n\
                    diff --git a/src/bar.rs b/src/bar.rs\n\
                    --- a/src/bar.rs\n\
                    +++ b/src/bar.rs\n\
                    @@ -1 +1 @@\n\
                    +other\n";
        let foo = diff_for_file(diff, "src/foo.rs").unwrap();
        assert!(foo.contains("+let x = 2;"));
        assert!(!foo.contains("+other"));
        assert!(diff_for_file(diff, "src/missing.rs").is_none());
    }

    #[test]
    fn diff_for_file_does_not_suffix_false_match() {
        // `foo.rs` must NOT pull `myfoo.rs`'s hunk.
        let diff = "diff --git a/src/myfoo.rs b/src/myfoo.rs\n\
                    --- a/src/myfoo.rs\n\
                    +++ b/src/myfoo.rs\n\
                    @@ -1 +1 @@\n\
                    +wrong file\n";
        assert!(diff_for_file(diff, "foo.rs").is_none());
    }

    #[test]
    fn diff_for_file_keeps_body_lines_that_look_like_headers() {
        // An added line whose content is `++ Heading` renders as `+++ Heading`
        // and must be treated as body, not a file header that stops collection.
        let diff = "diff --git a/README.md b/README.md\n\
                    --- a/README.md\n\
                    +++ b/README.md\n\
                    @@ -1,2 +1,3 @@\n\
                     intro\n\
                    +++ Installation\n\
                    +done\n";
        let out = diff_for_file(diff, "README.md").unwrap();
        assert!(out.contains("+++ Installation"));
        assert!(out.contains("+done"));
    }

    #[test]
    fn paths_match_respects_component_boundary() {
        assert!(paths_match("src/foo.rs", "foo.rs"));
        assert!(paths_match("foo.rs", "foo.rs"));
        assert!(!paths_match("src/myfoo.rs", "foo.rs"));
        assert!(!paths_match("foobar.rs", "bar.rs"));
    }

    #[test]
    fn shape_report_keeps_distinct_findings_on_same_line() {
        let mut a = finding("a.rs", 10, "correctness", "high", 0.9);
        a.title = "off-by-one in loop bound".into();
        let mut b = finding("a.rs", 10, "correctness", "high", 0.9);
        b.title = "missing null check".into();
        let out = shape_report(vec![(0, a), (1, b)]);
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn shape_report_dedups_same_defect_keeps_higher_rank() {
        let ordered = vec![
            (0, finding("a.rs", 10, "correctness", "low", 0.6)),
            (1, finding("a.rs", 10, "correctness", "high", 0.9)),
            (2, finding("a.rs", 11, "correctness", "low", 0.5)),
        ];
        let out = shape_report(ordered);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].severity, "high");
    }

    #[test]
    fn shape_report_merges_static_and_model_finding_on_same_defect() {
        // A static analyzer (category = tool name) and the model flag the same
        // SQL-injection defect on the same line with overlapping titles.
        let mut sast = finding("db.rs", 42, "semgrep", "high", 0.9);
        sast.title = "sql-injection".into();
        let mut llm = finding("db.rs", 42, "security", "critical", 0.8);
        llm.title = "SQL injection in query builder".into();
        let out = shape_report(vec![(0, sast), (1, llm)]);
        assert_eq!(out.len(), 1, "same defect from two sources reports once");
    }

    #[test]
    fn titles_overlap_matches_same_defect_only() {
        assert!(titles_overlap(
            "sql-injection",
            "SQL injection in query builder"
        ));
        assert!(titles_overlap("panic on unwrap", "unwrap panic"));
        assert!(!titles_overlap(
            "off-by-one in loop bound",
            "missing null check"
        ));
        assert!(!titles_overlap("database error", "error handling"));
    }

    #[test]
    fn is_common_ident_skips_noise_names() {
        assert!(is_common_ident("get"));
        assert!(is_common_ident("new"));
        assert!(is_common_ident("id"));
        assert!(is_common_ident("unwrap"));
        assert!(!is_common_ident("charge_customer"));
        assert!(!is_common_ident("validate_token"));
    }

    #[test]
    fn shape_report_ranks_by_severity_at_equal_confidence() {
        let ordered = vec![
            (0, finding("a.rs", 1, "x", "low", 0.9)),
            (1, finding("b.rs", 1, "x", "critical", 0.9)),
            (2, finding("c.rs", 1, "x", "medium", 0.9)),
        ];
        let out = shape_report(ordered);
        assert_eq!(out[0].severity, "critical");
        assert_eq!(out[1].severity, "medium");
        assert_eq!(out[2].severity, "low");
    }

    #[test]
    fn shape_report_confidence_can_outrank_severity() {
        // severity x confidence: a low-confidence critical (5*0.2=1.0) ranks
        // below a high-confidence medium (3*0.9=2.7). Confidence matters.
        let ordered = vec![
            (0, finding("a.rs", 1, "x", "critical", 0.2)),
            (1, finding("b.rs", 1, "x", "medium", 0.9)),
        ];
        let out = shape_report(ordered);
        assert_eq!(out[0].severity, "medium");
    }

    #[test]
    fn is_simple_ident_rejects_operators_and_leading_digits() {
        assert!(is_simple_ident("foo_bar"));
        assert!(is_simple_ident("_private"));
        assert!(!is_simple_ident("1abc"));
        assert!(!is_simple_ident("a.b"));
        assert!(!is_simple_ident(""));
        assert!(!is_simple_ident("a(b)"));
    }

    #[test]
    fn is_test_path_detects_common_conventions() {
        assert!(is_test_path("src/foo_test.rs"));
        assert!(is_test_path("tests/integration.rs"));
        assert!(is_test_path("src/__tests__/x.ts"));
        assert!(is_test_path("app/foo.spec.ts"));
        assert!(!is_test_path("src/foo.rs"));
    }
}
