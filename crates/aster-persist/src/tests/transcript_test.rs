use super::*;

fn writer(dir: &Path) -> SessionWriter {
    let meta = SessionMeta {
        id: "s1".into(),
        v: TRANSCRIPT_VERSION,
        created_at: Utc::now(),
        cwd: "/repo".into(),
        repo_root: "/repo".into(),
        model: None,
        aster_version: None,
        title: None,
    };
    SessionWriter::create(dir.join("s1.jsonl"), meta).unwrap()
}

#[test]
fn a_title_survives_a_reload() {
    let dir = tempfile::tempdir().unwrap();
    let mut w = writer(dir.path());
    w.append_message(MessageEvent::user("fix the sandbox"))
        .unwrap();
    w.set_title("Fix the sandbox seccomp filter").unwrap();

    let loaded = SessionTranscript::load(&dir.path().join("s1.jsonl")).unwrap();
    assert_eq!(loaded.title(), Some("Fix the sandbox seccomp filter"));
    assert_eq!(
        loaded.meta.title.as_deref(),
        Some("Fix the sandbox seccomp filter")
    );
}

#[test]
fn the_newest_title_wins() {
    let dir = tempfile::tempdir().unwrap();
    let mut w = writer(dir.path());
    w.set_title("First guess").unwrap();
    w.set_title("Better name").unwrap();

    let loaded = SessionTranscript::load(&dir.path().join("s1.jsonl")).unwrap();
    assert_eq!(loaded.title(), Some("Better name"));
}

#[test]
fn an_untitled_session_displays_its_opening_message() {
    let dir = tempfile::tempdir().unwrap();
    let mut w = writer(dir.path());
    w.append_message(MessageEvent::user("how does naming work"))
        .unwrap();

    let loaded = SessionTranscript::load(&dir.path().join("s1.jsonl")).unwrap();
    assert_eq!(loaded.display_title(), Some("how does naming work"));
}

/// The agent loop records its own steering (loop corrections, round budgets) as
/// `system` so it stays out of the replayed conversation. Replaying it would
/// re-nag the model about a turn that already ended, and show the user words
/// they never wrote.
#[test]
fn harness_steering_is_recorded_but_never_replayed() {
    let dir = tempfile::tempdir().unwrap();
    let mut w = writer(dir.path());
    w.append_message(MessageEvent::user("rename the label"))
        .unwrap();
    w.append_message(MessageEvent::system(
        "You have spent 10 tool rounds without editing a file.",
    ))
    .unwrap();
    w.append_message(MessageEvent::assistant(
        Some("Renamed it.".into()),
        Vec::new(),
    ))
    .unwrap();

    let loaded = SessionTranscript::load(&dir.path().join("s1.jsonl")).unwrap();
    // Kept on disk, so a session report can still show why the turn changed course.
    assert_eq!(loaded.messages().count(), 3);
    let replayed = loaded.to_chat_messages();
    assert_eq!(replayed.len(), 2);
    assert!(replayed.iter().all(|m| m.role != "system"));
    assert!(
        !replayed
            .iter()
            .any(|m| m.content.text().contains("10 tool rounds"))
    );
    // And it must not count as the user talking, which drives session naming.
    assert_eq!(loaded.user_turn_count(), 1);
}

/// Every session written before reasoning was recorded has no `reasoning` key.
/// A strict field would make all of them unreadable, not just reasoning-less.
#[test]
fn a_transcript_written_before_reasoning_still_loads() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("s1.jsonl");
    std::fs::write(
        &path,
        concat!(
            r#"{"type":"session","id":"s1","v":1,"created_at":"2026-01-01T00:00:00Z","cwd":"/repo","repo_root":"/repo"}"#,
            "\n",
            r#"{"type":"message","role":"user","content":"hi","ts":"2026-01-01T00:00:01Z"}"#,
            "\n",
            r#"{"type":"message","role":"assistant","content":"hey","ts":"2026-01-01T00:00:02Z"}"#,
            "\n",
        ),
    )
    .unwrap();

    let loaded = SessionTranscript::load(&path).unwrap();
    assert_eq!(loaded.to_chat_messages().len(), 2);
    assert!(loaded.messages().all(|m| m.reasoning.is_none()));
}

#[test]
fn reasoning_survives_a_reload() {
    let dir = tempfile::tempdir().unwrap();
    let mut w = writer(dir.path());
    w.append_message(
        MessageEvent::assistant(Some("Renamed it.".into()), Vec::new()).with_reasoning(Some(
            ReasoningRecord {
                text: "checking both call sites".into(),
                tokens: Some(40),
                duration_ms: Some(1200),
            },
        )),
    )
    .unwrap();

    let loaded = SessionTranscript::load(&dir.path().join("s1.jsonl")).unwrap();
    let reasoning = loaded.messages().next().unwrap().reasoning.clone().unwrap();
    assert_eq!(reasoning.text, "checking both call sites");
    assert_eq!(reasoning.tokens, Some(40));
    assert_eq!(reasoning.duration_ms, Some(1200));
}
