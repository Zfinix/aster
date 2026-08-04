use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use aster_ai::{Annotation, ChatMessage, ToolCall};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub const TRANSCRIPT_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TranscriptEvent {
    Session(SessionMeta),
    Message(MessageEvent),
    Summary(SummaryEvent),
    Eviction(EvictionEvent),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMeta {
    pub id: String,
    #[serde(default = "default_version")]
    pub v: u32,
    pub created_at: DateTime<Utc>,
    pub cwd: String,
    pub repo_root: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aster_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

fn default_version() -> u32 {
    TRANSCRIPT_VERSION
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageEvent {
    pub role: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    pub ts: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<EventUsage>,
    /// Web-search source citations, attached to assistant turns by the
    /// OpenRouter `web` plugin.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub annotations: Vec<Annotation>,
}

impl MessageEvent {
    fn now(role: &str) -> Self {
        Self {
            role: role.to_string(),
            content: None,
            tool_calls: Vec::new(),
            tool_call_id: None,
            ts: Utc::now(),
            usage: None,
            annotations: Vec::new(),
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            content: Some(content.into()),
            ..Self::now("user")
        }
    }

    pub fn system(content: impl Into<String>) -> Self {
        Self {
            content: Some(content.into()),
            ..Self::now("system")
        }
    }

    pub fn assistant(content: Option<String>, tool_calls: Vec<ToolCall>) -> Self {
        Self {
            content,
            tool_calls,
            ..Self::now("assistant")
        }
    }

    pub fn tool(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            content: Some(content.into()),
            tool_call_id: Some(tool_call_id.into()),
            ..Self::now("tool")
        }
    }

    pub fn with_usage(mut self, usage: Option<EventUsage>) -> Self {
        self.usage = usage;
        self
    }

    pub fn with_annotations(mut self, annotations: Vec<Annotation>) -> Self {
        self.annotations = annotations;
        self
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct EventUsage {
    #[serde(default)]
    pub prompt_tokens: u64,
    #[serde(default)]
    pub completion_tokens: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SummaryEvent {
    pub content: String,
    pub replaces_through: usize,
    pub ts: DateTime<Utc>,
}

impl SummaryEvent {
    pub fn new(content: impl Into<String>, replaces_through: usize) -> Self {
        Self {
            content: content.into(),
            replaces_through,
            ts: Utc::now(),
        }
    }
}

/// One message dropped or stubbed out by the context budget, kept in the
/// transcript so a run can explain exactly what the model no longer saw.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvictionEvent {
    pub reason: String,
    pub role: String,
    /// Position in the request being built when the eviction happened.
    pub index: usize,
    pub chars: usize,
    pub ts: DateTime<Utc>,
}

impl EvictionEvent {
    pub fn new(
        reason: impl Into<String>,
        role: impl Into<String>,
        index: usize,
        chars: usize,
    ) -> Self {
        Self {
            reason: reason.into(),
            role: role.into(),
            index,
            chars,
            ts: Utc::now(),
        }
    }
}

pub struct SessionWriter {
    file: BufWriter<File>,
    meta: SessionMeta,
    path: PathBuf,
}

impl SessionWriter {
    pub(crate) fn create(path: PathBuf, meta: SessionMeta) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .with_context(|| format!("opening {}", path.display()))?;
        let mut writer = Self {
            file: BufWriter::new(file),
            meta: meta.clone(),
            path,
        };
        writer.append(&TranscriptEvent::Session(meta))?;
        Ok(writer)
    }

    pub(crate) fn reopen(path: PathBuf, meta: SessionMeta) -> Result<Self> {
        let file = OpenOptions::new()
            .append(true)
            .open(&path)
            .with_context(|| format!("opening {}", path.display()))?;
        Ok(Self {
            file: BufWriter::new(file),
            meta,
            path,
        })
    }

    pub fn append(&mut self, event: &TranscriptEvent) -> Result<()> {
        let line = serde_json::to_string(event).context("serializing transcript event")?;
        self.file.write_all(line.as_bytes())?;
        self.file.write_all(b"\n")?;
        self.file.flush().context("flushing transcript")?;
        Ok(())
    }

    pub fn append_message(&mut self, message: MessageEvent) -> Result<()> {
        self.append(&TranscriptEvent::Message(message))
    }

    pub fn id(&self) -> &str {
        &self.meta.id
    }

    pub fn meta(&self) -> &SessionMeta {
        &self.meta
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

pub struct SessionTranscript {
    pub meta: SessionMeta,
    pub events: Vec<TranscriptEvent>,
}

impl SessionTranscript {
    pub fn load(path: &Path) -> Result<Self> {
        let file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
        let reader = BufReader::new(file);
        let mut events = Vec::new();
        let mut meta: Option<SessionMeta> = None;

        for line in reader.lines() {
            let Ok(line) = line else { break };
            if line.trim().is_empty() {
                continue;
            }
            let event: TranscriptEvent = match serde_json::from_str(&line) {
                Ok(ev) => ev,
                Err(e) => {
                    tracing::warn!("skipping unparseable line in {}: {e}", path.display());
                    continue;
                }
            };
            if let TranscriptEvent::Session(m) = &event {
                meta.get_or_insert_with(|| m.clone());
            }
            events.push(event);
        }

        let meta =
            meta.with_context(|| format!("transcript {} has no session header", path.display()))?;
        Ok(Self { meta, events })
    }

    pub fn to_chat_messages(&self) -> Vec<ChatMessage> {
        let mut out = Vec::new();
        if let Some(summary) = self.latest_summary() {
            out.push(ChatMessage {
                role: "assistant".into(),
                content: format!("Summary of earlier conversation:\n{summary}"),
            });
        }
        for event in &self.events {
            let TranscriptEvent::Message(m) = event else {
                continue;
            };
            if m.role != "user" && m.role != "assistant" {
                continue;
            }
            let Some(content) = m.content.as_deref() else {
                continue;
            };
            if content.trim().is_empty() {
                continue;
            }
            out.push(ChatMessage {
                role: m.role.clone(),
                content: content.to_string(),
            });
        }
        out
    }

    fn latest_summary(&self) -> Option<&str> {
        self.events.iter().rev().find_map(|e| match e {
            TranscriptEvent::Summary(s) => Some(s.content.as_str()),
            _ => None,
        })
    }

    pub fn messages(&self) -> impl Iterator<Item = &MessageEvent> {
        self.events.iter().filter_map(|e| match e {
            TranscriptEvent::Message(m) => Some(m),
            _ => None,
        })
    }

    pub fn user_turn_count(&self) -> usize {
        self.messages().filter(|m| m.role == "user").count()
    }

    pub fn first_user_text(&self) -> Option<&str> {
        self.messages()
            .find(|m| m.role == "user")
            .and_then(|m| m.content.as_deref())
    }
}
