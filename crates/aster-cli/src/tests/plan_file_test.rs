use std::sync::{Arc, Mutex};

use aster_persist::Store;

use super::*;

fn saved_ctx(home: &Path, repo: &Path) -> SessionCtx {
    let store = Store::open(home).unwrap();
    let writer = store.new_session(repo, repo, None, None).unwrap();
    SessionCtx {
        recorder: Some(Arc::new(Mutex::new(writer))),
        store: Some(store),
        ..SessionCtx::default()
    }
}

#[test]
fn a_draft_is_saved_to_the_session_plan_file() {
    let home = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    let ctx = saved_ctx(home.path(), repo.path());

    let out = write_plan(&ctx, repo.path(), Some("## Context\n\ndraft"), None, None).unwrap();

    let path = plan_path(&ctx, repo.path()).unwrap();
    assert!(
        path.starts_with(home.path().join("plans")),
        "{}",
        path.display()
    );
    assert!(out.contains(&path.display().to_string()), "{out}");
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "## Context\n\ndraft"
    );
}

#[test]
fn a_section_is_revised_in_place() {
    let repo = tempfile::tempdir().unwrap();
    let ctx = SessionCtx::default();
    write_plan(
        &ctx,
        repo.path(),
        Some("## Context\n\nTODO\n\n## Verification\n\nTODO later"),
        None,
        None,
    )
    .unwrap();

    write_plan(
        &ctx,
        repo.path(),
        None,
        Some("TODO\n\n##"),
        Some("`chat.rs:42` drops it\n\n##"),
    )
    .unwrap();

    assert_eq!(
        read_plan(&ctx, repo.path()),
        "## Context\n\n`chat.rs:42` drops it\n\n## Verification\n\nTODO later"
    );
}

#[test]
fn an_edit_that_matches_twice_is_refused() {
    let repo = tempfile::tempdir().unwrap();
    let ctx = SessionCtx::default();
    write_plan(&ctx, repo.path(), Some("TODO\nTODO"), None, None).unwrap();

    let err = write_plan(&ctx, repo.path(), None, Some("TODO"), Some("done")).unwrap_err();

    assert!(err.to_string().contains("2 times"), "{err}");
    assert_eq!(read_plan(&ctx, repo.path()), "TODO\nTODO");
}

#[test]
fn a_missed_edit_hands_back_the_plan() {
    let repo = tempfile::tempdir().unwrap();
    let ctx = SessionCtx::default();
    write_plan(
        &ctx,
        repo.path(),
        Some("## Context\n\nreal text"),
        None,
        None,
    )
    .unwrap();

    let err = write_plan(&ctx, repo.path(), None, Some("imagined text"), Some("x")).unwrap_err();

    assert!(err.to_string().contains("real text"), "{err}");
}

#[test]
fn an_edit_before_any_draft_is_refused() {
    let repo = tempfile::tempdir().unwrap();
    let err = write_plan(
        &SessionCtx::default(),
        repo.path(),
        None,
        Some("a"),
        Some("b"),
    )
    .unwrap_err();
    assert!(err.to_string().contains("no plan yet"), "{err}");
}

#[test]
fn the_file_wins_over_memory() {
    let home = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    let ctx = saved_ctx(home.path(), repo.path());
    write_plan(&ctx, repo.path(), Some("mine"), None, None).unwrap();

    std::fs::write(plan_path(&ctx, repo.path()).unwrap(), "edited by the user").unwrap();

    assert_eq!(read_plan(&ctx, repo.path()), "edited by the user");
}

#[test]
fn rewriting_an_approved_plan_needs_approval_again() {
    let repo = tempfile::tempdir().unwrap();
    let ctx = SessionCtx::default();
    write_plan(&ctx, repo.path(), Some("v1"), None, None).unwrap();
    ctx.plan.lock().unwrap().approved = true;

    write_plan(&ctx, repo.path(), Some("v2"), None, None).unwrap();

    assert!(!ctx.plan.lock().unwrap().approved);
}
