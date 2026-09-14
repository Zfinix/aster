//! The normalized form every reader produces and the writer consumes. It stays
//! deliberately close to the Grok share JSON so the mapping is auditable.

use serde::{Deserialize, Serialize};

/// What a routine fires on. Only `Cron` becomes a schedule: a fabricated cron
/// for an event listener would be a lie about when the bot runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Trigger {
    Cron { expr: String },
    Event { name: String },
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Identity {
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BotSkill {
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Routine {
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    pub task: String,
    pub trigger: Trigger,
}

/// One thing the bot assumes exists. Every connector starts here, because a
/// share JSON names a plugin without ever configuring one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Requirement {
    pub id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
}

/// The publisher's notes. Quarantined here and never merged into the user's
/// memory: these are their preferences and their machine's layout, not facts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Note {
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Origin {
    /// Which reader produced this, for the report and for `update`.
    pub format: String,
    pub source: String,
    pub fetched_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BotIr {
    pub identity: Identity,
    pub instructions: String,
    #[serde(default)]
    pub skills: Vec<BotSkill>,
    #[serde(default)]
    pub routines: Vec<Routine>,
    #[serde(default)]
    pub requirements: Vec<Requirement>,
    #[serde(default)]
    pub notes: Vec<Note>,
    pub origin: Origin,
    /// Fields a reader could not recover, reported rather than invented.
    #[serde(default)]
    pub missing: Vec<String>,
}

impl BotIr {
    pub fn cron_routines(&self) -> impl Iterator<Item = (&Routine, &str)> {
        self.routines.iter().filter_map(|r| match &r.trigger {
            Trigger::Cron { expr } => Some((r, expr.as_str())),
            _ => None,
        })
    }

    pub fn non_cron_routines(&self) -> impl Iterator<Item = &Routine> {
        self.routines
            .iter()
            .filter(|r| !matches!(r.trigger, Trigger::Cron { .. }))
    }
}

/// Lowercase, hyphenated, and safe as a directory name: the same shape agent,
/// skill, and schedule names are validated against.
pub fn slug(raw: &str) -> String {
    let mut out = String::new();
    for ch in raw.trim().chars() {
        match ch {
            c if c.is_ascii_alphanumeric() => out.push(c.to_ascii_lowercase()),
            _ if out.ends_with('-') => {}
            _ if out.is_empty() => {}
            _ => out.push('-'),
        }
    }
    let out = out.trim_matches('-').to_string();
    out.chars().take(64).collect()
}
