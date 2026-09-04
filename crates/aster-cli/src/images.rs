//! Images a turn mentions, read off disk and attached to it. Every surface sends
//! its turn as text with `@path` mentions, so resolving them here gives the TUI,
//! the editors, and a piped prompt image input at once.

use std::path::{Path, PathBuf};

use aster_ai::{ContentPart, ImageUrl, MessageContent};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use image::ImageFormat;

const EXTENSIONS: [&str; 6] = ["png", "jpg", "jpeg", "gif", "webp", "bmp"];

const MAX_BYTES: u64 = 10 * 1024 * 1024;

const MAX_EDGE: u32 = 2048;

/// The turn with each image it mentions attached. The mention stays in the text:
/// it is what the user wrote, and what tells the model which image is which when
/// a turn carries several.
pub(crate) fn attach(text: &str, repo_root: &Path) -> MessageContent {
    let paths = mentioned(text, repo_root);
    if paths.is_empty() {
        return MessageContent::Text(text.to_string());
    }
    let mut parts = vec![ContentPart::Text {
        text: text.to_string(),
    }];
    for path in paths {
        match encode(&path) {
            Ok(url) => parts.push(ContentPart::ImageUrl {
                image_url: ImageUrl { url },
            }),
            // A broken attachment must not cost the turn: the text still says
            // what the user asked, and the mention still names the file.
            Err(err) => eprintln!("  ! {}: {err:#}", path.display()),
        }
    }
    if parts.len() == 1 {
        return MessageContent::Text(text.to_string());
    }
    MessageContent::Parts(parts)
}

fn mentioned(text: &str, repo_root: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for line in text.lines() {
        let mut at = 0;
        while at < line.len() {
            let rest = &line[at..];
            let lead = rest.len() - rest.trim_start().len();
            if lead > 0 {
                at += lead;
                continue;
            }
            let token = rest.split_whitespace().next().unwrap_or_default();
            let mut next = at + token.len();
            let mut found = on_disk(trim_punctuation(token).trim_start_matches('@'), repo_root);
            if found.is_none()
                && token.starts_with('@')
                && let Some((path, len)) = spanning_spaces(&rest[1..], repo_root)
            {
                found = Some(path);
                next = at + 1 + len;
            }
            if let Some(path) = found
                && !out.contains(&path)
            {
                out.push(path);
            }
            at = next;
        }
    }
    out
}

fn spanning_spaces(rest: &str, repo_root: &Path) -> Option<(PathBuf, usize)> {
    let lead = rest.len() - rest.trim_start_matches(['"', '\'']).len();
    let body = &rest[lead..];
    for (dot, _) in body.match_indices('.') {
        let after = &body[dot + 1..];
        let end = dot
            + 1
            + after
                .find(|c: char| !c.is_ascii_alphanumeric())
                .unwrap_or(after.len());
        let candidate = &body[..end];
        if !candidate.contains(char::is_whitespace) {
            continue;
        }
        if let Some(path) = on_disk(candidate, repo_root) {
            return Some((path, lead + end));
        }
    }
    None
}

fn on_disk(token: &str, repo_root: &Path) -> Option<PathBuf> {
    if !has_image_extension(token) {
        return None;
    }
    let path = resolve(token, repo_root);
    path.is_file().then_some(path)
}

fn trim_punctuation(token: &str) -> &str {
    token.trim_matches(|c: char| matches!(c, '"' | '\'' | '(' | ')' | '[' | ']' | ',' | ';' | ':'))
}

fn has_image_extension(token: &str) -> bool {
    Path::new(token)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| {
            EXTENSIONS
                .iter()
                .any(|known| known.eq_ignore_ascii_case(ext))
        })
}

fn resolve(token: &str, repo_root: &Path) -> PathBuf {
    if let Some(rest) = token.strip_prefix("~/")
        && let Some(home) = dirs::home_dir()
    {
        return home.join(rest);
    }
    let path = Path::new(token);
    if path.is_absolute() {
        return path.to_path_buf();
    }
    repo_root.join(path)
}

fn encode(path: &Path) -> anyhow::Result<String> {
    let size = std::fs::metadata(path)?.len();
    anyhow::ensure!(
        size <= MAX_BYTES,
        "image is {}MB, over the {}MB limit",
        size / 1024 / 1024,
        MAX_BYTES / 1024 / 1024
    );
    encode_bytes(&std::fs::read(path)?)
}

/// Image bytes as a `data:` URL, downscaled to [`MAX_EDGE`] first. Always PNG:
/// re-encoding one format is one thing to be right about, and the provider
/// cares about pixels rather than container.
pub(crate) fn encode_bytes(bytes: &[u8]) -> anyhow::Result<String> {
    anyhow::ensure!(
        bytes.len() as u64 <= MAX_BYTES,
        "image is {}MB, over the {}MB limit",
        bytes.len() as u64 / 1024 / 1024,
        MAX_BYTES / 1024 / 1024
    );
    let decoded = image::load_from_memory(bytes)?;
    let fitted = if decoded.width() > MAX_EDGE || decoded.height() > MAX_EDGE {
        decoded.thumbnail(MAX_EDGE, MAX_EDGE)
    } else {
        decoded
    };
    let mut png = std::io::Cursor::new(Vec::new());
    fitted.write_to(&mut png, ImageFormat::Png)?;
    Ok(format!(
        "data:image/png;base64,{}",
        STANDARD.encode(png.into_inner())
    ))
}

#[cfg(test)]
#[path = "tests/images_test.rs"]
mod tests;
