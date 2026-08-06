use super::*;
use std::fs;
use tempfile::tempdir;

fn tree(repo: &Path) {
    fs::create_dir_all(repo.join("crates/aster-cli/src")).unwrap();
    fs::write(repo.join("crates/aster-cli/src/chat.rs"), "").unwrap();
    fs::write(repo.join("crates/aster-cli/src/edits.rs"), "").unwrap();
    fs::write(repo.join("README.md"), "").unwrap();
}

#[test]
fn find_matches_a_bare_name_at_any_depth() {
    let repo = tempdir().unwrap();
    tree(repo.path());
    let out = find(repo.path(), repo.path(), "chat.rs", 10).unwrap();
    assert_eq!(out, "crates/aster-cli/src/chat.rs");
}

#[test]
fn find_matches_an_extension_glob() {
    let repo = tempdir().unwrap();
    tree(repo.path());
    let out = find(repo.path(), repo.path(), "*.rs", 10).unwrap();
    assert!(out.contains("chat.rs"), "{out}");
    assert!(out.contains("edits.rs"), "{out}");
    assert!(!out.contains("README.md"), "{out}");
}

#[test]
fn find_matches_a_path_glob() {
    let repo = tempdir().unwrap();
    tree(repo.path());
    let out = find(repo.path(), repo.path(), "crates/*/src/*.rs", 10).unwrap();
    assert!(out.contains("crates/aster-cli/src/chat.rs"), "{out}");
}

#[test]
fn find_reports_no_matches() {
    let repo = tempdir().unwrap();
    tree(repo.path());
    let out = find(repo.path(), repo.path(), "*.py", 10).unwrap();
    assert_eq!(out, "no files matched");
}

#[test]
fn find_empty_pattern_errors() {
    let repo = tempdir().unwrap();
    assert!(find(repo.path(), repo.path(), "  ", 10).is_err());
}

#[test]
fn find_reaches_a_gitignored_path_when_nothing_else_matches() {
    let repo = tempdir().unwrap();
    tree(repo.path());
    fs::write(repo.path().join(".gitignore"), "editors/\n").unwrap();
    fs::create_dir_all(repo.path().join("editors/vscode")).unwrap();
    fs::write(repo.path().join("editors/vscode/package.json"), "").unwrap();

    let out = find(repo.path(), repo.path(), "**/vscode/**", 10).unwrap();
    assert!(out.contains("editors/vscode/package.json"), "{out}");
    assert!(out.contains("ignored by .gitignore"), "{out}");
}

#[test]
fn find_prefers_tracked_matches_over_ignored_ones() {
    let repo = tempdir().unwrap();
    tree(repo.path());
    fs::write(repo.path().join(".gitignore"), "editors/\n").unwrap();
    fs::create_dir_all(repo.path().join("editors")).unwrap();
    fs::write(repo.path().join("editors/chat.rs"), "").unwrap();

    let out = find(repo.path(), repo.path(), "chat.rs", 10).unwrap();
    assert_eq!(out, "crates/aster-cli/src/chat.rs");
}

#[test]
fn find_skips_build_output_on_the_ignored_pass() {
    let repo = tempdir().unwrap();
    tree(repo.path());
    fs::write(repo.path().join(".gitignore"), "target/\n").unwrap();
    fs::create_dir_all(repo.path().join("target/debug")).unwrap();
    fs::write(repo.path().join("target/debug/huge.rs"), "").unwrap();

    let out = find(repo.path(), repo.path(), "huge.rs", 10).unwrap();
    assert_eq!(out, "no files matched");
}
