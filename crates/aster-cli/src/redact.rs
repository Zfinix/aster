//! Redacts known secret values from text before Aster surfaces it. The values come
//! from the environment and the `.env` files Aster loads; tool output passes
//! through once, so stream, transcript, and model context all stay clean.

use std::path::PathBuf;
use std::sync::OnceLock;

const MIN_SECRET_LEN: usize = 8;

const REDACTED: &str = "[redacted]";

/// Secret-named values discovered once, longest first so a value that is a
/// prefix of another cannot be partly replaced first.
#[derive(Debug, Default)]
pub struct Redactor {
    secrets: Vec<String>,
}

impl Redactor {
    /// Collect secret-shaped values from the process environment and the `.env`
    /// files Aster itself loads.
    pub fn discover() -> Self {
        let mut secrets = Vec::new();
        for (name, value) in std::env::vars() {
            if is_secret_name(&name) {
                secrets.push(value);
            }
        }
        for path in env_files() {
            if let Ok(text) = std::fs::read_to_string(&path) {
                for (name, value) in text.lines().filter_map(parse_env_line) {
                    if is_secret_name(&name) {
                        secrets.push(value);
                    }
                }
            }
        }
        Self::new(secrets)
    }

    fn new(mut secrets: Vec<String>) -> Self {
        secrets.retain(|s| s.len() >= MIN_SECRET_LEN);
        secrets.sort_by_key(|s| std::cmp::Reverse(s.len()));
        secrets.dedup();
        Self { secrets }
    }

    pub fn redact(&self, text: &str) -> String {
        let mut out = text.to_string();
        for secret in &self.secrets {
            if out.contains(secret.as_str()) {
                out = out.replace(secret.as_str(), REDACTED);
            }
        }
        out
    }
}

fn is_secret_name(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    upper.ends_with("_API_KEY")
        || upper.ends_with("_KEY")
        || upper.ends_with("_TOKEN")
        || upper.ends_with("_SECRET")
        || upper.contains("PASSWORD")
        || upper.contains("PASSWD")
        || upper.contains("PRIVATE_KEY")
}

fn env_files() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(global) = crate::persist::global_env_path() {
        paths.push(global);
    }
    if let Ok(cwd) = std::env::current_dir() {
        paths.push(cwd.join(".env"));
    }
    paths
}

fn parse_env_line(line: &str) -> Option<(String, String)> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let (name, value) = line.split_once('=')?;
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    let value = value.trim();
    let value = value
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
        .unwrap_or(value);
    Some((name.to_string(), value.to_string()))
}

fn redactor() -> &'static Redactor {
    static INSTANCE: OnceLock<Redactor> = OnceLock::new();
    INSTANCE.get_or_init(Redactor::discover)
}

/// Redact known secrets from `text`. Cheap enough to run on every tool result.
pub fn redact(text: &str) -> String {
    redactor().redact(text)
}

#[cfg(test)]
#[path = "tests/redact_test.rs"]
mod tests;
