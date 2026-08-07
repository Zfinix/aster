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
