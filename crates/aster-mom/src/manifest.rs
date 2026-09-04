//! `mom.yaml` parsing, validation warnings, and discovery (spec 5-6).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

pub const DEFAULT_HOLD: u32 = 3;
pub const DEFAULT_STUCK: u32 = 3;
pub const DEFAULT_CHAT_FULL: f64 = 70.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Power {
    Low,
    #[default]
    Medium,
    Max,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MemoryBand {
    #[default]
    Normal,
    Large,
    Huge,
    Vast,
}

impl MemoryBand {
    pub fn floor_tokens(self) -> u64 {
        match self {
            MemoryBand::Normal => 32_000,
            MemoryBand::Large => 128_000,
            MemoryBand::Huge => 200_000,
            MemoryBand::Vast => 1_000_000,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Thinking {
    #[default]
    None,
    Some,
    Deep,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ModelEntry {
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub power: Power,
    #[serde(default)]
    pub memory: MemoryBand,
    #[serde(default)]
    pub thinking: Thinking,
    #[serde(default)]
    pub sees_images: bool,
    #[serde(default)]
    pub uses_tools: bool,
    #[serde(default)]
    pub prefer: Vec<String>,
    #[serde(default)]
    pub settings: BTreeMap<String, serde_yaml::Value>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_yaml::Value>,
}

/// One switch condition (spec 6.3). Unknown keywords stay inert.
#[derive(Debug, Clone, PartialEq)]
pub enum Condition {
    Planning(Option<String>),
    Stuck(u32),
    Looping,
    ChatFull(f64),
    SpentOver(f64),
    TokensOver(u64),
    TurnOver(u64),
    ModelDown,
    Extension(String, serde_yaml::Value),
    Inert(String),
}

impl Condition {
    pub fn is_emergency(&self) -> bool {
        matches!(self, Condition::Looping | Condition::ModelDown)
    }

    pub fn is_spending(&self) -> bool {
        matches!(
            self,
            Condition::SpentOver(_) | Condition::TokensOver(_) | Condition::TurnOver(_)
        )
    }
}

#[derive(Debug, Clone)]
pub struct Rule {
    pub when: Vec<Condition>,
    pub use_entry: String,
    pub hold: u32,
}

impl Rule {
    pub fn is_inert(&self) -> bool {
        self.when.iter().all(|c| matches!(c, Condition::Inert(_)))
    }

    pub fn is_emergency(&self) -> bool {
        self.when.iter().any(Condition::is_emergency)
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Router {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub power: Power,
    #[serde(default)]
    pub prefer: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Manifest {
    pub version: String,
    pub name: Option<String>,
    pub models: BTreeMap<String, ModelEntry>,
    pub start_with: String,
    pub switch: Vec<Rule>,
    pub router: Router,
    pub warnings: Vec<String>,
    pub path: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct RawManifest {
    mom: String,
    #[serde(default)]
    name: Option<String>,
    models: BTreeMap<String, ModelEntry>,
    start_with: String,
    #[serde(default)]
    switch: Vec<RawRule>,
    #[serde(default)]
    router: Router,
    #[serde(flatten)]
    extra: BTreeMap<String, serde_yaml::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct RawRule {
    when: serde_yaml::Value,
    #[serde(rename = "use")]
    use_entry: String,
    #[serde(default)]
    hold: Option<u32>,
}

const KNOWN_ENTRY_KEYS: &[&str] = &[
    "description",
    "power",
    "memory",
    "thinking",
    "sees-images",
    "uses-tools",
    "prefer",
    "settings",
];

pub fn parse(text: &str) -> Result<Manifest> {
    let raw: RawManifest = serde_yaml::from_str(text).context("mom.yaml is not valid YAML")?;

    let major = raw.mom.split('.').next().unwrap_or_default();
    if major != "1" && major != "0" {
        bail!(
            "mom.yaml declares format version '{}'; this tool supports 1.x (and the 0.x draft)",
            raw.mom
        );
    }

    let mut warnings = Vec::new();
    if raw.models.is_empty() {
        bail!("mom.yaml declares no model entries");
    }
    if !raw.models.contains_key(&raw.start_with) {
        bail!(
            "start-with names '{}', which is not a key of models",
            raw.start_with
        );
    }
    for key in raw.extra.keys().filter(|k| !k.starts_with("x-")) {
        warnings.push(format!("unknown top-level key '{key}' ignored"));
    }
    for (name, entry) in &raw.models {
        for key in entry.extra.keys().filter(|k| !k.starts_with("x-")) {
            if !KNOWN_ENTRY_KEYS.contains(&key.as_str()) {
                warnings.push(format!(
                    "unknown key '{key}' on model entry '{name}' ignored"
                ));
            }
        }
    }

    let mut switch = Vec::new();
    for (idx, rule) in raw.switch.into_iter().enumerate() {
        if !raw.models.contains_key(&rule.use_entry) {
            bail!(
                "switch rule {} uses '{}', which is not a key of models",
                idx + 1,
                rule.use_entry
            );
        }
        let when = compile_when(&rule.when, idx, &mut warnings);
        switch.push(Rule {
            when,
            use_entry: rule.use_entry,
            hold: rule.hold.unwrap_or(DEFAULT_HOLD),
        });
    }

    Ok(Manifest {
        version: raw.mom,
        name: raw.name,
        models: raw.models,
        start_with: raw.start_with,
        switch,
        router: raw.router,
        warnings,
        path: None,
    })
}

fn compile_when(
    value: &serde_yaml::Value,
    rule: usize,
    warnings: &mut Vec<String>,
) -> Vec<Condition> {
    match value {
        serde_yaml::Value::Sequence(items) => items
            .iter()
            .map(|item| compile_condition(item, rule, warnings))
            .collect(),
        other => vec![compile_condition(other, rule, warnings)],
    }
}

fn compile_condition(
    value: &serde_yaml::Value,
    rule: usize,
    warnings: &mut Vec<String>,
) -> Condition {
    match value {
        serde_yaml::Value::String(word) => bare_condition(word, rule, warnings),
        serde_yaml::Value::Mapping(map) if map.len() == 1 => {
            let (key, param) = map
                .iter()
                .next()
                .map(|(k, v)| (k.as_str().unwrap_or_default().to_string(), v.clone()))
                .unwrap_or_default();
            keyed_condition(&key, param, rule, warnings)
        }
        other => {
            warnings.push(format!(
                "switch rule {}: condition {:?} is not a keyword or one-key map; inert",
                rule + 1,
                other
            ));
            Condition::Inert(format!("{other:?}"))
        }
    }
}

fn bare_condition(word: &str, rule: usize, warnings: &mut Vec<String>) -> Condition {
    match word {
        "planning" => Condition::Planning(None),
        "stuck" => Condition::Stuck(DEFAULT_STUCK),
        "looping" => Condition::Looping,
        "chat-full" => Condition::ChatFull(DEFAULT_CHAT_FULL),
        "model-down" => Condition::ModelDown,
        "spent-over" | "tokens-over" | "turn-over" => {
            warnings.push(format!(
                "switch rule {}: '{word}' requires a parameter; inert",
                rule + 1
            ));
            Condition::Inert(word.to_string())
        }
        other if other.starts_with("x-") => {
            Condition::Extension(other.to_string(), serde_yaml::Value::Null)
        }
        other => {
            warnings.push(format!(
                "switch rule {}: unknown condition '{other}'; inert",
                rule + 1
            ));
            Condition::Inert(other.to_string())
        }
    }
}

fn keyed_condition(
    key: &str,
    param: serde_yaml::Value,
    rule: usize,
    warnings: &mut Vec<String>,
) -> Condition {
    let bad = |warnings: &mut Vec<String>| {
        warnings.push(format!(
            "switch rule {}: bad parameter for '{key}'; inert",
            rule + 1
        ));
        Condition::Inert(key.to_string())
    };
    match key {
        "planning" => match param.as_str() {
            Some(mode) => Condition::Planning(Some(mode.to_string())),
            None => bad(warnings),
        },
        "stuck" => match param.as_u64() {
            Some(n) if n >= 1 => Condition::Stuck(n as u32),
            _ => bad(warnings),
        },
        "chat-full" => match param.as_f64() {
            Some(p) if p > 0.0 && p <= 100.0 => Condition::ChatFull(p),
            _ => bad(warnings),
        },
        "spent-over" => match param.as_f64() {
            Some(d) if d > 0.0 => Condition::SpentOver(d),
            _ => bad(warnings),
        },
        "tokens-over" => match param.as_u64() {
            Some(t) if t >= 1 => Condition::TokensOver(t),
            _ => bad(warnings),
        },
        "turn-over" => match param.as_u64() {
            Some(t) if t >= 1 => Condition::TurnOver(t),
            _ => bad(warnings),
        },
        "looping" | "model-down" => {
            warnings.push(format!(
                "switch rule {}: '{key}' takes no parameter; parameter ignored",
                rule + 1
            ));
            if key == "looping" {
                Condition::Looping
            } else {
                Condition::ModelDown
            }
        }
        other if other.starts_with("x-") => Condition::Extension(other.to_string(), param),
        other => {
            warnings.push(format!(
                "switch rule {}: unknown condition '{other}'; inert",
                rule + 1
            ));
            Condition::Inert(other.to_string())
        }
    }
}

/// Finds the active manifest for a project (spec 5): `mom.yaml`, then
/// `.agents/mom.yaml`, then the personal `~/.aster/mom.yaml`. No walk-up,
/// no merging; the first file found is the whole policy.
pub fn discover(repo_root: Option<&Path>, home: Option<&Path>) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(root) = repo_root {
        candidates.push(root.join("mom.yaml"));
        candidates.push(root.join(".agents/mom.yaml"));
    }
    if let Some(home) = home {
        candidates.push(home.join("mom.yaml"));
    }
    candidates.into_iter().find(|p| p.is_file())
}

pub fn load(path: &Path) -> Result<Manifest> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let mut manifest = parse(&text)?;
    manifest.path = Some(path.to_path_buf());
    Ok(manifest)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ABSTRACT_FILE: &str = r#"
mom: "0.1"
models:
  everyday:
    power: medium
  thinker:
    power: max
    thinking: deep
start-with: everyday
switch:
  - when: planning
    use: thinker
  - when: stuck
    use: thinker
  - when: { spent-over: 5 }
    use: everyday
"#;

    #[test]
    fn parse_abstract_file_succeeds() {
        let m = parse(ABSTRACT_FILE).unwrap();
        assert_eq!(m.start_with, "everyday");
        assert_eq!(m.switch.len(), 3);
        assert!(m.warnings.is_empty());
        assert_eq!(m.switch[0].when, vec![Condition::Planning(None)]);
        assert_eq!(m.switch[1].when, vec![Condition::Stuck(3)]);
        assert_eq!(m.switch[2].when, vec![Condition::SpentOver(5.0)]);
        assert_eq!(m.switch[2].hold, DEFAULT_HOLD);
    }

    #[test]
    fn parse_condition_list_and_params() {
        let m = parse(
            r#"
mom: "0.1"
models:
  a: {}
start-with: a
switch:
  - when: [stuck, looping, model-down]
    use: a
  - when: { chat-full: 85 }
    use: a
    hold: 5
"#,
        )
        .unwrap();
        assert_eq!(m.switch[0].when.len(), 3);
        assert!(m.switch[0].is_emergency());
        assert_eq!(m.switch[1].when, vec![Condition::ChatFull(85.0)]);
        assert_eq!(m.switch[1].hold, 5);
    }

    #[test]
    fn parse_bare_spending_condition_is_inert_with_warning() {
        let m = parse(
            r#"
mom: "0.1"
models:
  a: {}
start-with: a
switch:
  - when: spent-over
    use: a
"#,
        )
        .unwrap();
        assert!(m.switch[0].is_inert());
        assert_eq!(m.warnings.len(), 1);
    }

    #[test]
    fn parse_unknown_condition_is_inert_but_list_survives() {
        let m = parse(
            r#"
mom: "0.1"
models:
  a: {}
start-with: a
switch:
  - when: [frobnicating, stuck]
    use: a
"#,
        )
        .unwrap();
        assert!(!m.switch[0].is_inert());
        assert!(
            m.switch[0]
                .when
                .iter()
                .any(|c| matches!(c, Condition::Inert(_)))
        );
    }

    #[test]
    fn parse_rejects_unknown_start_with() {
        assert!(parse(r#"{ mom: "0.1", models: { a: {} }, start-with: b }"#).is_err());
    }

    #[test]
    fn parse_accepts_one_point_oh_and_draft_zero() {
        assert!(parse(r#"{ mom: "1.0", models: { a: {} }, start-with: a }"#).is_ok());
        assert!(parse(r#"{ mom: "0.1", models: { a: {} }, start-with: a }"#).is_ok());
    }

    #[test]
    fn parse_rejects_unsupported_major() {
        assert!(parse(r#"{ mom: "2.0", models: { a: {} }, start-with: a }"#).is_err());
    }

    #[test]
    fn parse_x_condition_is_extension_not_inert() {
        let m = parse(
            r#"
mom: "0.1"
models:
  a: {}
start-with: a
switch:
  - when: { x-mytool-stage: verify }
    use: a
"#,
        )
        .unwrap();
        assert!(
            matches!(&m.switch[0].when[0], Condition::Extension(k, _) if k == "x-mytool-stage")
        );
        assert!(m.warnings.is_empty());
    }
}
