//! Rebuilds turns and tool rounds from a flat transcript, so the report can
//! talk about round-trips and batching rather than raw messages.

use std::collections::HashMap;

use aster_persist::{SessionTranscript, TranscriptEvent};
use chrono::{DateTime, Utc};

/// Results that told the model nothing, so the round that produced them was
/// spent for no information. Compared against the whole trimmed result.
const BARREN: &[&str] = &["no matches", "no files matched", "no results"];

/// Tools that block on a human. Their duration is the user thinking, not
/// aster working, so it never counts towards active time.
const WAITS_ON_USER: &[&str] = &["ask_user", "exit_plan_mode"];

pub struct Call {
    pub tool: String,
    /// `None` when the transcript ends before the result was recorded.
    pub duration: Option<f64>,
    pub result_chars: usize,
    pub barren: bool,
}

impl Call {
    pub fn waits_on_user(&self) -> bool {
        WAITS_ON_USER.contains(&self.tool.as_str())
    }
}

/// Everything between one user message and the next.
pub struct Turn {
    pub session: String,
    pub model: Option<String>,
    pub started: DateTime<Utc>,
    pub ended: DateTime<Utc>,
    /// Model round-trips: the gap from each user or tool message to the
    /// assistant message it produced. Excludes tool execution by construction.
    pub latencies: Vec<f64>,
    /// Calls in each assistant message that carried at least one, in order.
    pub batches: Vec<usize>,
    pub calls: Vec<Call>,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
}

impl Turn {
    pub fn rounds(&self) -> usize {
        self.batches.len()
    }

    /// Wall time from the user's message to the last thing aster did.
    pub fn wall(&self) -> f64 {
        (self.ended - self.started).num_milliseconds() as f64 / 1000.0
    }

    /// Wall time with human waits removed, which is the part aster owns.
    pub fn active(&self) -> f64 {
        let waiting: f64 = self
            .calls
            .iter()
            .filter(|c| c.waits_on_user())
            .filter_map(|c| c.duration)
            .sum();
        (self.wall() - waiting).max(0.0)
    }
}

pub fn barren(result: &str) -> bool {
    let result = result.trim();
    if result.is_empty() {
        return true;
    }
    if BARREN.iter().any(|m| result.eq_ignore_ascii_case(m)) {
        return true;
    }
    // A wrong path guess: the note names nearby paths but answers nothing.
    result.starts_with("note:") && result.contains("does not exist.")
}

/// Split one transcript into turns. Messages before the first user message
/// (a seeded system prompt, an imported header) belong to no turn and are
/// dropped rather than attributed to the first one.
pub fn turns(transcript: &SessionTranscript) -> Vec<Turn> {
    let model = transcript.meta.model.clone();
    let session = transcript.meta.id.clone();
    let mut turns: Vec<Turn> = Vec::new();
    let mut pending: HashMap<String, (String, DateTime<Utc>)> = HashMap::new();
    let mut previous: Option<DateTime<Utc>> = None;

    for event in &transcript.events {
        let TranscriptEvent::Message(message) = event else {
            continue;
        };
        match message.role.as_str() {
            "user" => {
                turns.push(Turn {
                    session: session.clone(),
                    model: model.clone(),
                    started: message.ts,
                    ended: message.ts,
                    latencies: Vec::new(),
                    batches: Vec::new(),
                    calls: Vec::new(),
                    prompt_tokens: 0,
                    completion_tokens: 0,
                });
                previous = Some(message.ts);
            }
            "assistant" => {
                let Some(turn) = turns.last_mut() else {
                    continue;
                };
                turn.ended = message.ts;
                if let Some(at) = previous {
                    turn.latencies.push(seconds(at, message.ts));
                }
                if !message.tool_calls.is_empty() {
                    turn.batches.push(message.tool_calls.len());
                }
                for call in &message.tool_calls {
                    pending.insert(call.id.clone(), (call.function.name.clone(), message.ts));
                }
                if let Some(usage) = message.usage {
                    turn.prompt_tokens += usage.prompt_tokens;
                    turn.completion_tokens += usage.completion_tokens;
                }
                previous = Some(message.ts);
            }
            "tool" => {
                let Some(turn) = turns.last_mut() else {
                    continue;
                };
                turn.ended = message.ts;
                let result = message.content.as_deref().unwrap_or_default();
                let started = message
                    .tool_call_id
                    .as_ref()
                    .and_then(|id| pending.remove(id));
                let (tool, duration) = match started {
                    Some((tool, at)) => (tool, Some(seconds(at, message.ts))),
                    None => ("unknown".to_string(), None),
                };
                turn.calls.push(Call {
                    tool,
                    duration,
                    result_chars: result.len(),
                    barren: barren(result),
                });
                previous = Some(message.ts);
            }
            _ => {}
        }
    }
    turns
}

fn seconds(from: DateTime<Utc>, to: DateTime<Utc>) -> f64 {
    ((to - from).num_milliseconds() as f64 / 1000.0).max(0.0)
}

#[cfg(test)]
#[path = "tests/turn_test.rs"]
pub(crate) mod tests;
