//! Path containment and placeholder expansion, the two rules the spec applies
//! to every package path and runtime value (§4.1, §9.2).

use std::path::{Component, Path, PathBuf};

use anyhow::{Result, bail};

pub const PLUGIN_ROOT_VAR: &str = "PLUGIN_ROOT";
pub const PLUGIN_DATA_VAR: &str = "PLUGIN_DATA";

const ROOT_PLACEHOLDER: &str = "${PLUGIN_ROOT}";
const DATA_PLACEHOLDER: &str = "${PLUGIN_DATA}";

/// Resolve `./`-prefixed package path `value` against `root`, refusing anything
/// that escapes it. Other forms are not plugin-relative paths.
pub fn plugin_relative(root: &Path, value: &str) -> Result<PathBuf> {
    let Some(rel) = value.strip_prefix("./") else {
        bail!("{value:?} must be a plugin-relative path beginning with `./`");
    };
    let resolved = root.join(rel);
    if !contained(root, &resolved) {
        bail!("{value:?} resolves outside the plugin root");
    }
    Ok(resolved)
}

/// True when `path` stays inside `root`. The lexical check runs first so a
/// target that does not exist yet is still judged; an existing one must also
/// survive symlink resolution.
pub fn contained(root: &Path, path: &Path) -> bool {
    if !normalize(path).starts_with(normalize(root)) {
        return false;
    }
    match (std::fs::canonicalize(path), std::fs::canonicalize(root)) {
        (Ok(path), Ok(root)) => path.starts_with(root),
        _ => true,
    }
}

fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Replace every `${PLUGIN_ROOT}` and `${PLUGIN_DATA}` in one pass. Replacement
/// text is never rescanned, and unrecognized `${…}` stays literal.
pub fn expand(text: &str, plugin_root: &str, plugin_data: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("${PLUGIN_") {
        out.push_str(&rest[..at]);
        let tail = &rest[at..];
        if let Some(after) = tail.strip_prefix(ROOT_PLACEHOLDER) {
            out.push_str(plugin_root);
            rest = after;
        } else if let Some(after) = tail.strip_prefix(DATA_PLACEHOLDER) {
            out.push_str(plugin_data);
            rest = after;
        } else {
            out.push_str("${PLUGIN_");
            rest = &tail["${PLUGIN_".len()..];
        }
    }
    out.push_str(rest);
    out
}

/// Which directory a `cwd` value is rooted in, once expanded.
pub enum Anchor {
    Root,
    Data,
}

/// Classify a `cwd` value by its required form (§7.2.1). Returns the anchor the
/// expanded path must stay inside.
pub fn cwd_anchor(value: &str) -> Result<Anchor> {
    if value.starts_with("./") {
        return Ok(Anchor::Root);
    }
    if value == ROOT_PLACEHOLDER || value.starts_with(&format!("{ROOT_PLACEHOLDER}/")) {
        return Ok(Anchor::Root);
    }
    if value == DATA_PLACEHOLDER || value.starts_with(&format!("{DATA_PLACEHOLDER}/")) {
        return Ok(Anchor::Data);
    }
    bail!("`cwd` must be `./…`, `{ROOT_PLACEHOLDER}…`, or `{DATA_PLACEHOLDER}…`")
}
