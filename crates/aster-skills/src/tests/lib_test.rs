use super::*;

fn write_skill(root: &Path, name: &str, body: &str) {
    let dir = root.join(name);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join(SKILL_FILE), body).unwrap();
}

#[test]
fn builtins_load_with_parseable_bodies() {
    let set = SkillSet::default().with_builtins();
    assert_eq!(set.len(), BUILTIN_SKILLS.len() + INTERNAL_SKILLS.len());
    for skill in set.iter() {
        assert!(skill.is_builtin(), "{}", skill.name);
        assert!(!skill.description.is_empty(), "{}", skill.name);
        let body = skill.load_body().unwrap();
        assert!(body.starts_with('#'), "{}: {body:.40}", skill.name);
    }
    assert!(set.get("git-workflow").is_some());
    assert!(set.get("verify-before-done").is_some());
}

#[test]
fn an_installed_skill_shadows_its_builtin() {
    let tmp = tempfile::tempdir().unwrap();
    write_skill(
        tmp.path(),
        "git-workflow",
        "---\nname: git-workflow\ndescription: repo override\n---\n\ncustom body",
    );
    let set = SkillSet::discover(&[tmp.path().to_path_buf()]).with_builtins();
    let skill = set.get("git-workflow").unwrap();
    assert!(!skill.is_builtin());
    assert_eq!(skill.load_body().unwrap(), "custom body");
    assert_eq!(set.iter().filter(|s| s.name == "git-workflow").count(), 1);
}

#[test]
fn optional_skills_stay_out_of_the_default_index() {
    let set = SkillSet::default().with_builtins();
    for optional in optional_skills() {
        assert!(set.get(&optional.name).is_none(), "{}", optional.name);
    }
    assert!(!optional_skills().is_empty());
}

#[test]
fn install_bundled_materializes_a_discoverable_skill() {
    let tmp = tempfile::tempdir().unwrap();
    let dest = install_bundled("debug-systematically", tmp.path(), false).unwrap();
    assert!(dest.join(SKILL_FILE).is_file());
    assert_eq!(
        dest,
        tmp.path().join("internal").join("debug-systematically")
    );
    let set = SkillSet::discover(&[tmp.path().to_path_buf()]);
    let skill = set.get("debug-systematically").unwrap();
    assert!(!skill.is_builtin());
    assert!(skill.internal);
    assert!(set.visible().all(|s| s.name != "debug-systematically"));
    assert!(remove_skill(tmp.path(), "debug-systematically").unwrap());
    assert!(!dest.exists());
    install_bundled("debug-systematically", tmp.path(), false).unwrap();
    assert!(skill.load_body().unwrap().starts_with('#'));
    let err = install_bundled("debug-systematically", tmp.path(), false).unwrap_err();
    assert!(err.to_string().contains("already installed"), "{err:#}");
}

#[test]
fn defaults_install_once_and_respect_removal() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    install_defaults(root);
    if cfg!(target_os = "macos") {
        assert!(root.join("macos-harness").join(SKILL_FILE).is_file());
    } else {
        assert!(!root.join("macos-harness").exists());
    }
    // A second run is a no-op: already installed.
    install_defaults(root);
    if cfg!(target_os = "macos") {
        assert!(remove_skill(root, "macos-harness").unwrap());
        assert!(mark_default_removed(root, "macos-harness"));
        assert!(!root.join("macos-harness").exists());
        assert!(root.join(".removed-macos-harness").is_file());
        install_defaults(root);
        assert!(
            !root.join("macos-harness").exists(),
            "reinstalled after removal"
        );
    }
    assert!(!mark_default_removed(root, "git-workflow"));
}

#[test]
fn install_bundled_rejects_unknown_names() {
    let tmp = tempfile::tempdir().unwrap();
    let err = install_bundled("no-such-skill", tmp.path(), false).unwrap_err();
    assert!(format!("{err:#}").contains("no bundled skill"), "{err:#}");
}

#[test]
fn a_builtin_refuses_to_install() {
    let tmp = tempfile::tempdir().unwrap();
    let set = SkillSet::default().with_builtins();
    let err = install_skill(set.get("git-workflow").unwrap(), tmp.path(), false).unwrap_err();
    assert!(err.to_string().contains("built in"), "{err:#}");
}

#[test]
fn a_folded_description_reads_as_one_line() {
    let tmp = tempfile::tempdir().unwrap();
    write_skill(
        tmp.path(),
        "writer",
        "---\nname: writer\ndescription: >\n  Write blog posts and essays.\n  TRIGGER when drafting a post.\n---\n\nbody",
    );
    let set = SkillSet::discover(&[tmp.path().to_path_buf()]);
    let skill = set.get("writer").unwrap();
    assert_eq!(
        skill.description,
        "Write blog posts and essays. TRIGGER when drafting a post."
    );
    assert_eq!(skill.load_body().unwrap(), "body");
}

#[test]
fn a_literal_description_keeps_its_newlines() {
    let tmp = tempfile::tempdir().unwrap();
    write_skill(
        tmp.path(),
        "writer",
        "---\nname: writer\ndescription: |\n  line one\n  line two\n---\n\nbody",
    );
    let set = SkillSet::discover(&[tmp.path().to_path_buf()]);
    assert_eq!(set.get("writer").unwrap().description, "line one\nline two");
}

#[test]
fn an_indented_key_does_not_end_a_folded_description() {
    let tmp = tempfile::tempdir().unwrap();
    write_skill(
        tmp.path(),
        "writer",
        "---\nname: writer\ndescription: >\n  Reflects conventions: agentic ledes.\n  More prose.\n---\n\nbody",
    );
    let set = SkillSet::discover(&[tmp.path().to_path_buf()]);
    assert_eq!(
        set.get("writer").unwrap().description,
        "Reflects conventions: agentic ledes. More prose."
    );
}

#[test]
fn discovers_and_parses_frontmatter() {
    let tmp = tempfile::tempdir().unwrap();
    write_skill(
        tmp.path(),
        "pdf-tools",
        "---\nname: pdf-tools\ndescription: Work with PDFs. Use when the user mentions PDFs.\n---\n\n# PDF Tools\n\nDo the thing.",
    );
    let set = SkillSet::discover(&[tmp.path().to_path_buf()]);
    assert_eq!(set.len(), 1);
    let skill = set.get("pdf-tools").unwrap();
    assert!(skill.description.starts_with("Work with PDFs"));
    assert_eq!(skill.load_body().unwrap(), "# PDF Tools\n\nDo the thing.");
}

#[test]
fn name_falls_back_to_directory() {
    let tmp = tempfile::tempdir().unwrap();
    write_skill(
        tmp.path(),
        "reviewer",
        "---\ndescription: Review code carefully. Use before merging.\n---\nbody",
    );
    let set = SkillSet::discover(&[tmp.path().to_path_buf()]);
    assert!(set.get("reviewer").is_some());
}

#[test]
fn title_cased_name_is_slugified() {
    let tmp = tempfile::tempdir().unwrap();
    write_skill(
        tmp.path(),
        "ste",
        "---\nname: Simplified Technical English (ASD-STE100)\ndescription: Rewrite text.\n---\nbody",
    );
    let set = SkillSet::discover(&[tmp.path().to_path_buf()]);
    assert!(set.get("simplified-technical-english-asd-ste100").is_some());
}

#[test]
fn skips_skill_without_description() {
    let tmp = tempfile::tempdir().unwrap();
    write_skill(tmp.path(), "broken", "---\nname: broken\n---\nbody");
    let set = SkillSet::discover(&[tmp.path().to_path_buf()]);
    assert!(set.is_empty());
}

#[test]
fn project_root_shadows_global() {
    let project = tempfile::tempdir().unwrap();
    let global = tempfile::tempdir().unwrap();
    write_skill(
        project.path(),
        "dup",
        "---\nname: dup\ndescription: project version.\n---\nproject",
    );
    write_skill(
        global.path(),
        "dup",
        "---\nname: dup\ndescription: global version.\n---\nglobal",
    );
    let set = SkillSet::discover(&[project.path().to_path_buf(), global.path().to_path_buf()]);
    assert_eq!(set.len(), 1);
    assert_eq!(set.get("dup").unwrap().description, "project version.");
}

#[test]
fn empty_set_renders_no_index() {
    assert!(SkillSet::default().render_index().is_none());
}

#[test]
fn find_skills_walks_nested_trees() {
    let tmp = tempfile::tempdir().unwrap();
    let nested = tmp.path().join("skills").join("writing");
    fs::create_dir_all(&nested).unwrap();
    write_skill(
        &nested,
        "haiku",
        "---\nname: haiku\ndescription: Write haiku. Use for short poems.\n---\nbody",
    );
    let found = find_skills(tmp.path(), false);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].name, "haiku");
}

#[test]
fn install_copies_bundled_resources() {
    let src_root = tempfile::tempdir().unwrap();
    let dir = src_root.path().join("pdf");
    fs::create_dir_all(dir.join("scripts")).unwrap();
    fs::write(
        dir.join(SKILL_FILE),
        "---\nname: pdf\ndescription: Handle PDFs. Use for PDF files.\n---\nbody",
    )
    .unwrap();
    fs::write(dir.join("scripts").join("fill.py"), "print('hi')").unwrap();

    let dest = tempfile::tempdir().unwrap();
    let skill = &find_skills(src_root.path(), false)[0];
    let installed = install_skill(skill, dest.path(), false).unwrap();
    assert!(installed.join(SKILL_FILE).is_file());
    assert!(installed.join("scripts").join("fill.py").is_file());

    assert!(install_skill(skill, dest.path(), false).is_err());
    assert!(install_skill(skill, dest.path(), true).is_ok());

    assert!(remove_skill(dest.path(), "pdf").unwrap());
    assert!(!remove_skill(dest.path(), "pdf").unwrap());
}

#[test]
fn internal_builtins_are_indexed_but_never_visible() {
    let set = SkillSet::default().with_builtins();
    assert_eq!(set.visible().count(), BUILTIN_SKILLS.len());
    assert!(set.is_internal("correction-protocol"));
    assert!(!set.is_internal("git-workflow"));
    assert!(set.get("correction-protocol").is_some());
    let index = set.render_index().expect("index");
    assert!(index.contains("correction-protocol"));
}

#[test]
fn an_installed_internal_folder_marks_its_skills_internal() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join("internal").join("house-rules")).unwrap();
    fs::write(
        root.join("internal").join("house-rules").join("SKILL.md"),
        "---\ndescription: How the agent behaves here.\n---\nBe brief.\n",
    )
    .unwrap();
    fs::create_dir_all(root.join("deploy")).unwrap();
    fs::write(
        root.join("deploy").join("SKILL.md"),
        "---\ndescription: Deploy the service.\n---\nRun deploy.\n",
    )
    .unwrap();

    let set = SkillSet::discover(&[root.to_path_buf()]);
    assert_eq!(set.len(), 2);
    assert!(set.is_internal("house-rules"));
    assert_eq!(
        set.visible().map(|s| s.name.as_str()).collect::<Vec<_>>(),
        ["deploy"]
    );
}
