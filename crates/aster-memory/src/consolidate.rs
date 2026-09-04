//! The consolidation pass: turns a finished session's transcript into durable
//! memory. It reads a bounded digest, asks the model for proposals, and applies
//! them through the journaled store; a `Consolidated` marker makes it idempotent.

use anyhow::{Context, Result};
use aster_ai::AiClient;
use aster_persist::{MemoryOp, MemoryStore};
use chrono::{DateTime, Duration, Utc};

use crate::digest::{self, SessionDigest};

pub const JOURNAL_WINDOW: Duration = Duration::days(30);

pub const CONSOLIDATE_SYSTEM: &str = "\
You turn the tail of a finished coding session into durable memory for the agent that ran it.
The agent's memory is a set of named markdown blocks (a name, a one-line description, and a
body) plus a project file of short facts.

Read the session digest and the current memory index. Propose updates that will actually help
a future session, and nothing else:
- NEW: a fact, convention, or procedure the session established that is not already in memory.
  Prefer one focused block per idea. A name is a short kebab-case slug; a description is one
  plain line; a body is 1-3 sentences of plain, specific prose.
- MERGE: two listed blocks that now say the same thing. Merge them into one NEW-style block and
  list the old names to archive.
- ARCHIVE: a listed block the session contradicted or superseded. Only when the session gave
  concrete evidence, never just because it is old.
- LESSON: a durable mistake-and-correction the session contained: the agent did something, the
  user corrected it, and the outcome differed. Record the corrected behavior, not the mistake.

Rules:
- No filler, no summaries of the conversation, no praise. Only facts a future session could act on.
- Never propose writing secrets, credentials, or private content.
- Prefer a few real blocks over many trivial ones. When nothing qualifies, return only a summary.
- Memory blocks are shown as: NAME — description. Match against them by name and meaning.

Respond with ONLY a JSON object of this exact shape, no markdown fences, nothing else:
{
  \"summary\": \"<1-2 sentences on what the session was and what memory changed>\",
  \"new\": [{\"name\": \"<kebab-case>\", \"description\": \"<one line>\", \"body\": \"<1-3 sentences>\"}],
  \"merge\": [{\"into_name\": \"<the NEW block name>\", \"archive\": [\"<old block>\"]}],
  \"archive\": [\"<block name>\"],
  \"lessons\": [{\"name\": \"<kebab-case>\", \"description\": \"<one line>\", \"body\": \"<the corrected behavior>\"}]
}
";

pub const DEFAULT_MIN_TURNS: usize = 6;

/// The full set of decisions a consolidation call may return. Every field is
/// optional; an empty proposal means "learn nothing from this session".
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Proposals {
    pub summary: Option<String>,
    #[serde(default)]
    pub new: Vec<NewBlock>,
    #[serde(default)]
    pub merge: Vec<Merge>,
    #[serde(default)]
    pub archive: Vec<String>,
    #[serde(default)]
    pub lessons: Vec<NewBlock>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct NewBlock {
    pub name: String,
    pub description: String,
    pub body: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Merge {
    pub into_name: String,
    #[serde(default)]
    pub archive: Vec<String>,
}

/// What one pass did, for tests and for a human reading the journal.
#[derive(Debug, Clone, Default)]
pub struct ApplyReport {
    pub wrote: Vec<String>,
    pub archived: Vec<String>,
}

/// The live-pass entry point: run consolidation for one session and apply it.
/// Returns the apply report, or `None` when the session is under the turn gate
/// or has nothing durable.
pub async fn consolidate_session(
    client: &AiClient,
    memory: &MemoryStore,
    transcript: &aster_persist::SessionTranscript,
    min_turns: usize,
) -> Result<Option<ApplyReport>> {
    if transcript.user_turn_count() < min_turns {
        return Ok(None);
    }
    let Some(digest) = digest::build(transcript) else {
        return Ok(None);
    };
    let proposals = propose(client, memory, &digest).await?;
    Ok(Some(apply(memory, &digest.session_id, &proposals)?))
}

/// Run the consolidation model call. Exposed for tests with a canned client.
pub async fn propose(
    client: &AiClient,
    memory: &MemoryStore,
    digest: &SessionDigest,
) -> Result<Proposals> {
    let index = current_index(memory);
    let user = format!(
        "{}\n\nCurrent memory index:\n{}\n\nConsolidate this session.",
        digest::render(digest),
        if index.is_empty() {
            "(empty)".to_string()
        } else {
            index.join("\n")
        }
    );
    let raw = client
        .complete(CONSOLIDATE_SYSTEM, &user, 0.0)
        .await
        .context("consolidation call failed")?;
    parse_proposals(&raw)
}

/// Parse and validate a raw model response. Lenient about markdown fences and
/// stray prose around the JSON, strict about the shape inside it.
pub fn parse_proposals(raw: &str) -> Result<Proposals> {
    let json = strip_fences(raw);
    let proposals: Proposals = serde_json::from_str(json).context("consolidation JSON invalid")?;
    // A real answer is at least one concrete action; an empty object means the
    // model judged the session unworthy. Keep it as a valid no-op.
    Ok(proposals)
}

fn strip_fences(raw: &str) -> &str {
    let trimmed = raw.trim();
    if let Some(rest) = trimmed.strip_prefix("```json") {
        return rest.strip_suffix("```").unwrap_or(rest).trim();
    }
    if let Some(rest) = trimmed.strip_prefix("```") {
        return rest.strip_suffix("```").unwrap_or(rest).trim();
    }
    trimmed
}

/// Apply proposals through the journaled store API. This is the audit point:
/// every learned block carries the session that produced it, and the pass ends
/// with a `Consolidated` marker so the startup sweep never re-runs it.
pub fn apply(memory: &MemoryStore, session_id: &str, proposals: &Proposals) -> Result<ApplyReport> {
    let mut report = ApplyReport::default();

    for block in proposals.new.iter().chain(proposals.lessons.iter()) {
        let name = valid_name(&block.name);
        let body = block.body.trim();
        if body.is_empty() {
            continue;
        }
        let description = block.description.trim();
        memory.remember_sourced(&name, description, body, session_id)?;
        report.wrote.push(name);
    }

    for target in &proposals.archive {
        let target = target.trim();
        if !target.is_empty() && memory.archive(target)? {
            report.archived.push(target.to_string());
        }
    }

    // A merge archives the superseded blocks; the merged replacement arrives as
    // a `new` block (the model writes the combined body once).
    for merge in &proposals.merge {
        for old in &merge.archive {
            let old = old.trim();
            if old.is_empty() {
                continue;
            }
            if memory.archive(old)? {
                report.archived.push(old.to_string());
            }
        }
    }

    let _ = memory.record_consolidated(session_id);
    Ok(report)
}

fn valid_name(name: &str) -> String {
    let name = name.trim();
    if name.is_empty() {
        return "note".to_string();
    }
    name.chars().take(80).collect()
}

fn current_index(memory: &MemoryStore) -> Vec<String> {
    memory
        .list()
        .unwrap_or_default()
        .iter()
        .take(aster_persist::MAX_INDEX_ENTRIES)
        .map(|b| format!("{} — {}", b.name, b.description))
        .collect()
}

/// Recent journal entries (memory ops across sessions) that mention a session,
/// used by callers to decide whether a session already consolidated.
pub fn already_consolidated(memory: &MemoryStore, session_id: &str) -> bool {
    memory
        .journal()
        .unwrap_or_default()
        .iter()
        .any(|e| e.op == MemoryOp::Consolidated && e.source_session.as_deref() == Some(session_id))
}

/// Sessions that ended but never got a `Consolidated` marker. This is what the
/// startup sweep consumes; it tolerates a missing journal entirely.
pub fn unconsolidated_sessions(memory: &MemoryStore, since: DateTime<Utc>) -> Result<Vec<String>> {
    let entries = memory.journal()?;
    let consolidated: std::collections::HashSet<&str> = entries
        .iter()
        .filter(|e| e.op == MemoryOp::Consolidated)
        .filter_map(|e| e.source_session.as_deref())
        .collect();
    let mut out = Vec::new();
    for entry in entries.iter().rev() {
        let Some(session) = entry.source_session.as_deref() else {
            continue;
        };
        if consolidated.contains(session) {
            continue;
        }
        if entry.ts < since {
            continue;
        }
        if matches!(entry.op, MemoryOp::Remember | MemoryOp::AppendProject) {
            out.push(session.to_string());
        }
    }
    out.sort();
    out.dedup();
    Ok(out)
}

#[cfg(test)]
#[path = "consolidate_tests.rs"]
mod tests;
