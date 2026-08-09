//! The `plugin.json` manifest (§5). The schema is closed: an unknown top-level
//! field or a non-object `extensions` is reported and ignored, and every other
//! violation rejects the plugin.

use std::collections::BTreeMap;

use anyhow::{Result, bail};
use serde_json::{Map, Value};

/// The one manifest schema this client implements.
pub const PLUGIN_SCHEMA: &str = "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json";

const MAX_NAME_LEN: usize = 64;

const FIELDS: &[&str] = &[
    "$schema",
    "name",
    "version",
    "description",
    "author",
    "homepage",
    "repository",
    "license",
    "keywords",
    "extensions",
];

#[derive(Debug, Clone, Default)]
pub struct Manifest {
    pub name: String,
    pub version: Option<String>,
    pub description: Option<String>,
    pub author: Option<Author>,
    pub homepage: Option<String>,
    pub repository: Option<String>,
    pub license: Option<String>,
    pub keywords: Vec<String>,
    /// Client-specific data keyed by reverse-domain namespace, kept unvalidated.
    pub extensions: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Default)]
pub struct Author {
    pub name: Option<String>,
    pub email: Option<String>,
    pub url: Option<String>,
}

/// Parse one manifest, appending non-fatal problems to `warnings`.
pub fn parse(text: &str, warnings: &mut Vec<String>) -> Result<Manifest> {
    let value: Value = serde_json::from_str(text)?;
    let Value::Object(object) = value else {
        bail!("plugin.json must contain a top-level object");
    };

    for key in object.keys() {
        if !FIELDS.contains(&key.as_str()) {
            warnings.push(format!("ignoring unknown plugin.json field `{key}`"));
        }
    }

    match object.get("$schema").and_then(Value::as_str) {
        Some(PLUGIN_SCHEMA) => {}
        Some(other) => bail!("unsupported Agent Plugins version: $schema is {other:?}"),
        None => bail!("`$schema` is required and must be {PLUGIN_SCHEMA:?}"),
    }

    let name = match object.get("name") {
        Some(Value::String(name)) => name.clone(),
        Some(_) => bail!("`name` must be a string"),
        None => bail!("`name` is required"),
    };
    validate_name(&name)?;

    Ok(Manifest {
        name,
        version: string(&object, "version")?,
        description: string(&object, "description")?,
        author: author(&object)?,
        homepage: string(&object, "homepage")?,
        repository: string(&object, "repository")?,
        license: string(&object, "license")?,
        keywords: keywords(&object)?,
        extensions: extensions(&object, warnings),
    })
}

fn string(object: &Map<String, Value>, key: &str) -> Result<Option<String>> {
    match object.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        Some(_) => bail!("`{key}` must be a string"),
    }
}

fn keywords(object: &Map<String, Value>) -> Result<Vec<String>> {
    let Some(value) = object.get("keywords") else {
        return Ok(Vec::new());
    };
    let Some(items) = value.as_array() else {
        bail!("`keywords` must be an array of strings");
    };
    items
        .iter()
        .map(|item| match item.as_str() {
            Some(word) => Ok(word.to_string()),
            None => bail!("`keywords` must be an array of strings"),
        })
        .collect()
}

fn author(object: &Map<String, Value>) -> Result<Option<Author>> {
    let Some(value) = object.get("author") else {
        return Ok(None);
    };
    let Some(fields) = value.as_object() else {
        bail!("`author` must be an object");
    };
    for key in fields.keys() {
        if !["name", "email", "url"].contains(&key.as_str()) {
            bail!("`author.{key}` is not a permitted field");
        }
    }
    Ok(Some(Author {
        name: string(fields, "name")?,
        email: string(fields, "email")?,
        url: string(fields, "url")?,
    }))
}

/// A non-object `extensions` is reported and dropped rather than fatal, and
/// namespaces this client does not implement are carried through unread.
fn extensions(object: &Map<String, Value>, warnings: &mut Vec<String>) -> BTreeMap<String, Value> {
    let Some(value) = object.get("extensions") else {
        return BTreeMap::new();
    };
    let Some(namespaces) = value.as_object() else {
        warnings.push("ignoring `extensions`: it is not an object".to_string());
        return BTreeMap::new();
    };
    namespaces
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

/// Lowercase alphanumerics, hyphens, and periods, bounded, alphanumeric at both
/// ends, with no doubled separator (§5.5).
fn validate_name(name: &str) -> Result<()> {
    let count = name.chars().count();
    if count == 0 || count > MAX_NAME_LEN {
        bail!("`name` must be 1 to {MAX_NAME_LEN} characters");
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '.')
    {
        bail!("`name` must contain only lowercase letters, digits, hyphens, and periods");
    }
    let ends_alnum = |c: Option<char>| c.is_some_and(|c| c.is_ascii_alphanumeric());
    if !ends_alnum(name.chars().next()) || !ends_alnum(name.chars().next_back()) {
        bail!("`name` must start and end with a letter or digit");
    }
    if name.contains("--") || name.contains("..") {
        bail!("`name` must not contain consecutive hyphens or periods");
    }
    Ok(())
}
