#![forbid(unsafe_code)]

mod grants;
mod memory;
mod transcript;

pub use grants::GrantStore;
pub use memory::{MemoryMeta, MemoryStore, PROJECT_MEMORY_FILE};
pub use transcript::{
    EventUsage, EvictionEvent, MessageEvent, SessionMeta, SessionTranscript, SessionWriter,
    SummaryEvent, TRANSCRIPT_VERSION, TranscriptEvent,
};

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::Utc;
use ulid::Ulid;

#[derive(Clone)]
pub struct Store {
    home: PathBuf,
}

impl Store {
    pub fn open(home: impl Into<PathBuf>) -> Result<Self> {
        let home = home.into();
        std::fs::create_dir_all(&home).with_context(|| format!("creating {}", home.display()))?;
        Ok(Self { home })
    }

    pub fn memory(&self) -> MemoryStore {
        MemoryStore::new(self.home.join("memory"))
    }

    /// Out-of-repo directories approved for `repo_root`, scoped per project so
    /// one project's approval never widens another's reach.
    pub fn grants(&self, repo_root: &Path) -> GrantStore {
        GrantStore::new(
            self.home
                .join("grants")
                .join(format!("{}.json", project_slug(repo_root))),
        )
    }

    fn sessions_dir(&self, repo_root: &Path) -> PathBuf {
        self.home.join("sessions").join(project_slug(repo_root))
    }

    pub fn new_session(
        &self,
        repo_root: &Path,
        cwd: &Path,
        model: Option<String>,
    ) -> Result<SessionWriter> {
        let id = Ulid::new().to_string();
        let meta = SessionMeta {
            id: id.clone(),
            v: TRANSCRIPT_VERSION,
            created_at: Utc::now(),
            cwd: cwd.to_string_lossy().into_owned(),
            repo_root: repo_root.to_string_lossy().into_owned(),
            model,
            aster_version: option_env!("CARGO_PKG_VERSION").map(str::to_string),
            title: None,
        };
        let path = self.sessions_dir(repo_root).join(format!("{id}.jsonl"));
        SessionWriter::create(path, meta)
    }

    pub fn session_writer_for(
        &self,
        repo_root: &Path,
        id: &str,
        cwd: &Path,
        model: Option<String>,
    ) -> Result<SessionWriter> {
        let id = slugify(id);
        let id = if id.is_empty() {
            Ulid::new().to_string()
        } else {
            id
        };
        let path = self.sessions_dir(repo_root).join(format!("{id}.jsonl"));
        if path.exists() {
            let transcript = SessionTranscript::load(&path)?;
            return SessionWriter::reopen(path, transcript.meta);
        }
        let meta = SessionMeta {
            id,
            v: TRANSCRIPT_VERSION,
            created_at: Utc::now(),
            cwd: cwd.to_string_lossy().into_owned(),
            repo_root: repo_root.to_string_lossy().into_owned(),
            model,
            aster_version: option_env!("CARGO_PKG_VERSION").map(str::to_string),
            title: None,
        };
        SessionWriter::create(path, meta)
    }

    pub fn resume(&self, repo_root: &Path, id: &str) -> Result<SessionTranscript> {
        let path = self.sessions_dir(repo_root).join(format!("{id}.jsonl"));
        SessionTranscript::load(&path)
    }

    pub fn resume_writer(&self, repo_root: &Path, id: &str) -> Result<SessionWriter> {
        let path = self.sessions_dir(repo_root).join(format!("{id}.jsonl"));
        let transcript = SessionTranscript::load(&path)?;
        SessionWriter::reopen(path, transcript.meta)
    }

    pub fn latest(&self, repo_root: &Path) -> Result<Option<SessionTranscript>> {
        let Some(id) = self.session_ids(repo_root)?.pop() else {
            return Ok(None);
        };
        Ok(Some(self.resume(repo_root, &id)?))
    }

    pub fn list_sessions(&self, repo_root: &Path) -> Result<Vec<SessionMeta>> {
        let dir = self.sessions_dir(repo_root);
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return Ok(Vec::new());
        };
        let mut metas = Vec::new();
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            if let Ok(header) = read_header(&path) {
                metas.push(header);
            }
        }
        metas.sort_by(|a, b| b.id.cmp(&a.id));
        Ok(metas)
    }

    /// Delete a saved session transcript. Returns whether it existed; the id
    /// is slugified so it can never escape the sessions directory.
    pub fn delete_session(&self, repo_root: &Path, id: &str) -> Result<bool> {
        let path = self
            .sessions_dir(repo_root)
            .join(format!("{}.jsonl", slugify(id)));
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(e).with_context(|| format!("deleting session {}", path.display())),
        }
    }

    fn session_ids(&self, repo_root: &Path) -> Result<Vec<String>> {
        let dir = self.sessions_dir(repo_root);
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return Ok(Vec::new());
        };
        let mut ids: Vec<String> = entries
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let path = e.path();
                if path.extension().and_then(|x| x.to_str()) != Some("jsonl") {
                    return None;
                }
                path.file_stem()
                    .and_then(|s| s.to_str())
                    .map(str::to_string)
            })
            .collect();
        ids.sort();
        Ok(ids)
    }
}

fn read_header(path: &Path) -> Result<SessionMeta> {
    use std::io::{BufRead, BufReader};
    let file = std::fs::File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    match serde_json::from_str::<TranscriptEvent>(line.trim())? {
        TranscriptEvent::Session(meta) => Ok(meta),
        _ => anyhow::bail!("first line of {} is not a session header", path.display()),
    }
}

pub fn project_slug(repo_root: &Path) -> String {
    let slug = slugify(&repo_root.to_string_lossy());
    if slug.is_empty() {
        "default".to_string()
    } else {
        slug
    }
}

pub fn slugify(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut prev_dash = false;
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash && !out.is_empty() {
            out.push('-');
            prev_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}
