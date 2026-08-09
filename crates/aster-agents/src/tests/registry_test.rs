use super::*;

fn write_agent(root: &Path, name: &str, body: &str) {
    let dir = root.join(name);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join(AGENT_FILE), body).unwrap();
}

#[test]
fn builtins_present_and_valid() {
    let registry = AgentRegistry::discover(&[]);
    for name in ["explorer", "reviewer", "fixer", "synthesizer"] {
        let agent = registry.get(name).unwrap();
        assert!(agent.is_builtin());
        assert!(!agent.load_body().unwrap().is_empty());
    }
    assert!(registry.get("reviewer").unwrap().verify);
    assert!(registry.get("synthesizer").unwrap().verify);
    assert!(
        registry
            .get("fixer")
            .unwrap()
            .tools
            .as_deref()
            .unwrap()
            .contains(&"edit_file".to_string())
    );
}

#[test]
fn user_agent_shadows_builtin() {
    let tmp = tempfile::tempdir().unwrap();
    write_agent(
        tmp.path(),
        "explorer",
        "---\ndescription: project override.\n---\nbe brief",
    );
    let registry = AgentRegistry::discover(&[tmp.path().to_path_buf()]);
    let explorer = registry.get("explorer").unwrap();
    assert!(!explorer.is_builtin());
    assert_eq!(explorer.description, "project override.");
}

#[test]
fn project_root_shadows_global() {
    let project = tempfile::tempdir().unwrap();
    let global = tempfile::tempdir().unwrap();
    write_agent(
        project.path(),
        "dup",
        "---\ndescription: project version.\n---\np",
    );
    write_agent(
        global.path(),
        "dup",
        "---\ndescription: global version.\n---\ng",
    );
    let registry =
        AgentRegistry::discover(&[project.path().to_path_buf(), global.path().to_path_buf()]);
    assert_eq!(registry.get("dup").unwrap().description, "project version.");
}

#[test]
fn malformed_agent_skipped() {
    let tmp = tempfile::tempdir().unwrap();
    write_agent(
        tmp.path(),
        "broken",
        "---\nname: broken\n---\nno description",
    );
    let registry = AgentRegistry::discover(&[tmp.path().to_path_buf()]);
    assert!(registry.get("broken").is_none());
}

#[test]
fn index_lists_agents() {
    let registry = AgentRegistry::discover(&[]);
    let index = registry.render_index().unwrap();
    assert!(index.contains("**explorer**"));
    assert!(index.contains("**reviewer**"));
    assert!(index.contains("**fixer**"));
    assert!(index.contains("**synthesizer**"));
    assert!(index.contains("fan several cheap agents"));
}
