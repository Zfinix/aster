//! The page itself: the same front-end the desktop app runs, built for the web
//! and embedded in the binary. `ASTER_UI_DIR` serves it from disk instead,
//! which is how you work on it without rebuilding the CLI.

use std::env;
use std::path::{Component, Path, PathBuf};

use axum::http::{Uri, header};
use axum::response::{IntoResponse, Response};
use include_dir::{Dir, include_dir};

/// Staged here by `bun run build:web`. Empty in a checkout that has not built
/// the UI yet, which is what [`missing`] explains.
static UI: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/ui");

const INDEX: &str = "index.html";

pub async fn serve(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { INDEX } else { path };

    if let Some(bytes) = read(path) {
        return file(path, bytes);
    }
    // Anything else is a route the front-end owns, so hand it the page and let
    // it work out what to draw.
    match read(INDEX) {
        Some(bytes) => file(INDEX, bytes),
        None => crate::pages::missing_ui(),
    }
}

/// True when a browser has something to load: `aster serve` says so up front
/// rather than opening a window onto nothing.
pub fn is_built() -> bool {
    read(INDEX).is_some()
}

fn read(path: &str) -> Option<Vec<u8>> {
    if let Some(dir) = env::var_os("ASTER_UI_DIR") {
        let path = safe_join(Path::new(&dir), path)?;
        return std::fs::read(path).ok();
    }
    UI.get_file(path).map(|file| file.contents().to_vec())
}

/// Keep a request inside the served directory: no `..`, no absolute paths, no
/// root escape.
fn safe_join(root: &Path, path: &str) -> Option<PathBuf> {
    let relative = Path::new(path);
    relative
        .components()
        .all(|part| matches!(part, Component::Normal(_)))
        .then(|| root.join(relative))
}

fn file(path: &str, bytes: Vec<u8>) -> Response {
    (
        [
            (header::CONTENT_TYPE, content_type(path)),
            // The bundle keeps its name across upgrades, so a cached copy has
            // to be revalidated or a new Aster would serve the old page.
            (header::CACHE_CONTROL, "no-cache"),
        ],
        bytes,
    )
        .into_response()
}

fn content_type(path: &str) -> &'static str {
    match path.rsplit_once('.').map(|(_, ext)| ext) {
        Some("html") => "text/html; charset=utf-8",
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json" | "map") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("gif") => "image/gif",
        Some("ico") => "image/x-icon",
        Some("woff2") => "font/woff2",
        Some("woff") => "font/woff",
        Some("txt") => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}
