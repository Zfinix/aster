//! Client for TypeSafe's Jev, a System One evaluation model: state plus typed
//! questions in, typed answers with probabilities out. No text is generated.
//!
//! The endpoint is `POST {base_url}/v1/systemone` with model id `jev-latest`.
//! Every question is answered in parallel against the same state, so one call
//! covers a whole batch of decisions.

use std::collections::BTreeMap;
use std::time::Duration;

use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::json;

/// The state snapshot is capped so a runaway transcript cannot inflate the
/// request; anything past this is dropped from the tail.
pub const MAX_STATE_CHARS: usize = 4096;

/// One attempt, no retries: the caller falls back to its own judgment.
const ADVISE_TIMEOUT_SECS: u64 = 2;

/// A suggestion below this probability of its chosen option is ignored.
pub const ADVISE_CONFIDENCE_MIN: f64 = 0.8;

pub const ROUTE_CONFIDENCE_MIN: f64 = 0.6;

const DEFAULT_BASE_URL: &str = "https://api.typesafe.ai";
const DEFAULT_MODEL: &str = "jev-latest";

/// A typed question about the shared state.
#[derive(Debug, Clone)]
pub enum Question {
    /// The probability that the state satisfies the instructions.
    Boolean { instructions: String },
    /// One option from a named set, `(name, description)` pairs.
    Choice {
        instructions: String,
        criteria: Vec<(String, String)>,
    },
    /// A position on an ordered scale, lowest to highest.
    Score {
        instructions: String,
        criteria: Vec<String>,
    },
}

impl Question {
    fn to_json(&self) -> serde_json::Value {
        match self {
            // The API's tag for a boolean question is `noul`, not `boolean`.
            Question::Boolean { instructions } => json!({
                "type": "noul",
                "instructions": instructions,
            }),
            Question::Choice {
                instructions,
                criteria,
            } => json!({
                "type": "choice",
                "instructions": instructions,
                "criteria": criteria
                    .iter()
                    .map(|(name, description)| (name.as_str(), description.as_str()))
                    .collect::<BTreeMap<_, _>>(),
            }),
            Question::Score {
                instructions,
                criteria,
            } => json!({
                "type": "score",
                "instructions": instructions,
                "criteria": criteria,
            }),
        }
    }
}

/// The typed answer to one question, keyed by the question's id.
#[derive(Debug, Clone, PartialEq)]
pub enum Answer {
    Boolean {
        probability: f64,
    },
    Choice {
        choice: String,
        probabilities: BTreeMap<String, f64>,
        confidence: Option<f64>,
    },
    Score {
        score: f64,
        probabilities: BTreeMap<String, f64>,
        confidence: Option<f64>,
    },
}

#[derive(Debug, Deserialize)]
struct RawAnswer {
    #[serde(default)]
    probability: Option<f64>,
    // Noul answers carry their probability in a field named `noul`.
    #[serde(default)]
    noul: Option<f64>,
    #[serde(default)]
    choice: Option<String>,
    #[serde(default)]
    score: Option<f64>,
    #[serde(default)]
    probabilities: Option<BTreeMap<String, f64>>,
    #[serde(default)]
    confidence: Option<f64>,
    #[serde(default, rename = "type")]
    kind: Option<String>,
}

impl RawAnswer {
    fn into_answer(self) -> Result<Answer> {
        match self.kind.as_deref() {
            Some("noul") => Ok(Answer::Boolean {
                probability: self
                    .noul
                    .or(self.probability)
                    .context("noul answer without a probability")?,
            }),
            Some("choice") => Ok(Answer::Choice {
                choice: self.choice.context("choice answer without a choice")?,
                probabilities: self.probabilities.unwrap_or_default(),
                confidence: self.confidence,
            }),
            Some("score") => Ok(Answer::Score {
                score: self.score.context("score answer without a score")?,
                probabilities: self.probabilities.unwrap_or_default(),
                confidence: self.confidence,
            }),
            other => anyhow::bail!("unknown answer type {other:?}"),
        }
    }
}

pub struct JevClient {
    http: reqwest::Client,
    base_url: String,
    api_key: String,
    model: String,
}

impl JevClient {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            http: reqwest::Client::new(),
            base_url: DEFAULT_BASE_URL.to_string(),
            api_key: api_key.into(),
            model: DEFAULT_MODEL.to_string(),
        }
    }

    pub fn base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into().trim_end_matches('/').to_string();
        self
    }

    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.model = model.into();
        self
    }

    /// Evaluate every question against the same state in one round trip.
    pub async fn evaluate(
        &self,
        state: &str,
        questions: &BTreeMap<String, Question>,
    ) -> Result<BTreeMap<String, Answer>> {
        let questions: BTreeMap<&str, serde_json::Value> = questions
            .iter()
            .map(|(id, question)| (id.as_str(), question.to_json()))
            .collect();
        let response = self
            .http
            .post(format!("{}/v1/systemone", self.base_url))
            .bearer_auth(&self.api_key)
            .timeout(Duration::from_secs(ADVISE_TIMEOUT_SECS))
            .json(&json!({
                "model": self.model,
                "state": state,
                "questions": questions,
            }))
            .send()
            .await
            .context("jev request failed")?;
        let status = response.status();
        let body: serde_json::Value = response.json().await.context("jev reply was not JSON")?;
        if !status.is_success() {
            anyhow::bail!("jev returned {status}: {body}");
        }
        let raw: BTreeMap<String, RawAnswer> = serde_json::from_value(
            body.get("answers")
                .cloned()
                .context("jev reply has no answers")?,
        )
        .context("jev answers did not match the expected shape")?;
        raw.into_iter()
            .map(|(id, answer)| Ok((id, answer.into_answer()?)))
            .collect()
    }

    /// Ask what the agent loop should do next. Advisory only: `None` means no
    /// confident suggestion or any failure, and the caller keeps its own path.
    pub async fn advise_loop(&self, state: &LoopState) -> Option<LoopAction> {
        let mut questions = BTreeMap::new();
        questions.insert(
            "action".to_string(),
            Question::Choice {
                instructions: "What should the agent loop do next?".to_string(),
                criteria: vec![
                    (
                        "continue".to_string(),
                        "keep working on the task as it is".to_string(),
                    ),
                    (
                        "retry".to_string(),
                        "the current approach is failing; try a different one".to_string(),
                    ),
                    (
                        "ask_user".to_string(),
                        "a needed fact is missing; ask the user".to_string(),
                    ),
                    (
                        "stop".to_string(),
                        "the task is done or cannot progress; answer with what you have"
                            .to_string(),
                    ),
                ],
            },
        );
        let started = std::time::Instant::now();
        let answers = match self.evaluate(&state.state_prompt(), &questions).await {
            Ok(answers) => answers,
            Err(e) => {
                tracing::debug!("jev check failed: {e:#}");
                return None;
            }
        };
        let elapsed_ms = started.elapsed().as_millis();
        tracing::debug!(?answers, elapsed_ms, "jev answered");
        match answers.get("action")? {
            Answer::Choice {
                choice,
                probabilities,
                confidence,
            } => {
                let probability = probabilities
                    .get(choice)
                    .copied()
                    .or(*confidence)
                    .unwrap_or(0.0);
                if probability < ADVISE_CONFIDENCE_MIN {
                    return None;
                }
                Some(match choice.as_str() {
                    "continue" => LoopAction::Continue,
                    "retry" => LoopAction::Retry,
                    "ask_user" => LoopAction::AskUser,
                    "stop" => LoopAction::Stop,
                    _ => return None,
                })
            }
            _ => None,
        }
    }

    pub async fn route(&self, message: &str, entries: &[(String, String)]) -> Result<RoutePick> {
        let mut questions = BTreeMap::new();
        questions.insert(
            "entry".to_string(),
            Question::Choice {
                instructions: "Which model entry is best suited to answer this user message? \
                               Multi-step design, debugging, or refactoring work goes to a \
                               stronger entry; small mechanical asks go to a cheaper one."
                    .to_string(),
                criteria: entries.to_vec(),
            },
        );
        let state = format!("User message:\n{}", truncate(message, MAX_STATE_CHARS));
        let answers = self.evaluate(&state, &questions).await?;
        match answers.get("entry") {
            Some(Answer::Choice {
                choice,
                probabilities,
                confidence,
            }) => {
                anyhow::ensure!(
                    entries.iter().any(|(name, _)| name == choice),
                    "jev picked {choice:?}, which is not a declared entry"
                );
                let probability = probabilities
                    .get(choice)
                    .copied()
                    .or(*confidence)
                    .unwrap_or(0.0);
                Ok(RoutePick {
                    entry: choice.clone(),
                    probability,
                })
            }
            other => anyhow::bail!("jev returned no entry choice: {other:?}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RoutePick {
    pub entry: String,
    pub probability: f64,
}

/// What the agent loop should do at a round boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopAction {
    Continue,
    Retry,
    AskUser,
    Stop,
}

/// The loop facts the advisor sees. `recent` is the caller's bounded snapshot
/// of what happened in the last few rounds.
pub struct LoopState {
    pub round: usize,
    pub round_cap: usize,
    pub recent: String,
}

impl LoopState {
    fn state_prompt(&self) -> String {
        format!(
            "Tool round {} of a hard cap of {}.\nRecent activity:\n{}",
            self.round + 1,
            self.round_cap,
            truncate(&self.recent, MAX_STATE_CHARS),
        )
    }
}

fn truncate(text: &str, cap: usize) -> &str {
    match text.char_indices().nth(cap) {
        Some((index, _)) => &text[..index],
        None => text,
    }
}

#[cfg(test)]
#[path = "jev_tests.rs"]
mod tests;
