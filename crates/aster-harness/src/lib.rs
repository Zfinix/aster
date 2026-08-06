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

    // I/O-bound on the model call; verify bounded-concurrently and re-sort to
    // hypothesis order before shaping so output stays deterministic.
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

        // Static and hypothesized candidates share the same adversarial gate;
        // provenance rides along but no longer branches the output path.
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
        // A dropped stream leaves the array unterminated; salvage whole objects
        // rather than failing the whole review.
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
    // An empty `candidates` array is silent (unlike a parse error), so surface
    // it: clean diff or the model returned an empty set.
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

/// Assemble a candidate's working set (changed hunk, source window, enclosing
/// symbol, and indexed definition/callers/tests). Each section is bounded and
/// the whole is capped to `max_evidence_bytes`.
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
                // Definition may live in another file (e.g. a type the scenario depends on).
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

                // Skipped for common/short identifiers, where a name match is mostly noise.
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

/// Symbol whose span contains `line`, else the nearest.
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

/// Symbols referencing `name` via the index's snippet FTS, split into callers
/// and tests. Symbol-granularity, no filesystem walk; the definition symbol is
/// excluded (surfaced separately).
async fn references_via_index(
    index: &SqliteCodeIndex,
    name: &str,
    def_file: &str,
) -> (Vec<String>, Vec<String>) {
    // Quote so the name is a phrase literal, never an FTS operator (AND/OR).
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

/// Identifiers too common or short for reference search to be meaningful
/// (`get`/`new`/`run`), so caller retrieval is skipped.
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

/// Identifiers safe to turn into a `\b..\b` regex without escaping.
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

/// Hunks under the header whose `+++ b/<path>` matches `file`; None if absent.
fn diff_for_file(diff: &str, file: &str) -> Option<String> {
    let target = file.replace('\\', "/");
    let mut collecting = false;
    // Inside a hunk body, `+++ `/`--- ` lines are content, not file headers, so
    // header parsing is gated on `in_hunk`. `@@` opens a hunk; `diff --git`
    // closes the file section.
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

/// Match paths on component boundaries: a suffix counts only when it aligns to
/// a `/`, so `foo.rs` does not match `myfoo.rs`.
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

/// Same file, line, and overlapping titles. Title overlap (not category) is the
/// signal, so a static-analyzer and model hit on one defect collapse while two
/// differently-worded bugs on the same line are both kept.
fn same_defect(a: &Finding, b: &Finding) -> bool {
    a.file_path == b.file_path && a.line == b.line && titles_overlap(&a.title, &b.title)
}

/// Loose title similarity: share >=2 meaningful (>=4 char) words, or one word
/// set is contained in the other.
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

/// Dedup same-defect findings to the higher-ranked survivor, then order by
/// `severity x confidence`.
fn shape_report(mut ordered: Vec<(usize, Finding)>) -> Vec<Finding> {
    // Restore hypothesis order so dedup is deterministic regardless of verify timing.
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
        // Not every OpenAI-compatible endpoint honors `stream`; fall back to a
        // single-shot request.
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
    // Walk back to a char boundary so slicing does not split a codepoint.
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

/// Recover complete `Candidate` objects from truncated JSON by walking the
/// array with a string- and escape-aware depth counter, parsing each balanced
/// top-level object and dropping the incomplete tail. None if nothing whole.
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
#[path = "tests/lib_test.rs"]
mod tests;
