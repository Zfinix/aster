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
