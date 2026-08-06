use super::*;
use std::collections::HashSet;
use tempfile::tempdir;

#[test]
fn keys_and_display_names_are_unique() {
    let mut keys = HashSet::new();
    let mut names = HashSet::new();
    for agent in AGENTS {
        assert!(keys.insert(agent.key), "duplicate key {}", agent.key);
        assert!(
            names.insert(agent.display_name),
            "duplicate display name {}",
            agent.display_name
        );
    }
}

#[test]
fn project_dirs_are_relative_and_never_aster_own_root() {
    for agent in AGENTS {
        let dir = Path::new(agent.project_dir);
        assert!(dir.is_relative(), "{} is absolute", agent.key);
        assert_ne!(agent.project_dir, ".aster/skills", "{}", agent.key);
    }
}

#[test]
fn agent_by_key_is_case_insensitive_and_trims() {
    assert_eq!(agent_by_key("  Claude-Code ").unwrap().key, "claude-code");
    assert!(agent_by_key("not-an-agent").is_none());
}

#[test]
fn global_roots_are_absolute_and_end_in_a_skills_dir() {
    for agent in AGENTS {
        let Some(dir) = agent.global_dir() else {
            continue;
        };
        assert!(dir.is_absolute(), "{} is relative", agent.key);
        assert!(
            dir.ends_with("skills"),
            "{} is {}",
            agent.key,
            dir.display()
        );
    }
}

#[test]
fn project_only_agents_have_no_global_root() {
    assert!(agent_by_key("eve").unwrap().global_dir().is_none());
    assert!(agent_by_key("promptscript").unwrap().global_dir().is_none());
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
