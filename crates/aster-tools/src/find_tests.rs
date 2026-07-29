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
