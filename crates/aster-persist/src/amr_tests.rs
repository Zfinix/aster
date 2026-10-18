use super::*;
use crate::Store;

fn store() -> (tempfile::TempDir, AmrStore) {
    let dir = tempfile::tempdir().unwrap();
    let s = AmrStore::new(dir.path().join("agent-memory"));
    s.ensure_repo().unwrap();
    (dir, s)
}

#[test]
fn remember_writes_a_bullet_and_commits() {
    let (_dir, s) = store();
    s.remember("editor", "prefers bun", "uses bun, never npm")
        .unwrap();
    let raw = std::fs::read_to_string(s.dir().join(MEMORY_FILE)).unwrap();
    assert!(raw.contains("uses bun, never npm"));
    assert!(raw.contains("[added:"));
    let log = s.git_output(&["log", "--oneline"]).unwrap();
    assert_eq!(log.lines().count(), 2, "init commit plus remember commit");
}

#[test]
fn long_facts_get_a_topic_file_and_an_index_line() {
    let (_dir, s) = store();
    let body = "x".repeat(MAX_INLINE_CHARS + 1);
    let path = s
        .remember("conventions", "repo conventions", &body)
        .unwrap();
    assert_eq!(path, s.dir().join("conventions.md"));
    let raw = std::fs::read_to_string(s.dir().join(MEMORY_FILE)).unwrap();
    assert!(raw.contains("[[conventions]]"));
    assert_eq!(s.read_block("conventions").unwrap(), body);
}

#[test]
fn read_block_finds_a_plain_bullet_by_name() {
    let (_dir, s) = store();
    s.remember("editor", "prefers bun", "uses bun, never npm")
        .unwrap();
    assert_eq!(s.read_block("editor").unwrap(), "uses bun, never npm");
}

#[test]
fn forget_removes_the_bullet_and_commits() {
    let (_dir, s) = store();
    s.remember("editor", "prefers bun", "uses bun, never npm")
        .unwrap();
    assert!(s.forget("editor").unwrap());
    let raw = std::fs::read_to_string(s.dir().join(MEMORY_FILE)).unwrap();
    assert!(!raw.contains("uses bun, never npm"));
    assert!(!s.forget("editor").unwrap());
}

#[test]
fn a_dirty_tree_refuses_the_write() {
    let (_dir, s) = store();
    std::fs::write(s.dir().join(MEMORY_FILE), "# Memory\n\nhand edit\n").unwrap();
    let err = s.remember("editor", "d", "fact").unwrap_err();
    assert!(err.to_string().contains("uncommitted changes"));
}

#[test]
fn project_facts_live_in_their_own_section() {
    let (_dir, s) = store();
    s.append_project("the repo is aster").unwrap();
    let added = chrono::Utc::now().format("%Y-%m-%d");
    assert_eq!(
        s.project_text().unwrap(),
        format!("- the repo is aster [added: {added}]")
    );
    assert!(
        s.list()
            .unwrap()
            .iter()
            .any(|b| b.description.contains("aster"))
    );
}

#[test]
fn context_is_capped() {
    let (_dir, s) = store();
    // Write the index directly: a commit per fact would make this test crawl.
    let index: String = (0..2000)
        .map(|i| format!("- fact number {i} [added: 2026-10-09]\n"))
        .collect();
    std::fs::write(s.dir().join(MEMORY_FILE), format!("# Memory\n\n{index}")).unwrap();
    let ctx = s.load_context().unwrap();
    assert!(ctx.chars().count() <= MAX_AMR_CONTEXT_CHARS + 200);
}

#[test]
fn import_lands_many_facts_in_one_commit() {
    let (_dir, s) = store();
    let n = s
        .import(
            "claude",
            &[
                ImportEntry {
                    section: "Editor".into(),
                    body: "uses bun".into(),
                },
                ImportEntry {
                    section: "Editor".into(),
                    body: "never npm".into(),
                },
            ],
        )
        .unwrap();
    assert_eq!(n, 2);
    let raw = std::fs::read_to_string(s.dir().join(MEMORY_FILE)).unwrap();
    assert!(raw.contains("uses bun"));
    assert!(raw.contains("[source: claude;"));
    let log = s.git_output(&["log", "--oneline"]).unwrap();
    assert_eq!(log.lines().count(), 2, "init commit plus one import commit");
}

#[test]
fn import_refuses_a_dirty_tree() {
    let (_dir, s) = store();
    std::fs::write(s.dir().join(MEMORY_FILE), "# Memory\n\nhand edit\n").unwrap();
    let err = s
        .import(
            "claude",
            &[ImportEntry {
                section: "A".into(),
                body: "b".into(),
            }],
        )
        .unwrap_err();
    assert!(err.to_string().contains("uncommitted changes"));
}

#[test]
fn long_imported_facts_get_unique_topic_files() {
    let (_dir, s) = store();
    let long = |tag: &str| format!("{tag} {}", "x".repeat(MAX_INLINE_CHARS + 1));
    let n = s
        .import(
            "aster",
            &[
                ImportEntry {
                    section: "Conventions".into(),
                    body: long("first"),
                },
                ImportEntry {
                    section: "Conventions".into(),
                    body: long("second"),
                },
            ],
        )
        .unwrap();
    assert_eq!(n, 2);
    let mut files: Vec<String> = std::fs::read_dir(s.dir())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".md") && name != MEMORY_FILE)
        .collect();
    files.sort();
    assert_eq!(files.len(), 2, "one topic file per long fact: {files:?}");
    let raw = std::fs::read_to_string(s.dir().join(MEMORY_FILE)).unwrap();
    assert!(raw.contains("[[first-"));
    assert!(raw.contains("[[second-"));
}

#[test]
fn memory_with_none_stays_on_the_block_store() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();
    assert!(matches!(store.memory_with(None), Memory::Blocks(_)));
    assert!(matches!(
        store.memory_with(Some(dir.path().join("am"))),
        Memory::AgentRepo(_)
    ));
}
