use super::*;
use tempfile::tempdir;

fn write(root: &Path, path: &str, body: &str) {
    let full = root.join(path);
    fs::create_dir_all(full.parent().unwrap()).unwrap();
    fs::write(full, body).unwrap();
}

#[test]
fn a_repo_with_no_instruction_files_contributes_nothing() {
    let repo = tempdir().unwrap();
    let found = discover(repo.path());
    assert!(found.is_empty());
    assert!(found.render().is_none());
}

#[test]
fn an_agents_file_is_read_even_though_it_was_written_for_another_tool() {
    let repo = tempdir().unwrap();
    write(
        repo.path(),
        "AGENTS.md",
        "Always run `cargo fmt` before finishing.",
    );
    let rendered = discover(repo.path()).render().expect("a section");
    assert!(rendered.contains("### AGENTS.md"), "{rendered}");
    assert!(rendered.contains("cargo fmt"), "{rendered}");
}

#[test]
fn every_known_root_file_is_read_with_asters_own_last() {
    let repo = tempdir().unwrap();
    write(repo.path(), "CLAUDE.md", "claude rules");
    write(repo.path(), "AGENTS.md", "agents rules");
    write(repo.path(), "ASTER.md", "aster rules");
    let found = discover(repo.path());
    assert_eq!(found.labels(), vec!["CLAUDE.md", "AGENTS.md", "ASTER.md"]);
    let rendered = found.render().unwrap();
    assert!(
        rendered.find("aster rules") > rendered.find("claude rules"),
        "Aster's own file should read last: {rendered}"
    );
}

#[test]
fn the_same_text_under_two_names_is_only_paid_for_once() {
    let repo = tempdir().unwrap();
    write(repo.path(), "AGENTS.md", "one rule");
    write(repo.path(), "CLAUDE.md", "one rule");
    let found = discover(repo.path());
    assert_eq!(found.labels().len(), 1, "{:?}", found.labels());
    assert_eq!(found.render().unwrap().matches("one rule").count(), 1);
}

#[test]
fn nested_files_are_listed_by_path_not_pasted_into_the_prompt() {
    let repo = tempdir().unwrap();
    write(repo.path(), "AGENTS.md", "root rules");
    write(repo.path(), "packages/api/AGENTS.md", "SECRET_API_RULE");
    let found = discover(repo.path());
    let rendered = found.render().unwrap();
    assert_eq!(found.nested(), [PathBuf::from("packages/api/AGENTS.md")]);
    assert!(rendered.contains("packages/api/AGENTS.md"), "{rendered}");
    assert!(
        !rendered.contains("SECRET_API_RULE"),
        "a nested body was preloaded: {rendered}"
    );
}

#[test]
fn the_nearest_file_governs_a_path() {
    let repo = tempdir().unwrap();
    write(repo.path(), "AGENTS.md", "root");
    write(repo.path(), "packages/AGENTS.md", "packages");
    write(repo.path(), "packages/api/AGENTS.md", "api");
    let found = discover(repo.path());
    assert_eq!(
        found.nearest(Path::new("packages/api/src/main.rs")),
        Some(Path::new("packages/api/AGENTS.md"))
    );
    assert_eq!(
        found.nearest(Path::new("packages/web/index.ts")),
        Some(Path::new("packages/AGENTS.md"))
    );
    // Nothing nested governs a root-level file; the root body is already loaded.
    assert_eq!(found.nearest(Path::new("README.md")), None);
}

#[test]
fn a_gitignored_directory_is_not_indexed() {
    let repo = tempdir().unwrap();
    write(repo.path(), ".gitignore", "vendor/\n");
    write(repo.path(), "vendor/dep/AGENTS.md", "not ours");
    assert!(discover(repo.path()).nested().is_empty());
}

#[test]
fn an_empty_file_is_not_a_section() {
    let repo = tempdir().unwrap();
    write(repo.path(), "AGENTS.md", "   \n\n");
    assert!(discover(repo.path()).is_empty());
}

#[test]
fn an_oversized_file_is_capped_and_says_so() {
    let repo = tempdir().unwrap();
    write(repo.path(), "AGENTS.md", &"x".repeat(MAX_FILE_CHARS * 2));
    let rendered = discover(repo.path()).render().unwrap();
    assert!(rendered.contains("[truncated"), "no truncation notice");
    assert!(rendered.chars().count() < MAX_FILE_CHARS * 2);
}

#[test]
fn the_total_budget_holds_across_several_files() {
    let repo = tempdir().unwrap();
    for name in INSTRUCTION_FILES {
        write(
            repo.path(),
            name,
            &format!("{name}{}", "y".repeat(MAX_FILE_CHARS)),
        );
    }
    let rendered = discover(repo.path()).render().unwrap();
    assert!(
        rendered.chars().count() <= MAX_TOTAL_CHARS + 1_000,
        "instructions overran their budget: {}",
        rendered.chars().count()
    );
}
