use aster_ai::AiClient;
use aster_persist::{MemoryOp, Store};

use super::{Proposals, already_consolidated, apply, parse_proposals, unconsolidated_sessions};

fn store() -> (tempfile::TempDir, aster_persist::MemoryStore) {
    let home = tempfile::tempdir().unwrap();
    let store = Store::open(home.path()).unwrap();
    let memory = store.memory();
    (home, memory)
}

#[test]
fn parse_proposals_accepts_plain_json() {
    let raw = r#"{"summary":"done","new":[{"name":"build-cmd","description":"how to build","body":"Run cargo build"}],"archive":[],"lessons":[]}"#;
    let proposals = parse_proposals(raw).unwrap();
    assert_eq!(proposals.new.len(), 1);
    assert_eq!(proposals.new[0].name, "build-cmd");
}

#[test]
fn parse_proposals_strips_markdown_fences() {
    let raw = "```json\n{\"summary\":\"x\",\"archive\":[\"old-fact\"]}\n```";
    let proposals = parse_proposals(raw).unwrap();
    assert_eq!(proposals.archive, vec!["old-fact"]);
}

#[test]
fn parse_proposals_rejects_invalid_json() {
    assert!(parse_proposals("not json").is_err());
    assert!(parse_proposals("{\"summary\":").is_err());
}

#[test]
fn apply_writes_new_blocks_sourced_and_marks_consolidated() {
    let (_home, memory) = store();
    let proposals = Proposals {
        new: vec![super::NewBlock {
            name: "build-cmd".into(),
            description: "how to build".into(),
            body: "Run cargo build".into(),
        }],
        ..Default::default()
    };
    let report = apply(&memory, "session-9", &proposals).unwrap();
    assert_eq!(report.wrote, vec!["build-cmd"]);

    let body = memory.read_block("build-cmd").unwrap();
    assert_eq!(body, "Run cargo build");

    let journal = memory.journal().unwrap();
    let sourced = journal
        .iter()
        .find(|e| e.op == MemoryOp::Remember && e.source_session.as_deref() == Some("session-9"));
    assert!(
        sourced.is_some(),
        "new block must be journaled to its session"
    );
    let marker = journal.iter().find(|e| {
        e.op == MemoryOp::Consolidated && e.source_session.as_deref() == Some("session-9")
    });
    assert!(marker.is_some(), "consolidation must be marked");
    assert!(already_consolidated(&memory, "session-9"));
}

#[test]
fn apply_archives_targets_and_merge_superseded_blocks() {
    let (_home, memory) = store();
    memory
        .remember("old-fact", "superseded", "the old way")
        .unwrap();
    memory
        .remember("other-old", "superseded too", "also stale")
        .unwrap();

    let proposals = Proposals {
        new: vec![super::NewBlock {
            name: "fresh-fact".into(),
            description: "the new way".into(),
            body: "Do it differently now".into(),
        }],
        archive: vec!["old-fact".into()],
        merge: vec![super::Merge {
            into_name: "fresh-fact".into(),
            archive: vec!["other-old".into()],
        }],
        ..Default::default()
    };
    let report = apply(&memory, "session-10", &proposals).unwrap();
    assert_eq!(report.wrote, vec!["fresh-fact"]);
    assert_eq!(report.archived.len(), 2);

    assert!(
        memory.read_block("old-fact").is_err(),
        "archived block is gone"
    );
    assert!(
        memory.read_block("other-old").is_err(),
        "merged block is gone"
    );
    assert!(
        memory.read_block("fresh-fact").is_ok(),
        "replacement block remains"
    );
}

#[test]
fn apply_records_consolidation_even_when_nothing_written() {
    let (_home, memory) = store();
    let report = apply(&memory, "session-11", &Proposals::default()).unwrap();
    assert!(report.wrote.is_empty());
    assert!(already_consolidated(&memory, "session-11"));
}

#[test]
fn unconsolidated_sessions_lists_only_unmarked_recent_writes() {
    let (_home, memory) = store();
    memory
        .remember_sourced("a", "d", "body", "session-a")
        .unwrap();
    memory
        .remember_sourced("b", "d", "body", "session-b")
        .unwrap();
    memory.record_consolidated("session-b").unwrap();

    // A recent window: both writes qualify, but session-b already consolidated.
    let recent = chrono::Utc::now() - chrono::Duration::days(1);
    let pending = unconsolidated_sessions(&memory, recent).unwrap();
    assert_eq!(pending, vec!["session-a"]);

    // A window entirely before both writes: nothing qualifies.
    let future = chrono::Utc::now() + chrono::Duration::days(1);
    let pending = unconsolidated_sessions(&memory, future).unwrap();
    assert!(pending.is_empty());
}

#[test]
fn remember_is_journaled_with_source() {
    let (_home, memory) = store();
    memory
        .remember_sourced("pref", "user pref", "always X", "session-c")
        .unwrap();
    let journal = memory.journal().unwrap();
    let entry = journal
        .iter()
        .find(|e| e.op == MemoryOp::Remember && e.name.as_deref() == Some("pref"))
        .expect("remember entry");
    assert_eq!(entry.source_session.as_deref(), Some("session-c"));
}

#[test]
fn under_gate_consolidation_is_skipped() {
    // The turn gate returns early before any model call, so a client pointed at
    // a dead port is never reached.
    let (_home, memory) = store();
    let transcript = aster_persist::SessionTranscript {
        meta: aster_persist::SessionMeta {
            id: "session-0".into(),
            v: 1,
            created_at: chrono::Utc::now(),
            cwd: "/tmp/repo".into(),
            repo_root: "/tmp/repo".into(),
            model: None,
            aster_version: None,
            title: None,
            schedule: None,
        },
        events: vec![],
    };
    let client = AiClient::new("http://127.0.0.1:1", "k", "mock-model");
    let result = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(super::consolidate_session(&client, &memory, &transcript, 6));
    assert!(matches!(result, Ok(None)));
}
