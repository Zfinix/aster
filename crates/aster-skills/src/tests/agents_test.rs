use super::*;
use tempfile::tempdir;

#[test]
fn agent_by_key_is_case_insensitive_and_trims() {
    assert_eq!(agent_by_key("  Claude-Code ").unwrap().key, "claude-code");
    assert!(agent_by_key("not-an-agent").is_none());
}

#[test]
fn existing_roots_finds_the_project_dir_and_skips_missing_ones() {
    let repo = tempdir().unwrap();
    let agent = agent_by_key("cursor").unwrap();
    std::fs::create_dir_all(agent.project_dir_in(repo.path())).unwrap();

    let roots = agent.existing_roots(Some(repo.path()));

    assert!(roots.contains(&agent.project_dir_in(repo.path())));

    let absent = agent_by_key("kode").unwrap();
    assert!(
        !absent
            .existing_roots(Some(repo.path()))
            .contains(&absent.project_dir_in(repo.path()))
    );
}
