use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::slugify;

pub const PROJECT_MEMORY_FILE: &str = "ASTER.md";

pub struct MemoryStore {
    dir: PathBuf,
}

#[derive(Debug, Clone)]
pub struct MemoryMeta {
    pub name: String,
    pub description: String,
    pub path: PathBuf,
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

        let index: Vec<String> = self
            .list()?
            .into_iter()
            .map(|b| {
                if b.description.is_empty() {
                    format!("- {}", b.name)
                } else {
                    format!("- {} — {}", b.name, b.description)
                }
            })
            .collect();
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
        let slug = slugify(name);
        let direct = self.dir.join(format!("{slug}.md"));
        if let Ok(raw) = fs::read_to_string(&direct) {
            return Ok(strip_frontmatter(&raw).trim().to_string());
        }
        for block in self.list()? {
            if block.name.eq_ignore_ascii_case(name) || slugify(&block.name) == slug {
                let raw = fs::read_to_string(&block.path)
                    .with_context(|| format!("reading {}", block.path.display()))?;
                return Ok(strip_frontmatter(&raw).trim().to_string());
            }
        }
        anyhow::bail!("no memory block named {name:?}")
    }

    pub fn remember(&self, name: &str, description: &str, body: &str) -> Result<PathBuf> {
        fs::create_dir_all(&self.dir)
            .with_context(|| format!("creating {}", self.dir.display()))?;
        let slug = slugify(name);
        let slug = if slug.is_empty() {
            "note".to_string()
        } else {
            slug
        };
        let path = self.dir.join(format!("{slug}.md"));
        let contents = format!(
            "---\nname: {slug}\ndescription: {}\n---\n\n{}\n",
            description.trim(),
            body.trim(),
        );
        fs::write(&path, contents).with_context(|| format!("writing {}", path.display()))?;
        Ok(path)
    }

    pub fn append_project(&self, fact: &str) -> Result<()> {
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
            let (name, description) = frontmatter_fields(&raw);
            let name = name.unwrap_or_else(|| {
                path.file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("note")
                    .to_string()
            });
            blocks.push(MemoryMeta {
                name,
                description: description.unwrap_or_default(),
                path,
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

fn frontmatter_fields(raw: &str) -> (Option<String>, Option<String>) {
    let trimmed = raw.trim_start();
    let Some(rest) = trimmed.strip_prefix("---") else {
        return (None, None);
    };
    let Some(end) = rest.find("\n---") else {
        return (None, None);
    };
    let front = &rest[..end];
    let mut name = None;
    let mut description = None;
    for line in front.lines() {
        if let Some(v) = line.strip_prefix("name:") {
            name = Some(v.trim().to_string());
        } else if let Some(v) = line.strip_prefix("description:") {
            description = Some(v.trim().to_string());
        }
    }
    (name, description)
}
