#![forbid(unsafe_code)]
//! Local artifacts and design canvases: one self-contained HTML page per
//! artifact, and a set of artboards plus a generated viewer per canvas. Both
//! live under `.aster/` in the repo so `open_preview` can open them by path.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

const ARTIFACTS: &str = ".aster/artifacts";
const CANVASES: &str = ".aster/canvases";

/// Reduce a name to one safe path segment: lowercase letters, digits and `-`.
pub fn slug(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut dash = false;
    for ch in name.chars().flat_map(char::to_lowercase) {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            dash = false;
        } else if !out.is_empty() && !dash {
            out.push('-');
            dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArtifactMeta {
    pub name: String,
    pub title: String,
    pub updated: String,
}

/// Write or replace an artifact page and its metadata.
pub fn write_artifact(root: &Path, name: &str, title: &str, html: &str) -> Result<PathBuf> {
    let id = slug(name);
    if id.is_empty() {
        bail!("artifact needs a `name` using letters or digits");
    }
    let dir = root.join(ARTIFACTS).join(&id);
    fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let page = dir.join("index.html");
    fs::write(&page, html).with_context(|| format!("writing {}", page.display()))?;
    let meta = ArtifactMeta {
        name: id,
        title: title.trim().to_string(),
        updated: now(),
    };
    fs::write(
        dir.join("artifact.json"),
        serde_json::to_string_pretty(&meta)?,
    )?;
    Ok(page)
}

pub fn artifact_page(root: &Path, name: &str) -> PathBuf {
    root.join(ARTIFACTS).join(slug(name)).join("index.html")
}

/// Every artifact, newest first.
pub fn list_artifacts(root: &Path) -> Vec<ArtifactMeta> {
    let Ok(entries) = fs::read_dir(root.join(ARTIFACTS)) else {
        return Vec::new();
    };
    let mut out: Vec<ArtifactMeta> = entries
        .flatten()
        .filter_map(|entry| {
            let raw = fs::read_to_string(entry.path().join("artifact.json")).ok()?;
            serde_json::from_str(&raw).ok()
        })
        .collect();
    out.sort_by(|a, b| b.updated.cmp(&a.updated));
    out
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Board {
    pub id: String,
    pub title: String,
    pub file: String,
    pub w: u32,
    pub h: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct Canvas {
    pub title: String,
    #[serde(default)]
    pub boards: Vec<Board>,
}

fn read_canvas(path: &Path) -> Result<Canvas> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("no canvas at {}; create it first", path.display()))?;
    serde_json::from_str(&raw).with_context(|| format!("reading {}", path.display()))
}

fn write_canvas(path: &Path, canvas: &Canvas) -> Result<()> {
    fs::write(path, serde_json::to_string_pretty(canvas)?)
        .with_context(|| format!("writing {}", path.display()))
}

fn canvas_dir(root: &Path, name: &str) -> Result<PathBuf> {
    let id = slug(name);
    if id.is_empty() {
        bail!("canvas needs a `name` using letters or digits");
    }
    Ok(root.join(CANVASES).join(id))
}

pub fn create_canvas(root: &Path, name: &str, title: &str) -> Result<PathBuf> {
    let dir = canvas_dir(root, name)?;
    let path = dir.join("canvas.json");
    if path.exists() {
        bail!("canvas `{}` already exists", slug(name));
    }
    fs::create_dir_all(dir.join("project"))?;
    let canvas = Canvas {
        title: title.trim().to_string(),
        boards: Vec::new(),
    };
    write_canvas(&path, &canvas)?;
    Ok(path)
}

/// Write or replace one board in a canvas, keeping its place in the order.
pub fn add_board(
    root: &Path,
    name: &str,
    board: &str,
    title: &str,
    html: &str,
    w: u32,
    h: u32,
) -> Result<PathBuf> {
    let dir = canvas_dir(root, name)?;
    let path = dir.join("canvas.json");
    let mut canvas = read_canvas(&path)?;
    let id = slug(board);
    if id.is_empty() {
        bail!("board needs a `board` name using letters or digits");
    }
    let file = format!("project/{id}.html");
    let page = dir.join(&file);
    fs::write(&page, html).with_context(|| format!("writing {}", page.display()))?;
    let board = Board {
        id,
        title: title.trim().to_string(),
        file,
        w: w.max(200),
        h: h.max(200),
    };
    match canvas.boards.iter_mut().find(|b| b.id == board.id) {
        Some(existing) => *existing = board,
        None => canvas.boards.push(board),
    }
    write_canvas(&path, &canvas)?;
    Ok(page)
}

/// Every canvas with its boards, sorted by name.
pub fn list_canvases(root: &Path) -> Vec<(String, Canvas)> {
    let Ok(entries) = fs::read_dir(root.join(CANVASES)) else {
        return Vec::new();
    };
    let mut out: Vec<(String, Canvas)> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            let raw = fs::read_to_string(entry.path().join("canvas.json")).ok()?;
            let canvas = serde_json::from_str(&raw).ok()?;
            Some((name, canvas))
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// Render the canvas viewer: every board in its own frame, laid out in rows.
pub fn write_viewer(root: &Path, name: &str) -> Result<PathBuf> {
    let dir = canvas_dir(root, name)?;
    let canvas = read_canvas(&dir.join("canvas.json"))?;
    let boards = if canvas.boards.is_empty() {
        "<p class=\"empty\">No boards yet. Add one with the canvas tool.</p>\n".to_string()
    } else {
        canvas
            .boards
            .iter()
            .map(|board| {
                format!(
                    "<section class=\"board\"><h2>{title}</h2><iframe src=\"{file}\" width=\"{w}\" height=\"{h}\" title=\"{title}\" loading=\"lazy\"></iframe></section>\n",
                    title = escape(&board.title),
                    file = escape(&board.file),
                    w = board.w,
                    h = board.h,
                )
            })
            .collect()
    };
    let title = if canvas.title.trim().is_empty() {
        slug(name)
    } else {
        escape(&canvas.title)
    };
    let count = canvas.boards.len();
    let html = format!(
        "<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>{title}</title>\n<style>\n:root{{color-scheme:dark light}}\nbody{{margin:0;background:#0b0b0b;color:#ededed;font:13px/1.5 system-ui,-apple-system,'Segoe UI',sans-serif}}\nheader{{position:sticky;top:0;z-index:1;display:flex;align-items:baseline;gap:12px;padding:14px 20px;background:rgba(11,11,11,.92);border-bottom:1px solid #1f1f1f;backdrop-filter:blur(8px)}}\nh1{{margin:0;font-size:14px;font-weight:600}}\n.count{{color:#8f8f8f;font-size:12px}}\nmain{{display:flex;flex-wrap:wrap;gap:32px;padding:24px;align-items:flex-start}}\n.board{{display:flex;flex-direction:column;gap:8px}}\n.board h2{{margin:0;font-size:12px;font-weight:600;color:#8f8f8f}}\niframe{{border:1px solid #262626;border-radius:8px;background:#0d0d0d;display:block;max-width:100%}}\n.empty{{padding:24px;color:#8f8f8f}}\n</style></head>\n<body><header><h1>{title}</h1><span class=\"count\">{count} boards</span></header>\n<main>\n{boards}</main></body></html>\n"
    );
    let path = dir.join("viewer.html");
    fs::write(&path, html).with_context(|| format!("writing {}", path.display()))?;
    Ok(path)
}

#[cfg(test)]
#[path = "tests/lib_test.rs"]
mod tests;
