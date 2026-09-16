//! Chunked hypothesis fan-out: the diff is split into file-scoped chunks and
//! each chunk gets its own hypothesis model call, run concurrently. Recall
//! stays high because every chunk still sees its files' full hunks; the
//! serial whole-diff pass remains the fallback when chunking is off or the
//! diff has no file boundaries.

use anyhow::Result;
use futures_util::{StreamExt, stream};

use crate::models::{Candidate, CandidateList};
use crate::progress::ProgressSink;
use crate::{ReviewDeps, complete, extract_json, prompts, salvage_candidates};

const DIFF_HEADER: &str = "diff --git ";

/// Split the diff into file-scoped chunks of at most `max_bytes` bytes. A
/// file's hunks are never split across chunks, so a boundary never hides a
/// defect from the model that would have caught it whole.
fn chunk_diff(diff: &str, max_bytes: usize) -> Vec<String> {
    if max_bytes == 0 {
        return vec![diff.to_string()];
    }
    let mut chunks: Vec<String> = Vec::new();
    let mut current = String::new();
    for (i, part) in diff.split("\ndiff --git ").enumerate() {
        let file_diff = if i == 0 {
            part.to_string()
        } else {
            format!("{DIFF_HEADER}{part}")
        };
        if file_diff.trim().is_empty() {
            continue;
        }
        // A single file larger than the budget gets its own oversized chunk;
        // splitting inside its hunks would hide context from the model.
        if !current.is_empty() && current.len() + file_diff.len() > max_bytes {
            chunks.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push('\n');
        }
        current.push_str(&file_diff);
    }
    if !current.trim().is_empty() {
        chunks.push(current);
    }
    if chunks.is_empty() {
        chunks.push(diff.to_string());
    }
    chunks
}

/// Fan the hypothesis pass over file-scoped chunks, or fall back to the
/// single whole-diff call when chunking is disabled or pointless.
pub(crate) async fn hypothesize(
    deps: &ReviewDeps,
    repo: &str,
    diff: &str,
    sink: &ProgressSink,
) -> Result<Vec<Candidate>> {
    let max_chunk = deps.config.hypothesis_chunk_bytes;
    let fan_out = deps.config.hypothesis_concurrency > 1
        && max_chunk > 0
        && diff.len() > max_chunk
        && diff.contains(DIFF_HEADER);

    if !fan_out {
        return hypothesize_once(deps, repo, diff, sink).await;
    }

    let chunks = chunk_diff(diff, max_chunk);
    let total = chunks.len();
    let concurrency = deps.config.hypothesis_concurrency.max(1);

    let lists: Vec<(usize, Result<Vec<Candidate>>)> = stream::iter(chunks.into_iter().enumerate())
        .map(|(i, chunk)| async move {
            let result = hypothesize_once(deps, repo, &chunk, sink).await;
            (i, result)
        })
        .buffered(concurrency)
        .collect()
        .await;

    let mut candidates = Vec::new();
    let mut failed = 0usize;
    let mut first_error: Option<String> = None;
    for (i, result) in lists {
        match result {
            Ok(mut chunk_candidates) => candidates.append(&mut chunk_candidates),
            Err(e) => {
                // One bad chunk must not sink the review; recall is the point
                // of this stage, so surviving chunks still verify.
                failed += 1;
                first_error.get_or_insert_with(|| e.to_string());
                tracing::warn!(chunk = i, error = %e, "hypothesis chunk failed; continuing");
            }
        }
    }
    if candidates.is_empty() && failed == total {
        anyhow::bail!(
            "all {failed} hypothesis chunks failed; first error: {}",
            first_error.unwrap_or_default()
        );
    }
    if failed > 0 {
        tracing::warn!(
            failed,
            total,
            "some hypothesis chunks failed; recall reduced"
        );
    }
    Ok(candidates)
}

/// One model call over a diff (whole or chunked), with the shared parse,
/// salvage, and scenario-gate behavior.
async fn hypothesize_once(
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

#[cfg(test)]
#[path = "hypothesis_tests.rs"]
mod tests;
