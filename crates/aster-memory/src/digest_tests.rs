use aster_persist::{MessageEvent, SessionMeta, SessionTranscript, TranscriptEvent};

use super::{MAX_DIGEST_CHARS, build, extract_durable};

fn ts() -> chrono::DateTime<chrono::Utc> {
    chrono::Utc::now()
}

fn msg(role: &str, content: Option<&str>) -> MessageEvent {
    MessageEvent {
        role: role.to_string(),
        content: content.map(str::to_string),
        tool_calls: vec![],
        tool_call_id: None,
        ts: ts(),
        usage: None,
        annotations: vec![],
        reasoning: None,
    }
}

fn transcript(events: Vec<MessageEvent>) -> SessionTranscript {
    let mut transcript_events: Vec<TranscriptEvent> =
        events.into_iter().map(TranscriptEvent::Message).collect();
    transcript_events.insert(
        0,
        TranscriptEvent::Session(SessionMeta {
            id: "session-1".to_string(),
            v: 1,
            created_at: ts(),
            cwd: "/tmp/repo".to_string(),
            repo_root: "/tmp/repo".to_string(),
            model: Some("mock".to_string()),
            aster_version: None,
            title: Some("A real session".to_string()),
            schedule: None,
        }),
    );
    SessionTranscript {
        meta: SessionMeta {
            id: "session-1".to_string(),
            v: 1,
            created_at: ts(),
            cwd: "/tmp/repo".to_string(),
            repo_root: "/tmp/repo".to_string(),
            model: Some("mock".to_string()),
            aster_version: None,
            title: Some("A real session".to_string()),
            schedule: None,
        },
        events: transcript_events,
    }
}

#[test]
fn empty_session_builds_no_digest() {
    let t = transcript(vec![]);
    assert!(build(&t).is_none());
}

#[test]
fn user_only_session_with_no_content_builds_no_digest() {
    let t = transcript(vec![msg("user", None), msg("user", None)]);
    assert!(build(&t).is_none());
}

#[test]
fn durable_messages_are_kept_in_order() {
    let t = transcript(vec![
        msg("user", Some("Add a retry helper")),
        msg("assistant", Some("Here is the retry helper")),
        msg("tool", Some("ran the tests: ok")),
    ]);
    let digest = build(&t).expect("digest");
    assert_eq!(digest.messages.len(), 3);
    assert_eq!(digest.messages[0].role, "user");
    assert_eq!(digest.messages[1].content, "Here is the retry helper");
    assert_eq!(digest.user_turn_count, 1);
    assert_eq!(digest.title.as_deref(), Some("A real session"));
}

#[test]
fn tool_result_is_truncated_to_a_bounded_slice() {
    let long = "x".repeat(10_000);
    let t = transcript(vec![msg("user", Some("go")), msg("tool", Some(&long))]);
    let digest = build(&t).expect("digest");
    let tool = digest
        .messages
        .iter()
        .find(|m| m.role == "tool")
        .expect("tool message kept");
    assert!(tool.content.len() <= 600, "tool result must be truncated");
}

#[test]
fn oversized_digest_drops_oldest_messages_only() {
    // Each user message is a third of the budget, so three of them overflow and
    // only the newest two survive.
    let chunk = "y".repeat(MAX_DIGEST_CHARS / 2 + 1);
    let t = transcript(vec![
        msg("user", Some(&chunk)),
        msg("user", Some(&chunk)),
        msg("user", Some("short tail")),
    ]);
    let digest = build(&t).expect("digest");
    assert_eq!(digest.messages.len(), 2, "oldest oversized message dropped");
    assert_eq!(digest.messages[0].content, chunk);
    assert_eq!(digest.messages[1].content, "short tail");
}

#[test]
fn extract_durable_skips_nondurable_roles() {
    assert!(extract_durable(&msg("system", Some("be nice"))).is_none());
    assert!(extract_durable(&msg("tool", None)).is_none());
    assert!(extract_durable(&msg("tool", Some("   "))).is_none());
}
