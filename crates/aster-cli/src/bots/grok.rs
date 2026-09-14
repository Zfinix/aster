//! Reader for Grok Bot share JSON, the format `Add to Grok Bot` links hand out
//! and the one community converters treat as the interchange.

use anyhow::{Context, Result, bail};
use chrono::Utc;
use serde::Deserialize;

use super::ir::{BotIr, BotSkill, Identity, Note, Origin, Requirement, Routine, Trigger};

/// Grok event listeners. A routine naming one of these is not a cron, and
/// inventing a schedule for it would misreport when the bot runs.
const EVENT_KINDS: &[&str] = &[
    "slack",
    "github",
    "origin",
    "microsoftteams",
    "linear",
    "sentry",
    "pagerduty",
    "webhook",
    "group",
];

#[derive(Debug, Deserialize)]
struct Share {
    profile: Profile,
    #[serde(default)]
    memory: Vec<MemoryItem>,
    #[serde(default)]
    skills: Vec<SkillItem>,
    #[serde(default)]
    routines: Vec<RoutineItem>,
    #[serde(default)]
    plugins: Vec<PluginItem>,
}

#[derive(Debug, Deserialize)]
struct Profile {
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    title: String,
}

#[derive(Debug, Deserialize)]
struct MemoryItem {
    #[serde(default)]
    kind: String,
    #[serde(rename = "createdAt", default)]
    created_at: Option<String>,
    #[serde(default)]
    content: String,
}

#[derive(Debug, Deserialize)]
struct SkillItem {
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    content: String,
}

#[derive(Debug, Deserialize)]
struct RoutineItem {
    #[serde(default)]
    slug: String,
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    content: String,
}

#[derive(Debug, Deserialize)]
struct PluginItem {
    #[serde(rename = "pluginId")]
    plugin_id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    description: String,
}

/// Parse share JSON into the IR. `source` is recorded verbatim for `update`.
pub fn read(raw: &str, source: &str) -> Result<BotIr> {
    let share: Share = serde_json::from_str(raw).context("parsing Grok share JSON")?;
    if share.profile.name.trim().is_empty() {
        bail!("share JSON has no bot name");
    }

    let mut missing = Vec::new();
    let instructions = share.profile.description.trim().to_string();
    if instructions.is_empty() {
        missing.push("instructions (profile.description was empty)".to_string());
    }

    let mut skills = Vec::new();
    for item in share.skills {
        // A description where a body belongs is a summary, not a skill. Report
        // the gap rather than installing something that cannot be followed.
        if item.content.trim().is_empty() {
            missing.push(format!("skill body for {:?}", item.name));
            continue;
        }
        skills.push(BotSkill {
            name: item.name,
            description: item.description.trim().to_string(),
            body: item.content.trim().to_string(),
        });
    }

    let routines = share.routines.into_iter().map(routine).collect();

    let requirements = share
        .plugins
        .into_iter()
        .map(|p| Requirement {
            description: match p.name.trim().is_empty() {
                true => p.description.trim().to_string(),
                false => format!("{} ({})", p.name.trim(), p.description.trim())
                    .trim_end_matches(" ()")
                    .to_string(),
            },
            id: p.plugin_id,
        })
        .collect();

    let notes = share
        .memory
        .into_iter()
        .filter(|m| !m.content.trim().is_empty())
        .map(|m| Note {
            kind: m.kind,
            created_at: m.created_at,
            content: m.content.trim().to_string(),
        })
        .collect();

    Ok(BotIr {
        identity: Identity {
            name: share.profile.name.trim().to_string(),
            title: share.profile.title.trim().to_string(),
            description: instructions.clone(),
        },
        instructions,
        skills,
        routines,
        requirements,
        notes,
        origin: Origin {
            format: "grok-share-json".to_string(),
            source: source.to_string(),
            fetched_at: Utc::now().to_rfc3339(),
        },
        missing,
    })
}

fn routine(item: RoutineItem) -> Routine {
    let prose = format!("{}\n{}", item.description, item.content);
    let trigger = match extract_cron(&prose) {
        Some(expr) => Trigger::Cron { expr },
        None => match event_kind(&prose) {
            Some(name) => Trigger::Event { name },
            None => Trigger::Unknown,
        },
    };
    let task = match item.content.trim().is_empty() {
        true => item.description.trim().to_string(),
        false => item.content.trim().to_string(),
    };
    Routine {
        name: match item.slug.trim().is_empty() {
            true => item.name.clone(),
            false => item.slug.trim().to_string(),
        },
        description: item.description.trim().to_string(),
        task,
        trigger,
    }
}

/// A share JSON carries no schedule field: the cron sits in prose. Slide a
/// five-token window over the text and let `aster-cron` be the judge, which
/// also guarantees anything recovered is a cron Aster can actually install.
fn extract_cron(text: &str) -> Option<String> {
    let tokens: Vec<&str> = text.split_whitespace().collect();
    for window in tokens.windows(5) {
        let fields: Vec<&str> = window
            .iter()
            .map(|t| t.trim_matches(|c: char| c == '.' || c == ',' || c == ';' || c == '`'))
            .collect();
        if !fields.iter().all(is_cron_field) {
            continue;
        }
        let expr = fields.join(" ");
        if aster_cron::validate_cron(&expr).is_ok() {
            return Some(expr);
        }
    }
    None
}

fn is_cron_field(field: &&str) -> bool {
    !field.is_empty()
        && field
            .chars()
            .all(|c| c.is_ascii_digit() || matches!(c, '*' | ',' | '-' | '/'))
}

fn event_kind(text: &str) -> Option<String> {
    let lower = text.to_lowercase();
    EVENT_KINDS
        .iter()
        .find(|kind| lower.contains(*kind))
        .map(|kind| (*kind).to_string())
}
