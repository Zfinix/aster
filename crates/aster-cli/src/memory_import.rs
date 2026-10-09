//! `aster memory import`: copy memory into the agent memory repo from the
//! block store, Claude Code's CLAUDE.md, or any markdown memory file.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::Args;

use aster_persist::ImportEntry;

const CLAUDE_MEMORY: &str = ".claude/CLAUDE.md";

#[derive(Debug, Args)]
pub(crate) struct MemoryImportArgs {
    /// Where to read from: `aster` (the block store), `claude`
    /// (~/.claude/CLAUDE.md), or a path to a markdown memory file.
    pub source: String,
    /// Report what would be imported without writing anything.
    #[arg(long)]
    pub dry_run: bool,
}

pub(crate) fn run(args: MemoryImportArgs) -> Result<()> {
    let (source, entries) = collect(&args.source)?;
    if entries.is_empty() {
        println!("nothing to import from {source}");
        return Ok(());
    }
    if args.dry_run {
        println!("would import {} facts from {source}:", entries.len());
        for entry in &entries {
            let first = entry.body.lines().next().unwrap_or("").trim();
            println!("  [{}] {first}", entry.section);
        }
        return Ok(());
    }
    let store = aster_persist::AmrStore::new(crate::persist::amr_path()?);
    let written = store.import(&source, &entries)?;
    println!(
        "imported {written} facts into {} (from {source})",
        store.dir().display()
    );
    Ok(())
}

fn collect(source: &str) -> Result<(String, Vec<ImportEntry>)> {
    match source.to_ascii_lowercase().as_str() {
        "aster" => Ok(("aster".into(), from_aster()?)),
        "claude" => {
            let path = dirs::home_dir()
                .context("could not determine home directory")?
                .join(CLAUDE_MEMORY);
            from_markdown("claude", &path)
        }
        _ => from_markdown("markdown", &PathBuf::from(source)),
    }
}

fn from_aster() -> Result<Vec<ImportEntry>> {
    let memory = crate::persist::memory()?;
    let mut entries = Vec::new();
    if let Some(project) = memory.project_text() {
        for line in project.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let fact = strip_meta(trimmed.trim_start_matches("- ").trim());
            if !fact.is_empty() {
                entries.push(ImportEntry {
                    section: "Project".into(),
                    body: fact.to_string(),
                });
            }
        }
    }
    for block in memory.list()? {
        entries.push(ImportEntry {
            section: block.name.clone(),
            body: memory.read_block(&block.name)?,
        });
    }
    Ok(entries)
}

fn from_markdown(source: &str, path: &Path) -> Result<(String, Vec<ImportEntry>)> {
    let raw = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    Ok((source.into(), parse_markdown(&raw)))
}

/// Sections and bullets from a CLAUDE.md-style memory file: `## Heading`
/// starts a section, `- fact` lines are facts, everything else is ignored.
fn parse_markdown(raw: &str) -> Vec<ImportEntry> {
    let mut entries = Vec::new();
    let mut section = String::from("Notes");
    for line in raw.lines() {
        let trimmed = line.trim();
        if let Some(heading) = trimmed.strip_prefix("## ") {
            section = heading.trim().to_string();
        } else if let Some(fact) = trimmed.strip_prefix("- ") {
            let fact = strip_meta(fact.trim());
            if !fact.is_empty() {
                entries.push(ImportEntry {
                    section: section.clone(),
                    body: fact.to_string(),
                });
            }
        }
    }
    entries
}

/// Drop a trailing `[source: ...; added: ...]` group so an import does not
/// stack its own metadata on top of the old one.
fn strip_meta(fact: &str) -> &str {
    if fact.ends_with(']') {
        fact.rfind(" [")
            .map(|at| fact[..at].trim_end())
            .unwrap_or(fact)
    } else {
        fact
    }
}

#[cfg(test)]
#[path = "tests/memory_import_test.rs"]
mod tests;
