//! Turning a `BotIr` into an installed package: an `AGENT.md` the registry
//! discovers, the bot's own `skills/`, a `bot.json` record, and the untouched
//! original under `source/`.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use super::ir::{BotIr, slug};

pub const RECORD_FILE: &str = "bot.json";
pub const RECORD_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
pub struct BotRecord {
    pub version: u32,
    pub installed_at: String,
    /// Tools the agent was installed with, so `show` can report the narrowing.
    #[serde(default)]
    pub tools: Vec<String>,
    pub bot: BotIr,
}

impl BotRecord {
    pub fn load(dir: &Path) -> Result<Self> {
        let path = dir.join(RECORD_FILE);
        let raw =
            fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        serde_json::from_str(&raw).with_context(|| format!("parsing {}", path.display()))
    }
}

pub struct Written {
    pub dir: PathBuf,
    pub name: String,
    pub skills: usize,
}

/// Install `ir` under `root`. `raw` is the original bytes, kept verbatim so a
/// later `update` can diff against what the publisher actually served.
pub fn install(
    root: &Path,
    ir: &BotIr,
    raw: &str,
    tools: &[String],
    force: bool,
) -> Result<Written> {
    let name = slug(&ir.identity.name);
    if name.is_empty() {
        bail!("{:?} does not reduce to a usable name", ir.identity.name);
    }
    let dir = root.join(&name);
    if dir.exists() && !force {
        bail!(
            "{name} is already installed at {}. Pass --force to overwrite, or `aster bots update {name}`",
            dir.display()
        );
    }
    if dir.exists() {
        fs::remove_dir_all(&dir).with_context(|| format!("replacing {}", dir.display()))?;
    }
    fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;

    fs::write(
        dir.join(aster_agents::AGENT_FILE),
        agent_md(ir, &name, tools),
    )
    .with_context(|| format!("writing {}", dir.join("AGENT.md").display()))?;

    let mut skills = 0;
    for skill in &ir.skills {
        let skill_name = slug(&skill.name);
        if skill_name.is_empty() {
            continue;
        }
        let skill_dir = dir.join("skills").join(&skill_name);
        fs::create_dir_all(&skill_dir)
            .with_context(|| format!("creating {}", skill_dir.display()))?;
        fs::write(
            skill_dir.join(aster_skills::SKILL_FILE),
            skill_md(&skill_name, &skill.description, &skill.body),
        )
        .with_context(|| format!("writing {}", skill_dir.display()))?;
        skills += 1;
    }

    let source_dir = dir.join("source");
    fs::create_dir_all(&source_dir)
        .with_context(|| format!("creating {}", source_dir.display()))?;
    let source_name = match ir.origin.format.as_str() {
        "grok-share-json" => "share.json",
        _ => "source.md",
    };
    fs::write(source_dir.join(source_name), raw)
        .with_context(|| format!("writing {}", source_dir.join(source_name).display()))?;

    let record = BotRecord {
        version: RECORD_VERSION,
        installed_at: chrono::Utc::now().to_rfc3339(),
        tools: tools.to_vec(),
        bot: ir.clone(),
    };
    fs::write(
        dir.join(RECORD_FILE),
        serde_json::to_string_pretty(&record)?,
    )
    .with_context(|| format!("writing {}", dir.join(RECORD_FILE).display()))?;

    Ok(Written { dir, name, skills })
}

/// The description is required and capped by the agent parser, so a bot with a
/// thin profile still produces a definition the registry accepts.
fn agent_md(ir: &BotIr, name: &str, tools: &[String]) -> String {
    let description = describe(ir);
    let tools_line = tools
        .iter()
        .map(|t| t.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let body = match ir.instructions.trim().is_empty() {
        true => format!(
            "You are {}, an imported bot. Its published instructions did not come \
             across in the import, so work from the task you are given and say \
             plainly when you lack the instructions to do it.",
            ir.identity.name
        ),
        false => ir.instructions.trim().to_string(),
    };
    format!(
        "---\nname: {name}\ndescription: {}\ncategory: bots\ntools: [{tools_line}]\nmax_rounds: 12\n---\n{body}\n",
        yaml_scalar(&description)
    )
}

fn skill_md(name: &str, description: &str, body: &str) -> String {
    let description = match description.trim().is_empty() {
        true => format!("Imported skill: {name}."),
        false => description.trim().to_string(),
    };
    format!(
        "---\nname: {name}\ndescription: {}\n---\n{}\n",
        yaml_scalar(&description),
        body.trim()
    )
}

fn describe(ir: &BotIr) -> String {
    let base = [ir.identity.title.as_str(), ir.identity.description.as_str()]
        .into_iter()
        .find(|s| !s.trim().is_empty())
        .unwrap_or("imported bot");
    let one_line: String = base.split_whitespace().collect::<Vec<_>>().join(" ");
    let clipped: String = one_line.chars().take(900).collect();
    format!("{} (imported bot: {})", clipped, ir.identity.name)
}

fn yaml_scalar(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}
