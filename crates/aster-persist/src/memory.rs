use std::fs;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::slugify;

pub const PROJECT_MEMORY_FILE: &str = "ASTER.md";
pub const MAX_INDEX_ENTRIES: usize = 60;
pub const JOURNAL_FILE: &str = "journal.jsonl";

pub struct MemoryStore {
    dir: PathBuf,
}

#[derive(Debug, Clone)]
pub struct MemoryMeta {
    pub name: String,
    pub description: String,
    pub path: PathBuf,
    pub source_session: Option<String>,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}

/// A memory event: every write, delete, archive, and recall, one JSON line in
/// `journal.jsonl`. It is the audit trail behind consolidation: decay, merges,
/// and archives can be explained and reverted from it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryOp {
    Remember,
    AppendProject,
    Forget,
    Archive,
    Recall,
    Consolidated,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryJournalEntry {
    pub ts: DateTime<Utc>,
    pub op: MemoryOp,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_session: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct BlockMeta {
    name: Option<String>,
    description: Option<String>,
    source_session: Option<String>,
    created_at: Option<DateTime<Utc>>,
    updated_at: Option<DateTime<Utc>>,
}

impl MemoryStore {
    pub(crate) fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn project_path(&self) -> PathBuf {
        self.dir.join(PROJECT_MEMORY_FILE)
    }

    pub fn load_context(&self) -> Result<String> {
        let mut sections = Vec::new();

        if let Ok(project) = fs::read_to_string(self.project_path()) {
            let project = project.trim();
            if !project.is_empty() {
                sections.push(project.to_string());
            }
        }

        let mut blocks = self.list()?;
        blocks.sort_by(|a, b| {
            recency(b)
                .cmp(&recency(a))
                .then_with(|| a.name.cmp(&b.name))
        });
        let total = blocks.len();
        let shown = total.min(MAX_INDEX_ENTRIES);
        let mut index: Vec<String> = blocks
            .iter()
            .take(shown)
            .map(|b| {
                if b.description.is_empty() {
                    format!("- {}", b.name)
                } else {
                    format!("- {} — {}", b.name, b.description)
                }
            })
            .collect();
        if total > shown {
            index.push(format!(
                "- ... and {} more blocks; run `aster memory list` to see them all.",
                total - shown
            ));
        }
        if !index.is_empty() {
            sections.push(format!(
                "### Recallable memory\nCall `recall(name)` to read any of these in full before relying on it:\n{}",
                index.join("\n")
            ));
        }

        if sections.is_empty() {
            return Ok(String::new());
        }
        Ok(format!("## Memory\n\n{}", sections.join("\n\n")))
    }

    pub fn read_block(&self, name: &str) -> Result<String> {
        let Some(path) = self.find_block_path(name) else {
            anyhow::bail!("no memory block named {name:?}");
        };
        let raw =
            fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        self.log(MemoryOp::Recall, Some(name), None, None)?;
        Ok(strip_frontmatter(&raw).trim().to_string())
    }

    pub fn remember(&self, name: &str, description: &str, body: &str) -> Result<PathBuf> {
        self.write_block(name, description, body, None)
    }

    /// [`Self::remember`] with the transcript id that produced the write, so a
    /// fact can be traced back to the session it came from.
    pub fn remember_sourced(
        &self,
        name: &str,
        description: &str,
        body: &str,
        source_session: &str,
    ) -> Result<PathBuf> {
        self.write_block(name, description, body, Some(source_session))
    }

    fn write_block(
        &self,
        name: &str,
        description: &str,
        body: &str,
        source_session: Option<&str>,
    ) -> Result<PathBuf> {
        fs::create_dir_all(&self.dir)
            .with_context(|| format!("creating {}", self.dir.display()))?;
        let slug = slugify(name);
        let slug = if slug.is_empty() {
            "note".to_string()
        } else {
            slug
        };
        let path = self.dir.join(format!("{slug}.md"));
        let existing = self
            .find_block_path(&slug)
            .and_then(|p| fs::read_to_string(&p).ok())
            .map(|raw| parse_block(&raw));
        let now = Utc::now();
        let created = existing.as_ref().and_then(|m| m.created_at).unwrap_or(now);
        // An edit keeps the block's origin: a rewrite without a session does not
        // erase the session that first created the fact.
        let source = source_session.or(existing.as_ref().and_then(|m| m.source_session.as_deref()));
        let mut front = format!("name: {slug}\ndescription: {}\n", description.trim());
        if let Some(source) = source {
            front.push_str(&format!("source_session: {source}\n"));
        }
        front.push_str(&format!(
            "created_at: {}\nupdated_at: {}\n",
            created.to_rfc3339(),
            now.to_rfc3339()
        ));
        let contents = format!("---\n{front}---\n\n{}\n", body.trim());
        fs::write(&path, contents).with_context(|| format!("writing {}", path.display()))?;
        self.log(MemoryOp::Remember, Some(&slug), None, source_session)?;
        Ok(path)
    }

    pub fn append_project(&self, fact: &str) -> Result<()> {
        self.append_project_inner(fact, None)
    }

    /// [`Self::append_project`] recording the session that produced the fact.
    pub fn append_project_sourced(&self, fact: &str, source_session: &str) -> Result<()> {
        self.append_project_inner(fact, Some(source_session))
    }

    fn append_project_inner(&self, fact: &str, source_session: Option<&str>) -> Result<()> {
        fs::create_dir_all(&self.dir)
            .with_context(|| format!("creating {}", self.dir.display()))?;
        let path = self.project_path();
        let mut existing = fs::read_to_string(&path).unwrap_or_default();
        if existing.is_empty() {
            existing.push_str("# Project memory\n\n");
        } else if !existing.ends_with('\n') {
            existing.push('\n');
        }
        existing.push_str(&format!("- {}\n", fact.trim()));
        fs::write(&path, existing).with_context(|| format!("writing {}", path.display()))?;
        let detail: String = fact.trim().chars().take(200).collect();
        self.log(MemoryOp::AppendProject, None, Some(detail), source_session)?;
        Ok(())
    }

    /// Delete a block by name or slug. `false` when there was no such block.
    pub fn forget(&self, name: &str) -> Result<bool> {
        let Some(path) = self.find_block_path(name) else {
            return Ok(false);
        };
        fs::remove_file(&path).with_context(|| format!("removing {}", path.display()))?;
        self.log(MemoryOp::Forget, Some(name), None, None)?;
        Ok(true)
    }

    /// Move a block to `.archive/`, keeping it out of the prompt index but
    /// recoverable and journaled. This is how decay and supersession retire
    /// stale facts without ever silently losing them.
    pub fn archive(&self, name: &str) -> Result<bool> {
        let Some(path) = self.find_block_path(name) else {
            return Ok(false);
        };
        let archive_dir = self.dir.join(".archive");
        fs::create_dir_all(&archive_dir)
            .with_context(|| format!("creating {}", archive_dir.display()))?;
        let dest = archive_dir.join(
            path.file_name()
                .with_context(|| format!("unexpected block path {}", path.display()))?,
        );
        fs::rename(&path, &dest)
            .with_context(|| format!("archiving {} to {}", path.display(), dest.display()))?;
        self.log(MemoryOp::Archive, Some(name), None, None)?;
        Ok(true)
    }

    fn find_block_path(&self, name: &str) -> Option<PathBuf> {
        let slug = slugify(name);
        let direct = self.dir.join(format!("{slug}.md"));
        if direct.exists() {
            return Some(direct);
        }
        self.list().ok()?.into_iter().find_map(|b| {
            (b.name.eq_ignore_ascii_case(name) || slugify(&b.name) == slug).then_some(b.path)
        })
    }

    fn journal_path(&self) -> PathBuf {
        self.dir.join(JOURNAL_FILE)
    }

    /// Read the memory journal, newest first. Every line is one auditable
    /// memory event; consolidation and the CLI `memory log` consume this.
    pub fn journal(&self) -> Result<Vec<MemoryJournalEntry>> {
        let path = self.journal_path();
        let Ok(raw) = fs::read_to_string(&path) else {
            return Ok(Vec::new());
        };
        let mut entries = Vec::new();
        for line in raw.lines() {
            if let Ok(entry) = serde_json::from_str::<MemoryJournalEntry>(line) {
                entries.push(entry);
            }
        }
        entries.sort_by_key(|e| std::cmp::Reverse(e.ts));
        Ok(entries)
    }

    /// Record that a consolidation pass finished for `session_id`, so the same
    /// session is never distilled twice.
    pub fn record_consolidated(&self, session_id: &str) -> Result<()> {
        self.log(MemoryOp::Consolidated, None, None, Some(session_id))
    }

    fn log(
        &self,
        op: MemoryOp,
        name: Option<&str>,
        detail: Option<String>,
        source_session: Option<&str>,
    ) -> Result<()> {
        fs::create_dir_all(&self.dir)
            .with_context(|| format!("creating {}", self.dir.display()))?;
        let entry = MemoryJournalEntry {
            ts: Utc::now(),
            op,
            name: name.map(str::to_string),
            detail,
            source_session: source_session.map(str::to_string),
        };
        let line = serde_json::to_string(&entry).context("serializing memory journal entry")?;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.journal_path())
            .with_context(|| format!("opening {}", self.journal_path().display()))?;
        file.write_all(line.as_bytes())
            .context("writing memory journal")?;
        file.write_all(b"\n").context("writing memory journal")?;
        Ok(())
    }

    pub fn list(&self) -> Result<Vec<MemoryMeta>> {
        let mut blocks = Vec::new();
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return Ok(blocks);
        };
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            if path.file_name().and_then(|n| n.to_str()) == Some(PROJECT_MEMORY_FILE) {
                continue;
            }
            let raw = fs::read_to_string(&path).unwrap_or_default();
            let meta = parse_block(&raw);
            let name = meta.name.unwrap_or_else(|| {
                path.file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("note")
                    .to_string()
            });
            blocks.push(MemoryMeta {
                name,
                description: meta.description.unwrap_or_default(),
                path,
                source_session: meta.source_session,
                created_at: meta.created_at,
                updated_at: meta.updated_at,
            });
        }
        blocks.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(blocks)
    }
}

fn strip_frontmatter(raw: &str) -> &str {
    let trimmed = raw.trim_start();
    if let Some(rest) = trimmed.strip_prefix("---")
        && let Some(end) = rest.find("\n---")
    {
        return rest[end + 4..].trim_start_matches('\n');
    }
    raw
}

fn parse_block(raw: &str) -> BlockMeta {
    let trimmed = raw.trim_start();
    let Some(rest) = trimmed.strip_prefix("---") else {
        return BlockMeta::default();
    };
    let Some(end) = rest.find("\n---") else {
        return BlockMeta::default();
    };
    let front = &rest[..end];
    let mut meta = BlockMeta::default();
    for line in front.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        match key.trim() {
            "name" => meta.name = Some(value.to_string()),
            "description" => meta.description = Some(value.to_string()),
            "source_session" => meta.source_session = Some(value.to_string()),
            "created_at" => meta.created_at = parse_ts(value),
            "updated_at" => meta.updated_at = parse_ts(value),
            _ => {}
        }
    }
    meta
}

fn parse_ts(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

fn recency(block: &MemoryMeta) -> DateTime<Utc> {
    block.updated_at.or(block.created_at).unwrap_or(Utc::now())
}

#[cfg(test)]
#[path = "memory_tests.rs"]
mod tests;
