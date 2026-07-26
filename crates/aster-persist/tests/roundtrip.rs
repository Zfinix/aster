use std::path::Path;

use aster_ai::{ToolCall, ToolCallFunction};
use aster_persist::{MessageEvent, Store, TranscriptEvent};

fn tool_call(id: &str, name: &str, args: &str) -> ToolCall {
    ToolCall {
        id: id.to_string(),
        kind: "function".to_string(),
        function: ToolCallFunction {
            name: name.to_string(),
            arguments: args.to_string(),
        },
    }
}

#[test]
fn append_and_reload_preserves_full_fidelity() {
    let home = tempfile::tempdir().unwrap();
    let store = Store::open(home.path()).unwrap();
    let repo = Path::new("/tmp/example-repo");

    let id = {
        let mut writer = store
            .new_session(
                repo,
                Path::new("/tmp/example-repo"),
                Some("test-model".into()),
            )
            .unwrap();
        let id = writer.id().to_string();
        writer
            .append_message(MessageEvent::user("read main.rs"))
            .unwrap();
        writer
            .append_message(MessageEvent::assistant(
                None,
                vec![tool_call("call_1", "read_file", "{\"path\":\"main.rs\"}")],
            ))
            .unwrap();
        writer
            .append_message(MessageEvent::tool("call_1", "fn main() {}"))
            .unwrap();
        writer
            .append_message(MessageEvent::assistant(
                Some("It is the entrypoint.".into()),
                vec![],
            ))
            .unwrap();
        id
    };

    let transcript = store.resume(repo, &id).unwrap();
    assert_eq!(transcript.meta.model.as_deref(), Some("test-model"));

    let tool_calls: Vec<_> = transcript
        .events
        .iter()
        .filter_map(|e| match e {
            TranscriptEvent::Message(m) if !m.tool_calls.is_empty() => Some(m),
            _ => None,
        })
        .collect();
    assert_eq!(tool_calls.len(), 1);
    assert_eq!(tool_calls[0].tool_calls[0].function.name, "read_file");

    let has_tool_result = transcript.events.iter().any(
        |e| matches!(e, TranscriptEvent::Message(m) if m.tool_call_id.as_deref() == Some("call_1")),
    );
    assert!(has_tool_result);

    let chat = transcript.to_chat_messages();
    assert_eq!(chat.len(), 2);
    assert_eq!(chat[0].role, "user");
    assert_eq!(chat[0].content, "read main.rs");
    assert_eq!(chat[1].role, "assistant");
    assert_eq!(chat[1].content, "It is the entrypoint.");
}

#[test]
fn latest_returns_most_recent_session() {
    let home = tempfile::tempdir().unwrap();
    let store = Store::open(home.path()).unwrap();
    let repo = Path::new("/tmp/repo");

    let first = store
        .new_session(repo, repo, None)
        .unwrap()
        .id()
        .to_string();
    std::thread::sleep(std::time::Duration::from_millis(2));
    let second = store
        .new_session(repo, repo, None)
        .unwrap()
        .id()
        .to_string();

    assert_ne!(first, second);
    let latest = store.latest(repo).unwrap().unwrap();
    assert_eq!(latest.meta.id, second);
    assert_eq!(store.list_sessions(repo).unwrap().len(), 2);
}

#[test]
fn session_writer_for_opens_or_creates_by_id() {
    let home = tempfile::tempdir().unwrap();
    let store = Store::open(home.path()).unwrap();
    let repo = Path::new("/tmp/desktop-repo");

    {
        let mut w = store
            .session_writer_for(repo, "conv-1", repo, Some("m".into()))
            .unwrap();
        w.append_message(MessageEvent::user("first")).unwrap();
    }
    {
        let mut w = store
            .session_writer_for(repo, "conv-1", repo, Some("m".into()))
            .unwrap();
        w.append_message(MessageEvent::user("second")).unwrap();
    }

    let transcript = store.resume(repo, "conv-1").unwrap();
    assert_eq!(transcript.user_turn_count(), 2);
    let headers = transcript
        .events
        .iter()
        .filter(|e| matches!(e, TranscriptEvent::Session(_)))
        .count();
    assert_eq!(headers, 1, "reopening must not write a second header");
    assert_eq!(store.list_sessions(repo).unwrap().len(), 1);
}

#[test]
fn latest_is_none_for_unknown_repo() {
    let home = tempfile::tempdir().unwrap();
    let store = Store::open(home.path()).unwrap();
    assert!(store.latest(Path::new("/tmp/nope")).unwrap().is_none());
}

#[test]
fn memory_blocks_and_project_feed_context() {
    let home = tempfile::tempdir().unwrap();
    let store = Store::open(home.path()).unwrap();
    let memory = store.memory();

    assert_eq!(memory.load_context().unwrap(), "");

    memory
        .remember(
            "Build command",
            "how to build",
            "Run cargo build -p aster-cli",
        )
        .unwrap();
    memory
        .append_project("Uses filesystem-first persistence")
        .unwrap();

    let ctx = memory.load_context().unwrap();
    assert!(ctx.contains("## Memory"));
    assert!(ctx.contains("Uses filesystem-first persistence"));
    assert!(ctx.contains("Recallable memory"));
    assert!(ctx.contains("build-command"));
    assert!(ctx.contains("how to build"));
    assert!(!ctx.contains("Run cargo build -p aster-cli"));
    assert_eq!(memory.list().unwrap().len(), 1);

    let block = memory.read_block("Build command").unwrap();
    assert_eq!(block, "Run cargo build -p aster-cli");
    assert!(memory.read_block("nonexistent").is_err());
}
