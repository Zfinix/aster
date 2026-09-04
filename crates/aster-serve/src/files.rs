//! Files, for the composer: the @-mention search, and what to do with something
//! dropped or pasted into the page.

use std::path::{Path, PathBuf};

use ignore::WalkBuilder;

const LIMIT: usize = 50;

const SCAN: usize = 2000;

const MAX_PREVIEW_LINES: usize = 200;
const MAX_PREVIEW_CHARS: usize = 32_000;

/// Repo files matching `query`, plus the folders on the way to them, shallowest
/// first. Mirrors what the extension gets from the editor's own file index.
pub fn search(root: &Path, query: &str) -> Vec<String> {
    let query = query.to_lowercase();
    let mut files: Vec<String> = Vec::new();
    let mut folders: Vec<String> = Vec::new();

    for entry in walk(root).flatten() {
        let Ok(relative) = entry.path().strip_prefix(root) else {
            continue;
        };
        let path = relative.to_string_lossy().replace('\\', "/");
        if path.is_empty() || path == ".git" || path.starts_with(".git/") {
            continue;
        }
        if !path.to_lowercase().contains(&query) {
            continue;
        }
        let is_dir = entry.file_type().is_some_and(|kind| kind.is_dir());
        match is_dir {
            true => folders.push(format!("{path}/")),
            false => files.push(path),
        }
        if files.len() + folders.len() >= SCAN {
            break;
        }
    }

    let mut found: Vec<String> = folders.into_iter().chain(files).collect();
    found.sort_by(|a, b| shallowest_first(a, b));
    found.truncate(LIMIT);
    found
}

/// A path a drag dropped on the page, as `file://…` or as itself. Relative to
/// the repo when it is inside it, since that is what the agent reads and what
/// the composer has room to show.
pub fn mention(root: &Path, uri: &str) -> Option<String> {
    let path = match uri.strip_prefix("file://") {
        Some(rest) => PathBuf::from(percent_decode(rest.split('?').next().unwrap_or(rest))),
        None if uri.starts_with('/') => PathBuf::from(uri),
        None => return None,
    };
    if !path.exists() {
        return None;
    }
    let relative = path.strip_prefix(root).unwrap_or(&path);
    Some(relative.to_string_lossy().replace('\\', "/"))
}

fn percent_decode(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        let hex: String = chars.clone().take(2).collect();
        match u8::from_str_radix(&hex, 16) {
            Ok(byte) => {
                out.push(byte as char);
                chars.next();
                chars.next();
            }
            Err(_) => out.push(c),
        }
    }
    out
}

/// A file pasted or dropped into the page, which arrives as bytes and a name. One
/// already in the repo is matched back to it by name and size, so the agent reads
/// the real file; anything else is written where the agent can still reach it.
pub fn stage(root: &Path, name: &str, size: u64, data: &[u8]) -> Result<String, String> {
    if let Some(existing) = find(root, name, size) {
        return Ok(existing);
    }
    let dir = crate::paths::home()
        .ok_or("no home directory on this platform")?
        .join("pasted");
    std::fs::create_dir_all(&dir).map_err(|e| format!("could not stage {name}: {e}"))?;
    // Stamped, so a second screenshot never overwrites the one already mentioned.
    let stamp = ulid::Ulid::new().to_string();
    let target = dir.join(format!("{stamp}-{}", sanitize(name)));
    std::fs::write(&target, data).map_err(|e| format!("could not stage {name}: {e}"))?;
    Ok(target.display().to_string())
}

fn find(root: &Path, name: &str, size: u64) -> Option<String> {
    let mut found: Option<PathBuf> = None;
    for entry in walk(root).flatten() {
        if entry.file_name() != name || !entry.file_type().is_some_and(|kind| kind.is_file()) {
            continue;
        }
        if entry.metadata().map(|meta| meta.len()).ok() != Some(size) {
            continue;
        }
        if found.is_some() {
            return None;
        }
        found = Some(entry.path().to_path_buf());
    }
    let found = found?;
    let relative = found.strip_prefix(root).unwrap_or(&found);
    Some(relative.to_string_lossy().replace('\\', "/"))
}

fn walk(root: &Path) -> ignore::Walk {
    WalkBuilder::new(root)
        .hidden(false)
        .require_git(false)
        .build()
}

fn sanitize(name: &str) -> String {
    name.rsplit(['/', '\\'])
        .next()
        .filter(|name| !name.is_empty() && *name != ".." && *name != ".")
        .unwrap_or("pasted")
        .to_string()
}

fn shallowest_first(a: &str, b: &str) -> std::cmp::Ordering {
    let (left, right) = (a.trim_end_matches('/'), b.trim_end_matches('/'));
    let depth = left.matches('/').count().cmp(&right.matches('/').count());
    depth.then_with(|| left.cmp(right))
}

/// The head of a file, for the preview card in the page.
#[derive(Debug, PartialEq, serde::Serialize)]
pub struct PreviewFile {
    pub path: String,
    pub lang: Option<String>,
    pub content: String,
    pub truncated: bool,
}

/// A bounded peek at a repo file: the head of it, never the whole thing, and
/// never anything outside the repo.
pub fn preview(root: &Path, path: &str) -> Option<PreviewFile> {
    let canonical_root = root.canonicalize().ok()?;
    let target = root.join(path).canonicalize().ok()?;
    if !target.starts_with(canonical_root) || !target.is_file() {
        return None;
    }
    let bytes = std::fs::read(&target).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    let lines: Vec<&str> = text.lines().collect();
    let mut content = String::new();
    let mut truncated = lines.len() > MAX_PREVIEW_LINES;
    for line in lines.iter().take(MAX_PREVIEW_LINES) {
        if content.len() + line.len() + 1 > MAX_PREVIEW_CHARS {
            truncated = true;
            break;
        }
        content.push_str(line);
        content.push('\n');
    }
    let lang = target
        .extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_string);
    Some(PreviewFile {
        path: path.to_string(),
        lang,
        content,
        truncated,
    })
}

#[cfg(test)]
#[path = "tests/files_test.rs"]
mod tests;
