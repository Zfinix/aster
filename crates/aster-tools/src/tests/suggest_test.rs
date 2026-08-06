use super::*;
use std::fs;
use tempfile::tempdir;

fn tree(repo: &Path) {
    fs::create_dir_all(repo.join("crates/aster-cli/src/tui")).unwrap();
    fs::write(repo.join("crates/aster-cli/src/chat.rs"), "").unwrap();
    fs::write(repo.join("crates/aster-cli/src/tui/composer.rs"), "").unwrap();
}

#[test]
fn suggest_finds_the_right_directory_for_a_known_name() {
    let repo = tempdir().unwrap();
    tree(repo.path());
    let out = suggest(repo.path(), "crates/ui/src/composer.rs", 5);
    assert_eq!(
        out.first().map(String::as_str),
        Some("crates/aster-cli/src/tui/composer.rs"),
        "{out:?}"
    );
}

#[test]
fn suggest_matches_directories_too() {
    let repo = tempdir().unwrap();
    tree(repo.path());
    let out = suggest(repo.path(), "crates/aster-tui", 5);
    assert!(out.iter().any(|p| p.ends_with("tui")), "{out:?}");
}

#[test]
fn suggest_returns_nothing_when_nothing_is_close() {
    let repo = tempdir().unwrap();
    tree(repo.path());
    assert!(suggest(repo.path(), "zzzzz.kt", 5).is_empty());
}
