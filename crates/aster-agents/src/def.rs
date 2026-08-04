use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

/// The definition file every agent directory must contain.
pub const AGENT_FILE: &str = "AGENT.md";

/// Tools an agent may call when its frontmatter names no allowlist: read-only.
pub const DEFAULT_TOOLS: &[&str] = &[
    "read_file",
    "list_files",
    "search_files",
    "find_files",
    "read_skill",
];

/// Spec limits on the frontmatter fields, matching `aster-skills`.
const MAX_NAME_LEN: usize = 64;
const MAX_DESCRIPTION_LEN: usize = 1024;

/// Where a definition came from. Built-ins embed the whole `AGENT.md` at
/// compile time; files are read on demand.
#[derive(Debug, Clone)]
pub enum AgentSource {
    BuiltIn(&'static str),
    File(PathBuf),
}

/// One agent definition: frontmatter metadata plus access to the prompt body.
#[derive(Debug, Clone)]
pub struct AgentDef {
    pub name: String,
    pub description: String,
    pub model: Option<String>,
    /// Tool allowlist; `None` means [`DEFAULT_TOOLS`].
    pub tools: Option<Vec<String>>,
    pub max_rounds: Option<usize>,
    /// Gate the agent's final reply through an adversarial verify pass.
    pub verify: bool,
    pub source: AgentSource,
}

impl AgentDef {
    /// The system prompt: everything below the frontmatter fence.
    pub fn load_body(&self) -> Result<String> {
        let raw = match &self.source {
            AgentSource::BuiltIn(raw) => (*raw).to_string(),
            AgentSource::File(path) => fs::read_to_string(path)
                .with_context(|| format!("reading agent {}", path.display()))?,
        };
        Ok(strip_frontmatter(&raw).trim().to_string())
    }

    /// True when the definition is compiled in rather than user-provided.
    pub fn is_builtin(&self) -> bool {
        matches!(self.source, AgentSource::BuiltIn(_))
    }
}

/// Frontmatter as written; unknown keys are ignored so future fields do not
/// break older binaries.
#[derive(Debug, Default, Deserialize)]
struct Frontmatter {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    tools: Option<Vec<String>>,
    #[serde(default)]
    max_rounds: Option<usize>,
    #[serde(default)]
    verify: bool,
}

/// Parse and validate one `AGENT.md`. `dir_name` is the fallback identity when
/// the frontmatter omits `name`.
pub(crate) fn parse_agent_md(raw: &str, dir_name: &str, source: AgentSource) -> Result<AgentDef> {
    let front = frontmatter(raw).context("missing `---` frontmatter fence")?;
    let front: Frontmatter = serde_yaml::from_str(front).context("parsing AGENT.md frontmatter")?;

    let name = front
        .name
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| dir_name.to_string());
    validate_name(&name)?;

    let description = front.description.unwrap_or_default().trim().to_string();
    if description.is_empty() {
        bail!("`description` is required and must be non-empty");
    }
    if description.len() > MAX_DESCRIPTION_LEN {
        bail!("`description` exceeds {MAX_DESCRIPTION_LEN} characters");
    }

    Ok(AgentDef {
        name,
        description,
        model: front.model,
        tools: front.tools,
        max_rounds: front.max_rounds,
        verify: front.verify,
        source,
    })
}

/// Split `raw` into `(frontmatter_yaml, body)` when the file opens with a
/// `---` fence.  The closing fence must be `\n---` at the start of a line,
/// followed by a newline or EOF.  The body strips exactly one leading newline
/// after the closing fence.  Returns `None` when the opening fence is missing.
fn split_frontmatter(raw: &str) -> Option<(&str, &str)> {
    let rest = raw
        .strip_prefix("---\n")
        .or_else(|| raw.strip_prefix("---\r\n"))?;
    let end = rest.find("\n---")?;
    let yaml = &rest[..end];
    let after = &rest[end + "\n---".len()..];
    let body = after
        .strip_prefix("\r\n")
        .or_else(|| after.strip_prefix('\n'))
        .unwrap_or(after);
    Some((yaml, body))
}

/// The YAML text between the opening `---` and the next `\n---` line.
/// Returns `None` when the file does not start with a frontmatter fence.
fn frontmatter(raw: &str) -> Option<&str> {
    split_frontmatter(raw).map(|(yaml, _)| yaml)
}

/// Everything after the frontmatter fence, or the whole input when there is
/// none.  Unlike the old code, this does NOT strip leading dashes/newlines
/// from the body — the shared `split_frontmatter` handles that cleanly.
fn strip_frontmatter(raw: &str) -> &str {
    match split_frontmatter(raw) {
        Some((_, body)) => body,
        None => raw,
    }
}

/// Structural checks on `name`: kebab-case identity, bounded length.
fn validate_name(name: &str) -> Result<()> {
    if name.len() > MAX_NAME_LEN {
        bail!("`name` exceeds {MAX_NAME_LEN} characters");
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        bail!("`name` must contain only lowercase letters, digits, and hyphens");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_full_frontmatter() {
        let raw = "---\nname: scout\ndescription: Finds things.\nmodel: openai/gpt-4o-mini\ntools: [read_file, search_files]\nmax_rounds: 4\nverify: true\n---\nBe terse.";
        let def = parse_agent_md(raw, "dir", AgentSource::BuiltIn(raw)).unwrap();
        assert_eq!(def.name, "scout");
        assert_eq!(def.model.as_deref(), Some("openai/gpt-4o-mini"));
        assert_eq!(
            def.tools.as_deref(),
            Some(&["read_file".to_string(), "search_files".to_string()][..])
        );
        assert_eq!(def.max_rounds, Some(4));
        assert!(def.verify);
        assert_eq!(def.load_body().unwrap(), "Be terse.");
    }

    #[test]
    fn name_falls_back_to_directory() {
        let raw = "---\ndescription: A helper.\n---\nbody";
        let def = parse_agent_md(raw, "helper", AgentSource::BuiltIn(raw)).unwrap();
        assert_eq!(def.name, "helper");
        assert!(!def.verify);
        assert!(def.tools.is_none());
    }

    #[test]
    fn rejects_missing_description() {
        let raw = "---\nname: broken\n---\nbody";
        assert!(parse_agent_md(raw, "broken", AgentSource::BuiltIn(raw)).is_err());
    }

    #[test]
    fn ignores_unknown_frontmatter_keys() {
        let raw = "---\ndescription: Fine.\ncolor: purple\n---\nbody";
        assert!(parse_agent_md(raw, "ok", AgentSource::BuiltIn(raw)).is_ok());
    }

    #[test]
    fn rejects_bad_name() {
        let raw = "---\nname: Not Kebab\ndescription: x.\n---\nbody";
        assert!(parse_agent_md(raw, "d", AgentSource::BuiltIn(raw)).is_err());
    }

    #[test]
    fn bullet_led_body_survives() {
        let raw = "---\ndescription: A helper.\n---\n- item one\n- item two\n";
        let def = parse_agent_md(raw, "helper", AgentSource::BuiltIn(raw)).unwrap();
        assert_eq!(def.load_body().unwrap(), "- item one\n- item two");
    }

    #[test]
    fn hrule_in_body_survives() {
        let raw = "---\ndescription: A helper.\n---\nSome text\n\n---\n\nMore text\n";
        let def = parse_agent_md(raw, "helper", AgentSource::BuiltIn(raw)).unwrap();
        assert_eq!(def.load_body().unwrap(), "Some text\n\n---\n\nMore text");
    }

    #[test]
    fn missing_fence_is_whole_body() {
        let raw = "No frontmatter here.\nJust a paragraph.\n";
        assert_eq!(strip_frontmatter(raw), raw);
    }

    #[test]
    fn crlf_fence_works() {
        let raw = "---\r\ndescription: A helper.\r\n---\r\nbody line\r\n";
        let def = parse_agent_md(raw, "helper", AgentSource::BuiltIn(raw)).unwrap();
        assert_eq!(def.load_body().unwrap(), "body line");
    }

    #[test]
    fn no_leading_newline_after_fence() {
        let raw = "---\ndescription: A helper.\n---";
        let def = parse_agent_md(raw, "helper", AgentSource::BuiltIn(raw)).unwrap();
        assert_eq!(def.load_body().unwrap(), "");
    }

    #[test]
    fn body_with_leading_dashes_not_eaten() {
        let raw = "---\ndescription: A helper.\n---\n- bullet\n- bullet\n";
        let def = parse_agent_md(raw, "helper", AgentSource::BuiltIn(raw)).unwrap();
        assert_eq!(def.load_body().unwrap(), "- bullet\n- bullet");
    }
}
