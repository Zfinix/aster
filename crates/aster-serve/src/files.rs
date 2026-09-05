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
/// the real file; anything else is staged in the OS temp dir, where the OS cleans
/// it up and the agent can still read it by absolute path.
pub fn stage(root: &Path, name: &str, size: u64, data: &[u8]) -> Result<String, String> {
    if let Some(existing) = find(root, name, size) {
        return Ok(existing);
    }
    let dir = staging_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("could not stage {name}: {e}"))?;
    let name = sanitize(name);
    // The original name, so the agent and the composer see what was dropped.
    // A collision is the only case that gets a suffix, so a second paste of the
    // same name never overwrites the one already mentioned.
    let mut target = dir.join(&name);
    let mut n = 1;
    while target.exists() {
        target = dir.join(numbered(&name, n));
        n += 1;
    }
    std::fs::write(&target, data).map_err(|e| format!("could not stage {name}: {e}"))?;
    Ok(target.display().to_string())
}

fn numbered(name: &str, n: u32) -> String {
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => format!("{stem}-{n}.{ext}"),
        _ => format!("{name}-{n}"),
    }
}

fn staging_dir() -> PathBuf {
    std::env::temp_dir().join("aster-pasted")
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
    /// A `data:` URL when the file is an image, so the page can show it.
    pub image: Option<String>,
    /// A `data:` URL for a document (pdf, office files), for the preview card.
    pub doc: Option<String>,
    /// File size in bytes, so the card can show it without a second read.
    pub size: Option<u64>,
}

const MAX_BINARY_PREVIEW_BYTES: u64 = 8 * 1024 * 1024;

fn mime_type(ext: &str) -> Option<&'static str> {
    match ext {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        "svg" => Some("image/svg+xml"),
        "bmp" => Some("image/bmp"),
        "ico" => Some("image/x-icon"),
        _ => None,
    }
}

fn doc_mime(ext: &str) -> Option<&'static str> {
    match ext {
        "pdf" => Some("application/pdf"),
        "doc" => Some("application/msword"),
        "docx" => Some("application/vnd.openxmlformats-officedocument.wordprocessingml.document"),
        "xls" => Some("application/vnd.ms-excel"),
        "xlsx" => Some("application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"),
        "ppt" => Some("application/vnd.ms-powerpoint"),
        "pptx" => Some("application/vnd.openxmlformats-officedocument.presentationml.presentation"),
        "odt" => Some("application/vnd.oasis.opendocument.text"),
        "ods" => Some("application/vnd.oasis.opendocument.spreadsheet"),
        "odp" => Some("application/vnd.oasis.opendocument.presentation"),
        "rtf" => Some("application/rtf"),
        "epub" => Some("application/epub+zip"),
        _ => None,
    }
}

/// A bounded peek at a repo file: the head of it, never the whole thing, and
/// never anything outside the repo. Staged pastes are the one exception: they
/// live in the OS temp dir, and the page just mentioned them.
fn resolve(root: &Path, path: &str) -> Option<PathBuf> {
    let canonical_root = root.canonicalize().ok()?;
    let target = root.join(path).canonicalize().ok()?;
    let staged = staging_dir().canonicalize().ok().unwrap_or_default();
    let allowed = target.starts_with(canonical_root)
        || (!staged.as_os_str().is_empty() && target.starts_with(&staged));
    (allowed && target.is_file()).then_some(target)
}

/// The bytes of an allowed file and their mime, for the page to fetch as a URL
/// instead of shipping base64 through a JSON message.
pub fn serve(root: &Path, path: &str) -> Option<(&'static str, Vec<u8>)> {
    let target = resolve(root, path)?;
    let ext = target
        .extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_lowercase)?;
    let mime = mime_type(&ext)
        .or_else(|| doc_mime(&ext))
        .unwrap_or("application/octet-stream");
    if target.metadata().ok()?.len() > MAX_BINARY_PREVIEW_BYTES {
        return None;
    }
    Some((mime, std::fs::read(target).ok()?))
}

pub fn preview(root: &Path, path: &str) -> Option<PreviewFile> {
    let target = resolve(root, path)?;
    let ext = target
        .extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_string);
    let mime = ext
        .as_deref()
        .and_then(|ext| mime_type(ext).map(|mime| (false, mime)))
        .or_else(|| {
            ext.as_deref()
                .and_then(|ext| doc_mime(ext).map(|mime| (true, mime)))
        });
    if let Some((is_doc, mime)) = mime {
        let meta = target.metadata().ok()?;
        let size = meta.len();
        if size > MAX_BINARY_PREVIEW_BYTES {
            return None;
        }
        let bytes = std::fs::read(&target).ok()?;
        use base64::Engine as _;
        let data = format!(
            "data:{mime};base64,{}",
            base64::engine::general_purpose::STANDARD.encode(bytes)
        );
        let (image, doc) = match is_doc {
            true => (None, Some(data)),
            false => (Some(data), None),
        };
        return Some(PreviewFile {
            path: path.to_string(),
            lang: None,
            content: String::new(),
            truncated: false,
            image,
            doc,
            size: Some(size),
        });
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
    let size = std::fs::metadata(&target).ok().map(|meta| meta.len());
    Some(PreviewFile {
        path: path.to_string(),
        lang: ext,
        content,
        truncated,
        image: None,
        doc: None,
        size,
    })
}

#[cfg(test)]
#[path = "tests/files_test.rs"]
mod tests;
