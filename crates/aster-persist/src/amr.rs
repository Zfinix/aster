//! The Agent Memory Repo backend: durable memory kept in its own git repo
//! (default `~/agent-memory`) instead of the block store under the aster data
//! dir. Format follows the AgentMemoryRepo protocol: `MEMORY.md` is the index,
//! one-line bullets carry `[key: value; ...]` metadata, longer facts live in
//! topic files linked as `[[slug]]`, and every edit is a commit. Local-only;
//! pushing to a remote is left to the user.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use chrono::Utc;

use crate::memory::{MemoryMeta, MemoryStore};
use crate::slugify;

pub const MEMORY_FILE: &str = "MEMORY.md";
/// Hard cap on how much of `MEMORY.md` enters the prompt in one request.
pub const MAX_AMR_CONTEXT_CHARS: usize = 8_000;
/// Facts at or under this length stay one-line bullets; longer ones get a
/// topic file and an index line.
const MAX_INLINE_CHARS: usize = 240;

/// One fact to bring in with [`AmrStore::import`].
#[derive(Debug, PartialEq)]
pub struct ImportEntry {
    /// Heading the fact files under, e.g. `Project` or the source file's
    /// section title.
    pub section: String,
    /// The fact itself.
    pub body: String,
}

pub struct AmrStore {
    root: PathBuf,
}

impl AmrStore {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn dir(&self) -> &Path {
        &self.root
    }

    fn index_path(&self) -> PathBuf {
        self.root.join(MEMORY_FILE)
    }

    /// Create the repo on first use: `git init`, a local identity when the
    /// machine has none, and a committed `MEMORY.md` so the clean-tree check
    /// has something to stand on.
    pub fn ensure_repo(&self) -> Result<()> {
        fs::create_dir_all(&self.root)
            .with_context(|| format!("creating {}", self.root.display()))?;
        if !self.root.join(".git").exists() {
            self.git(&["init"])?;
        }
        // Exit 1 here means "no identity configured anywhere", not an error;
        // a machine without a global gitconfig must still get a local one.
        if self
            .git_output(&["config", "user.email"])
            .unwrap_or_default()
            .trim()
            .is_empty()
        {
            self.git(&["config", "user.name", "aster"])?;
            self.git(&["config", "user.email", "aster@localhost"])?;
        }
        if !self.index_path().exists() {
            fs::write(self.index_path(), "# Memory\n")
                .with_context(|| format!("writing {}", self.index_path().display()))?;
            self.stage_and_commit("init memory repo")?;
        }
        Ok(())
    }

    /// The tree must be clean before a write so a commit never sweeps up
    /// changes the user made by hand.
    fn ensure_clean(&self) -> Result<()> {
        let status = self.git_output(&["status", "--porcelain"])?;
        anyhow::ensure!(
            status.trim().is_empty(),
            "{} has uncommitted changes; commit or stash them, then try again",
            self.root.display()
        );
        Ok(())
    }

    fn git(&self, args: &[&str]) -> Result<()> {
        self.git_output(args).map(|_| ())
    }

    fn git_output(&self, args: &[&str]) -> Result<String> {
        let out = Command::new("git")
            .arg("-C")
            .arg(&self.root)
            .args(args)
            .output()
            .map_err(|e| anyhow::anyhow!("git is needed for the agent memory repo: {e}"))?;
        anyhow::ensure!(
            out.status.success(),
            "git {} failed: {}",
            args.first().unwrap_or(&""),
            [out.stderr, out.stdout]
                .iter()
                .map(|s| String::from_utf8_lossy(s).trim().to_string())
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(" | ")
        );
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    fn stage_and_commit(&self, message: &str) -> Result<()> {
        self.git(&["add", "-A"])?;
        // --no-gpg-sign: a global signing setup would make every save fail or
        // prompt, and a local memory repo gains nothing from signatures.
        self.git(&["commit", "--no-gpg-sign", "-m", message, "--quiet"])?;
        Ok(())
    }

    fn today() -> String {
        Utc::now().format("%Y-%m-%d").to_string()
    }

    fn append_bullet(&self, section: &str, line: &str) -> Result<PathBuf> {
        self.ensure_repo()?;
        self.ensure_clean()?;
        let path = self.append_bullet_in(section, line)?;
        self.stage_and_commit("remember")?;
        Ok(path)
    }

    /// Append without the clean-tree check, for the topic-file path that
    /// commits the file and its index line together.
    fn append_bullet_in(&self, section: &str, line: &str) -> Result<PathBuf> {
        let path = self.index_path();
        let mut text =
            fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        let heading = format!("## {section}");
        if let Some(at) = text.find(&heading) {
            let after = &text[at + heading.len()..];
            let insert_at =
                at + heading.len() + after.find("\n- ").map(|p| p + 1).unwrap_or(after.len());
            text.insert_str(insert_at, format!("{line}\n").as_str());
        } else {
            if !text.ends_with('\n') {
                text.push('\n');
            }
            text.push_str(&format!("\n{heading}\n\n{line}\n"));
        }
        fs::write(&path, text).with_context(|| format!("writing {}", path.display()))?;
        Ok(path)
    }

    fn remember_inner(
        &self,
        name: &str,
        description: &str,
        body: &str,
        source_session: Option<&str>,
    ) -> Result<PathBuf> {
        let added = Self::today();
        let mut meta = format!("added: {added}");
        if let Some(sid) = source_session {
            meta.push_str(&format!("; source: {sid}"));
        }
        let body = body.trim();
        if body.len() <= MAX_INLINE_CHARS {
            let line = format!("- {} [{meta}]", body);
            self.append_bullet(name, &line)
        } else {
            let slug = slugify(name);
            let slug = if slug.is_empty() {
                "note".to_string()
            } else {
                slug
            };
            self.ensure_repo()?;
            self.ensure_clean()?;
            let path = self.root.join(format!("{slug}.md"));
            fs::write(&path, format!("{body}\n"))
                .with_context(|| format!("writing {}", path.display()))?;
            let line = format!(
                "- [[{slug}]] {} [{meta}]",
                if description.trim().is_empty() {
                    body.lines().next().unwrap_or(slug.as_str())
                } else {
                    description.trim()
                }
            );
            // The tree is dirty with the topic file on purpose; append the
            // index line and commit both together.
            self.append_bullet_in("Index", &line)?;
            self.stage_and_commit("remember")?;
            Ok(path)
        }
    }

    /// Bulk import from another memory (the block store, a CLAUDE.md): every
    /// fact lands as its own bullet or topic file, the whole batch as one
    /// commit so the history stays readable. Returns how many facts were
    /// written.
    pub fn import(&self, source: &str, entries: &[ImportEntry]) -> Result<usize> {
        self.ensure_repo()?;
        self.ensure_clean()?;
        let meta = format!("source: {source}; added: {}", Self::today());
        let mut written = 0;
        for entry in entries {
            let body = entry.body.trim();
            if body.is_empty() {
                continue;
            }
            if body.len() <= MAX_INLINE_CHARS {
                let line = format!("- {body} [{meta}]");
                self.append_bullet_in(entry.section.trim(), &line)?;
            } else {
                // The slug comes from the fact's own first line so two long
                // facts filed under one section cannot collide. Capped so a
                // long first line stays a legal file name.
                let mut base = slugify(body.lines().next().unwrap_or(entry.section.as_str()));
                base.truncate(64);
                while base.ends_with('-') {
                    base.pop();
                }
                let base = if base.is_empty() { "note".into() } else { base };
                let mut slug = base.clone();
                let mut n = 2;
                while self.root.join(format!("{slug}.md")).exists() {
                    slug = format!("{base}-{n}");
                    n += 1;
                }
                let path = self.root.join(format!("{slug}.md"));
                fs::write(&path, format!("{body}\n"))
                    .with_context(|| format!("writing {}", path.display()))?;
                let line = format!(
                    "- [[{slug}]] {} [{meta}]",
                    body.lines().next().unwrap_or(slug.as_str())
                );
                self.append_bullet_in("Index", &line)?;
            }
            written += 1;
        }
        if written > 0 {
            self.stage_and_commit(&format!("import from {source}"))?;
        }
        Ok(written)
    }

    pub fn remember(&self, name: &str, description: &str, body: &str) -> Result<PathBuf> {
        self.remember_inner(name, description, body, None)
    }

    pub fn remember_sourced(
        &self,
        name: &str,
        description: &str,
        body: &str,
        source_session: &str,
    ) -> Result<PathBuf> {
        self.remember_inner(name, description, body, Some(source_session))
    }

    pub fn append_project(&self, fact: &str) -> Result<()> {
        self.append_project_inner(fact, None)
    }

    pub fn append_project_sourced(&self, fact: &str, source_session: &str) -> Result<()> {
        self.append_project_inner(fact, Some(source_session))
    }

    fn append_project_inner(&self, fact: &str, source_session: Option<&str>) -> Result<()> {
        let added = Self::today();
        let meta = match source_session {
            Some(sid) => format!("source: {sid}; added: {added}"),
            None => format!("added: {added}"),
        };
        self.append_bullet("Project", &format!("- {} [{meta}]", fact.trim()))?;
        Ok(())
    }

    pub fn load_context(&self) -> Result<String> {
        let blocks = self.list_recent()?;
        if blocks.is_empty() {
            return Ok(String::new());
        }
        let mut index: Vec<String> = blocks
            .iter()
            .map(|b| {
                if b.description.is_empty() {
                    format!("- {}", b.name)
                } else {
                    format!("- {}: {}", b.name, b.description)
                }
            })
            .collect();
        let mut total = index.join("\n").chars().count();
        while total > MAX_AMR_CONTEXT_CHARS && index.len() > 1 {
            index.pop();
            total = index.join("\n").chars().count();
        }
        Ok(format!(
            "## Memory\n\n### Recallable memory\nCall `recall(name)` to read any of these in full before relying on it:\n{}",
            index.join("\n")
        ))
    }

    fn topic_path(&self, name: &str) -> Option<PathBuf> {
        let slug = slugify(name);
        let direct = self.root.join(format!("{slug}.md"));
        direct.exists().then_some(direct)
    }

    pub fn read_block(&self, name: &str) -> Result<String> {
        self.peek_block_inner(name, true)
    }

    pub fn peek_block(&self, name: &str) -> Result<String> {
        self.peek_block_inner(name, false)
    }

    fn peek_block_inner(&self, name: &str, _journal: bool) -> Result<String> {
        if let Some(path) = self.topic_path(name) {
            let raw =
                fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
            return Ok(raw.trim().to_string());
        }
        let slug = slugify(name);
        for block in self.list()? {
            if block.name == slug || block.name.eq_ignore_ascii_case(name) {
                // Drop the [name: ...; added: ...] metadata tail.
                let body = block.description.split(" [").next().unwrap_or("");
                return Ok(body.trim_end().to_string());
            }
        }
        bail!("no memory block named {name:?}");
    }

    pub fn list(&self) -> Result<Vec<MemoryMeta>> {
        let mut blocks = Vec::new();
        let Ok(raw) = fs::read_to_string(self.index_path()) else {
            return Ok(blocks);
        };
        let mut section = String::new();
        for line in raw.lines() {
            let trimmed = line.trim();
            if let Some(heading) = trimmed.strip_prefix("## ") {
                section = slugify(heading);
                continue;
            }
            let Some(line) = trimmed.strip_prefix("- ") else {
                continue;
            };
            let (name, description, path) = if let Some(rest) = line.strip_prefix("[[")
                && let Some((slug, tail)) = rest.split_once("]]")
            {
                (
                    slug.to_string(),
                    tail.trim().to_string(),
                    self.root.join(format!("{slug}.md")),
                )
            } else {
                let name = if section.is_empty() {
                    "note".to_string()
                } else {
                    section.clone()
                };
                (
                    name.clone(),
                    line.to_string(),
                    self.root.join(format!("{name}.md")),
                )
            };
            blocks.push(MemoryMeta {
                name,
                description,
                path,
                source_session: None,
                created_at: None,
                updated_at: None,
            });
        }
        blocks.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(blocks)
    }

    pub fn list_recent(&self) -> Result<Vec<MemoryMeta>> {
        // The index is already newest-last by append order; keep it as written.
        self.list()
    }

    fn remove_bullet(&self, name: &str) -> Result<Option<(String, Option<PathBuf>)>> {
        let slug = slugify(name);
        let path = self.index_path();
        let raw =
            fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        let mut kept = Vec::new();
        let mut removed = None;
        let mut section = String::new();
        for line in raw.lines() {
            let trimmed = line.trim();
            if let Some(heading) = trimmed.strip_prefix("## ") {
                section = slugify(heading);
                kept.push(line);
                continue;
            }
            let bullet = trimmed.strip_prefix("- ").map(str::trim);
            let is_target = match bullet {
                Some(b) => {
                    b.starts_with(&format!("[[{slug}]]")) || (!slug.is_empty() && section == slug)
                }
                None => false,
            };
            if removed.is_none() && is_target {
                removed = Some(line.to_string());
            } else {
                kept.push(line);
            }
        }
        let Some(line) = removed else {
            return Ok(None);
        };
        fs::write(&path, format!("{}\n", kept.join("\n")))
            .with_context(|| format!("writing {}", path.display()))?;
        let topic = self.root.join(format!("{slug}.md"));
        let topic = topic.exists().then_some(topic);
        Ok(Some((line, topic)))
    }

    pub fn forget(&self, name: &str) -> Result<bool> {
        self.retire(name, false)
    }

    pub fn archive(&self, name: &str) -> Result<bool> {
        self.retire(name, true)
    }

    fn retire(&self, name: &str, keep_file: bool) -> Result<bool> {
        self.ensure_repo()?;
        self.ensure_clean()?;
        let Some((_, topic)) = self.remove_bullet(name)? else {
            // A topic file without an index line is still a block.
            if let Some(path) = self.topic_path(name) {
                if keep_file {
                    self.move_to_archive(&path)?;
                } else {
                    fs::remove_file(&path)
                        .with_context(|| format!("removing {}", path.display()))?;
                }
                self.stage_and_commit("forget")?;
                return Ok(true);
            }
            return Ok(false);
        };
        if let Some(path) = topic {
            if keep_file {
                self.move_to_archive(&path)?;
            } else {
                fs::remove_file(&path).with_context(|| format!("removing {}", path.display()))?;
            }
        }
        self.stage_and_commit(if keep_file { "archive" } else { "forget" })?;
        Ok(true)
    }

    fn move_to_archive(&self, path: &Path) -> Result<()> {
        let archive_dir = self.root.join(".archive");
        fs::create_dir_all(&archive_dir)
            .with_context(|| format!("creating {}", archive_dir.display()))?;
        let dest = archive_dir.join(
            path.file_name()
                .with_context(|| format!("unexpected block path {}", path.display()))?,
        );
        fs::rename(path, &dest)
            .with_context(|| format!("archiving {} to {}", path.display(), dest.display()))
    }

    /// The `## Project` section of the index, `None` when nothing has been
    /// appended to it.
    pub fn project_text(&self) -> Option<String> {
        let raw = fs::read_to_string(self.index_path()).ok()?;
        let at = raw.find("## Project")?;
        let section: String = raw[at + "## Project".len()..]
            .lines()
            .take_while(|l| !l.starts_with("## "))
            .filter(|l| l.trim_start().starts_with("- "))
            .map(|l| format!("{l}\n"))
            .collect();
        let text = section.trim().to_string();
        (!text.is_empty()).then_some(text)
    }
}

/// The memory backend a session uses: the default block store, or the agent
/// memory repo when enabled. The journal and consolidation markers always stay
/// in the block store, so the audit trail lives in one place either way.
pub enum Memory {
    Blocks(MemoryStore),
    AgentRepo(AmrStore),
}

impl Memory {
    pub fn blocks(store: MemoryStore) -> Self {
        Self::Blocks(store)
    }

    pub fn agent_repo(root: PathBuf) -> Self {
        Self::AgentRepo(AmrStore::new(root))
    }

    pub fn journal(&self) -> Result<Vec<crate::memory::MemoryJournalEntry>> {
        match self {
            Self::Blocks(m) => m.journal(),
            Self::AgentRepo(_) => Ok(Vec::new()),
        }
    }

    pub fn record_consolidated(&self, session_id: &str) -> Result<()> {
        match self {
            Self::Blocks(m) => m.record_consolidated(session_id),
            Self::AgentRepo(_) => Ok(()),
        }
    }

    pub fn near_duplicates(&self, name: &str, description: &str) -> Vec<String> {
        let blocks = self.list().unwrap_or_default();
        crate::memory::near_duplicates_in(&blocks, name, description)
    }
}

#[cfg(test)]
#[path = "amr_tests.rs"]
mod tests;

macro_rules! delegate {
    ($self:expr, $m:ident, $body:expr) => {
        match $self {
            Memory::Blocks($m) => $body,
            Memory::AgentRepo($m) => $body,
        }
    };
}

impl Memory {
    pub fn load_context(&self) -> Result<String> {
        delegate!(self, m, m.load_context())
    }

    pub fn remember(&self, name: &str, description: &str, body: &str) -> Result<PathBuf> {
        delegate!(self, m, m.remember(name, description, body))
    }

    pub fn remember_sourced(
        &self,
        name: &str,
        description: &str,
        body: &str,
        source_session: &str,
    ) -> Result<PathBuf> {
        delegate!(
            self,
            m,
            m.remember_sourced(name, description, body, source_session)
        )
    }

    pub fn append_project(&self, fact: &str) -> Result<()> {
        delegate!(self, m, m.append_project(fact))
    }

    pub fn append_project_sourced(&self, fact: &str, source_session: &str) -> Result<()> {
        delegate!(self, m, m.append_project_sourced(fact, source_session))
    }

    pub fn read_block(&self, name: &str) -> Result<String> {
        delegate!(self, m, m.read_block(name))
    }

    pub fn peek_block(&self, name: &str) -> Result<String> {
        delegate!(self, m, m.peek_block(name))
    }

    pub fn list(&self) -> Result<Vec<MemoryMeta>> {
        delegate!(self, m, m.list())
    }

    pub fn list_recent(&self) -> Result<Vec<MemoryMeta>> {
        delegate!(self, m, m.list_recent())
    }

    pub fn forget(&self, name: &str) -> Result<bool> {
        delegate!(self, m, m.forget(name))
    }

    pub fn archive(&self, name: &str) -> Result<bool> {
        delegate!(self, m, m.archive(name))
    }

    pub fn project_text(&self) -> Option<String> {
        delegate!(self, m, m.project_text())
    }

    pub fn dir(&self) -> &Path {
        delegate!(self, m, m.dir())
    }
}
