//! Images a turn mentions, read off disk and attached to it. Every surface sends
//! its turn as text with `@path` mentions, so resolving them here gives the TUI,
//! the editors, and a piped prompt image input at once.

use std::path::{Path, PathBuf};

use aster_ai::{ContentPart, ImageUrl, MessageContent};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use image::ImageFormat;

/// Extensions worth attaching. Documents are excluded on purpose: the agent
/// reads those with `read_file`, and a PDF is not something to send as pixels.
const EXTENSIONS: [&str; 6] = ["png", "jpg", "jpeg", "gif", "webp", "bmp"];

/// Beyond this an image is more likely a mistake than a question about a
/// screenshot, and the re-encode would cost more than the turn is worth.
const MAX_BYTES: u64 = 10 * 1024 * 1024;

/// Long edge every image is fitted to before sending. Providers charge by
/// pixels and cap dimensions well below what a modern screenshot arrives at.
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

/// Paths in `text` that name an image on disk, in the order written and
/// without repeats.
fn mentioned(text: &str, repo_root: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for token in text.split_whitespace() {
        let token = token
            .trim_matches(|c: char| {
                matches!(c, '"' | '\'' | '(' | ')' | '[' | ']' | ',' | ';' | ':')
            })
            .trim_start_matches('@');
        if !has_image_extension(token) {
            continue;
        }
        let path = resolve(token, repo_root);
        if path.is_file() && !out.contains(&path) {
            out.push(path);
        }
    }
    out
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

/// A mention is repo-relative unless it is absolute or starts at home.
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

/// One image file as a `data:` URL.
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
