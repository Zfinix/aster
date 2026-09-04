use std::fs;

use crate::Store;
use crate::memory::{
    MAX_INDEX_ENTRIES, MemoryJournalEntry, MemoryOp, MemoryStore, PROJECT_MEMORY_FILE,
};

fn store(dir: &tempfile::TempDir) -> MemoryStore {
    Store::open(dir.path()).unwrap().memory()
}

#[test]
fn remember_stores_sourced_metadata_and_preserves_created_at_on_rewrite() {
    let dir = tempfile::tempdir().unwrap();
    let memory = store(&dir);

    let path = memory
        .remember_sourced(
            "Build command",
            "how to build",
            "Run cargo build",
            "session-1",
        )
        .unwrap();
    let raw = fs::read_to_string(&path).unwrap();
    assert!(raw.contains("source_session: session-1"));
    assert!(raw.contains("created_at:"));
    assert!(raw.contains("updated_at:"));

    let listed = memory.list().unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].name, "build-command");
    assert_eq!(listed[0].source_session.as_deref(), Some("session-1"));
    assert!(listed[0].created_at.is_some());
    let created = listed[0].created_at;

    memory
        .remember(
            "Build command",
            "how to build",
            "Run cargo build -p aster-cli",
        )
        .unwrap();
    let listed = memory.list().unwrap();
    assert_eq!(listed[0].source_session.as_deref(), Some("session-1"));
    assert_eq!(listed[0].created_at, created);
}

#[test]
fn journal_records_every_write_archive_and_recall() {
    let dir = tempfile::tempdir().unwrap();
    let memory = store(&dir);

    memory
        .remember_sourced("Pref", "user preference", "User likes emojis", "session-9")
        .unwrap();
    memory
        .append_project_sourced("Never use build: commits", "session-9")
        .unwrap();
    memory.read_block("Pref").unwrap();
    memory.archive("Pref").unwrap();
    assert!(memory.read_block("no-such-block").is_err());

    let raw = fs::read_to_string(memory.dir().join("journal.jsonl")).unwrap();
    let ops: Vec<MemoryJournalEntry> = raw
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();
    let op_names: Vec<MemoryOp> = ops.iter().map(|e| e.op).collect();
    assert_eq!(
        op_names,
        vec![
            MemoryOp::Remember,
            MemoryOp::AppendProject,
            MemoryOp::Recall,
            MemoryOp::Archive,
        ]
    );
    assert_eq!(ops[0].source_session.as_deref(), Some("session-9"));
    assert_eq!(ops[1].source_session.as_deref(), Some("session-9"));
}

#[test]
fn archive_hides_from_index_and_load_context() {
    let dir = tempfile::tempdir().unwrap();
    let memory = store(&dir);
    memory.remember("Stale", "old", "Superseded fact").unwrap();
    memory.append_project("Keep me").unwrap();

    assert!(memory.list().unwrap().iter().any(|b| b.name == "stale"));
    assert!(memory.load_context().unwrap().contains("stale"));

    assert!(memory.archive("Stale").unwrap());
    assert!(!memory.list().unwrap().iter().any(|b| b.name == "stale"));
    assert!(memory.dir().join(".archive/stale.md").exists());
    assert!(!memory.load_context().unwrap().contains("stale"));
    // The project feed is not a block: archiving a block never hides it.
    assert!(memory.load_context().unwrap().contains("Keep me"));
}

#[test]
fn index_is_capped_and_most_recent_first() {
    let dir = tempfile::tempdir().unwrap();
    let memory = store(&dir);
    for i in 0..(MAX_INDEX_ENTRIES + 10) {
        memory
            .remember(&format!("Block {i}"), "desc", "body")
            .unwrap();
    }
    let ctx = memory.load_context().unwrap();
    assert_eq!(memory.list().unwrap().len(), MAX_INDEX_ENTRIES + 10);
    assert!(!ctx.contains("block-0"));
    assert!(ctx.contains("more blocks"));
    assert!(ctx.contains(&format!("block-{}", MAX_INDEX_ENTRIES + 9)));
}

#[test]
fn append_project_sourced_records_provenance() {
    let dir = tempfile::tempdir().unwrap();
    let memory = store(&dir);
    memory
        .append_project_sourced("Conventional commits only", "session-2")
        .unwrap();
    let project = fs::read_to_string(memory.dir().join(PROJECT_MEMORY_FILE)).unwrap();
    assert!(project.contains("Conventional commits only"));
    let raw = fs::read_to_string(memory.dir().join("journal.jsonl")).unwrap();
    assert!(raw.contains("session-2"));
}

#[test]
fn legacy_blocks_without_frontmatter_still_list() {
    let dir = tempfile::tempdir().unwrap();
    let memory = store(&dir);
    fs::create_dir_all(memory.dir()).unwrap();
    fs::write(
        memory.dir().join("legacy.md"),
        "# Legacy\n\nNo frontmatter here",
    )
    .unwrap();
    let listed = memory.list().unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].name, "legacy");
    assert!(listed[0].source_session.is_none());
    assert!(listed[0].created_at.is_none());
}
