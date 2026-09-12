use super::*;

fn store() -> (tempfile::TempDir, Store) {
    let home = tempfile::tempdir().unwrap();
    let store = Store::open(home.path()).unwrap();
    (home, store)
}

#[test]
fn a_new_session_resumes_under_the_id_it_reports() {
    let (_home, store) = store();
    let repo = Path::new("/repo");
    let id = store
        .new_session(repo, repo, None, None)
        .unwrap()
        .meta()
        .id
        .clone();
    assert!(store.resume(repo, &id).is_ok());
    assert!(store.resume_writer(repo, &id).is_ok());
}

#[test]
fn a_session_resumes_whatever_case_the_id_is_given_in() {
    let (_home, store) = store();
    let repo = Path::new("/repo");
    let id = "01M297K092KEHYQPYACBZF35S0";
    store
        .session_writer_for(repo, id, repo, None, None)
        .unwrap();
    assert!(store.resume(repo, id).is_ok());
    assert!(store.resume(repo, &id.to_ascii_lowercase()).is_ok());
}

#[test]
fn a_session_written_before_ids_were_normalised_is_still_found() {
    let (_home, store) = store();
    let repo = Path::new("/repo");
    let id = "01M2A77EZ8A6CN8FES7VC0XKFA";
    let dir = store.sessions_dir(repo);
    std::fs::create_dir_all(&dir).unwrap();
    let meta = SessionMeta {
        id: id.into(),
        v: TRANSCRIPT_VERSION,
        created_at: Utc::now(),
        cwd: "/repo".into(),
        repo_root: "/repo".into(),
        model: None,
        base_url: None,
        aster_version: None,
        title: None,
        schedule: None,
    };
    SessionWriter::create(dir.join(format!("{id}.jsonl")), meta).unwrap();
    assert!(store.resume(repo, id).is_ok());
}
