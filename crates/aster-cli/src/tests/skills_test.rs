use super::*;

#[test]
fn github_shorthand_becomes_clone_url() {
    assert_eq!(
        git_source("anthropics/skills"),
        Some(("https://github.com/anthropics/skills".into(), None))
    );
}

#[test]
fn shorthand_carries_subpath() {
    assert_eq!(
        git_source("anthropics/skills/document/pdf"),
        Some((
            "https://github.com/anthropics/skills".into(),
            Some("document/pdf".into())
        ))
    );
}

#[test]
fn full_urls_pass_through() {
    assert_eq!(
        git_source("https://github.com/a/b.git"),
        Some(("https://github.com/a/b.git".into(), None))
    );
    assert_eq!(
        git_source("git@github.com:a/b.git"),
        Some(("git@github.com:a/b.git".into(), None))
    );
}

#[test]
fn plain_words_are_not_git_sources() {
    assert_eq!(git_source("just-a-name"), None);
}

#[test]
fn source_detection() {
    assert!(looks_like_source("owner/repo"));
    assert!(looks_like_source("https://github.com/a/b"));
    assert!(!looks_like_source("pdf"));
}

#[test]
fn template_has_valid_frontmatter() {
    let t = skill_template("my-skill");
    assert!(t.starts_with("---\nname: my-skill\n"));
    assert!(t.contains("description:"));
}

#[test]
fn scope_defaults_to_global() {
    assert!(matches!(scope_of(false), Scope::Global));
    assert!(matches!(scope_of(true), Scope::Project));
}

#[test]
fn global_and_project_roots_are_distinct() {
    let global = scope_root(Scope::Global).unwrap();
    let project = scope_root(Scope::Project).unwrap();
    assert_ne!(global, project);
    assert!(project.ends_with(".aster/skills"), "{}", project.display());
    assert!(global.ends_with("skills"), "{}", global.display());
}

#[test]
fn the_other_scope_is_named_for_error_messages() {
    assert!(matches!(other_scope(Scope::Global), Scope::Project));
    assert!(matches!(other_scope(Scope::Project), Scope::Global));
    assert_eq!(other_scope_flag(Scope::Global), "--project");
    assert_eq!(other_scope_flag(Scope::Project), "--global");
}

fn skill_dir(root: &Path, name: &str, description: &str) {
    let dir = root.join(name);
    std::fs::create_dir_all(&dir).expect("skill dir");
    std::fs::write(
        dir.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: {description}\n---\n\nbody\n"),
    )
    .expect("SKILL.md");
}

#[test]
fn a_term_matches_whole_words_only() {
    assert!(mentions("rust coding guidelines", "rust"));
    assert!(mentions("write swift", "swift"));
    assert!(!mentions("going somewhere", "go"));
    assert!(!mentions("trustworthy", "rust"));
    assert!(mentions("expo/react-native", "react"));
}

#[test]
fn manifests_and_dependencies_become_terms() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().join("juice-mobile");
    std::fs::create_dir_all(&root).expect("root");
    std::fs::write(root.join("Cargo.toml"), "[package]\nname = \"x\"\n").expect("cargo");
    std::fs::write(
        root.join("package.json"),
        r#"{"dependencies":{"expo":"1","@heroui/react":"3"},"devDependencies":{"vitest":"1"}}"#,
    )
    .expect("package.json");

    let terms = repo_terms(&root);
    for want in [
        "rust", "cargo", "expo", "heroui", "react", "vitest", "juice", "mobile",
    ] {
        assert!(terms.contains(want), "missing {want}: {terms:?}");
    }
    // Words that would match half the catalog are dropped rather than ranked.
    for skip in ["app", "cli", "web", "node"] {
        assert!(!terms.contains(skip), "kept {skip}");
    }
}

#[test]
fn a_repos_own_stack_sorts_above_everything_else() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().to_path_buf();
    skill_dir(
        &root,
        "rust-guidelines",
        "House Rust style for cargo crates.",
    );
    skill_dir(&root, "academic-paper", "Write an academic paper.");
    skill_dir(&root, "zzz-last", "Nothing to do with this repo.");
    let skills: Vec<Skill> = SkillSet::discover(&[root])
        .visible()
        .cloned()
        .collect::<Vec<_>>();

    let mut terms = BTreeSet::new();
    terms.insert("rust".to_string());
    terms.insert("cargo".to_string());
    let (here, rest) = by_relevance(&skills, &terms);

    assert_eq!(
        here.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(),
        ["rust-guidelines"]
    );
    // The rest keeps a plain A-Z, so the tail stays scannable.
    assert_eq!(
        rest.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(),
        ["academic-paper", "zzz-last"]
    );
}

#[test]
fn nothing_relevant_leaves_the_listing_alphabetical() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().to_path_buf();
    skill_dir(&root, "beta", "Second.");
    skill_dir(&root, "alpha", "First.");
    let skills: Vec<Skill> = SkillSet::discover(&[root]).visible().cloned().collect();

    let (here, rest) = by_relevance(&skills, &BTreeSet::new());
    assert!(here.is_empty());
    assert_eq!(
        rest.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(),
        ["alpha", "beta"]
    );
}
