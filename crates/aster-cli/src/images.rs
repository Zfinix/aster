//! Files a turn mentions, read off disk and attached to it. Every surface sends
//! its turn as text with `@path` mentions, so resolving them here gives the TUI,
//! the editors, and a piped prompt file input at once. Images ride as image
//! parts; any other readable file is parsed to Markdown and appended as text.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use aster_ai::{ContentPart, ImageUrl, MessageContent};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use image::ImageFormat;

const EXTENSIONS: [&str; 6] = ["png", "jpg", "jpeg", "gif", "webp", "bmp"];

/// Cap on what is sent to the provider, after downscaling.
const MAX_BYTES: u64 = 10 * 1024 * 1024;

/// Cap on what we will read and decode. Oversized images are downscaled to
/// fit [`MAX_BYTES`] rather than rejected, so this only guards memory.
const MAX_RAW_BYTES: u64 = 64 * 1024 * 1024;

const MAX_EDGE: u32 = 2048;

const MAX_DOC_CHARS: usize = 8_000;

const MAX_ATTACHED_DOCS: usize = 3;

const MAX_ATTACHED_IMAGES: usize = 4;

/// The turn with each file it mentions attached. The mention stays in the text:
/// it is what the user wrote, and what tells the model which file is which when
/// a turn carries several.
pub(crate) fn attach(text: &str, repo_root: &Path) -> MessageContent {
    let (images, docs) = mentioned_all(text, repo_root);
    if images.is_empty() && docs.is_empty() {
        return MessageContent::Text(text.to_string());
    }
    let mut parts = vec![ContentPart::Text {
        text: text.to_string(),
    }];
    for doc in docs.iter().take(MAX_ATTACHED_DOCS) {
        match read_text_or_document(doc) {
            Ok(body) => {
                let total = body.len();
                let mut cut = MAX_DOC_CHARS.min(total);
                while !body.is_char_boundary(cut) {
                    cut -= 1;
                }
                let mut text = format!(
                    "Contents of @{}:\n{}",
                    display(doc, repo_root),
                    &body[..cut]
                );
                if total > MAX_DOC_CHARS {
                    text.push_str(&format!(
                        "\n... [truncated: first {cut} of {total} bytes; \
                         read_file serves the rest in ranges]"
                    ));
                }
                parts.push(ContentPart::Text { text });
            }
            // A broken attachment must not cost the turn, but it must not be
            // silent either: the model has to know the file it was asked
            // about never arrived.
            Err(err) => {
                eprintln!("  ! {}: {err:#}", doc.display());
                parts.push(ContentPart::Text {
                    text: format!(
                        "[@{} could not be attached: {err:#}]",
                        display(doc, repo_root)
                    ),
                });
            }
        }
    }
    for path in images.iter().take(MAX_ATTACHED_IMAGES) {
        match encode(path) {
            Ok(url) => parts.push(ContentPart::ImageUrl {
                image_url: ImageUrl { url },
            }),
            Err(err) => {
                eprintln!("  ! {}: {err:#}", path.display());
                parts.push(ContentPart::Text {
                    text: format!(
                        "[@{} could not be attached: {err:#}]",
                        display(path, repo_root)
                    ),
                });
            }
        }
    }
    if images.len() > MAX_ATTACHED_IMAGES {
        let dropped: Vec<String> = images[MAX_ATTACHED_IMAGES..]
            .iter()
            .map(|p| display(p, repo_root))
            .collect();
        parts.push(ContentPart::Text {
            text: format!(
                "[{} not attached: at most {MAX_ATTACHED_IMAGES} images per turn]",
                dropped.join(", ")
            ),
        });
    }
    if parts.len() == 1 {
        return MessageContent::Text(text.to_string());
    }
    MessageContent::Parts(parts)
}

/// A document or text file as Markdown, via anydoc when the format needs it.
/// What `read_file` serves, so an attached file and a looked-up file read alike.
pub(crate) fn read_text_or_document(target: &Path) -> Result<String> {
    let bytes = fs::read(target).with_context(|| format!("reading {}", target.display()))?;
    // Sniff the bytes before trusting the extension, so a document hiding
    // behind a wrong or missing extension still converts. CSV stays raw text.
    if let Some(format) = anydoc::Format::from_bytes(&bytes)
        && !matches!(format, anydoc::Format::Csv)
    {
        return anydoc::to_markdown_bytes(&bytes, format)
            .map_err(|e| anyhow::anyhow!("converting {} to Markdown: {e}", target.display()));
    }
    match String::from_utf8(bytes) {
        Ok(text) => Ok(text),
        Err(_) => bail!(
            "{} is a binary file, not readable as text; images the user mentions \
             arrive as image parts, and only document formats (PDF, Office, EPUB, \
             RTF) convert to Markdown",
            target.display()
        ),
    }
}

fn display(path: &Path, repo_root: &Path) -> String {
    path.strip_prefix(repo_root)
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned()
}

fn mentioned_all(text: &str, repo_root: &Path) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let mut images: Vec<PathBuf> = Vec::new();
    let mut docs: Vec<PathBuf> = Vec::new();
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
                && !images.contains(&path)
                && !docs.contains(&path)
            {
                if has_image_extension(path.to_str().unwrap_or_default()) {
                    images.push(path);
                } else {
                    docs.push(path);
                }
            }
            at = next;
        }
    }
    (images, docs)
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
    if token.is_empty() {
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
    encode_bytes(&std::fs::read(path)?)
}

/// Image bytes as a `data:` URL, downscaled to [`MAX_EDGE`] first. Always PNG:
/// re-encoding one format is one thing to be right about, and the provider
/// cares about pixels rather than container. An image over [`MAX_BYTES`] is
/// downscaled until it fits; only one that will not compress below the cap,
/// or one over [`MAX_RAW_BYTES`], is rejected.
pub(crate) fn encode_bytes(bytes: &[u8]) -> anyhow::Result<String> {
    anyhow::ensure!(
        bytes.len() as u64 <= MAX_RAW_BYTES,
        "image is {}MB, over the {}MB paste limit; shrink it first",
        bytes.len() as u64 / 1024 / 1024,
        MAX_RAW_BYTES / 1024 / 1024
    );
    let decoded = image::load_from_memory(bytes)?;
    let mut fitted = if decoded.width() > MAX_EDGE || decoded.height() > MAX_EDGE {
        decoded.thumbnail(MAX_EDGE, MAX_EDGE)
    } else {
        decoded
    };
    let mut png = std::io::Cursor::new(Vec::new());
    fitted.write_to(&mut png, ImageFormat::Png)?;
    while png.get_ref().len() as u64 > MAX_BYTES {
        let next = (fitted.width() / 2).max(1);
        fitted = fitted.thumbnail(next, next);
        png = std::io::Cursor::new(Vec::new());
        fitted.write_to(&mut png, ImageFormat::Png)?;
    }
    Ok(format!(
        "data:image/png;base64,{}",
        STANDARD.encode(png.into_inner())
    ))
}

#[cfg(test)]
#[path = "tests/images_test.rs"]
mod tests;
