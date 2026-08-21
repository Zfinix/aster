use super::*;

use std::fs;

fn repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    fs::create_dir_all(root.join("crates/aster-serve/src")).expect("dirs");
    fs::write(root.join("crates/aster-serve/src/guard.rs"), "fn main() {}").expect("file");
    fs::write(root.join("README.md"), "# repo").expect("file");
    fs::write(root.join(".gitignore"), "target/\n").expect("file");
    fs::create_dir_all(root.join("target")).expect("dirs");
    fs::write(root.join("target/build.log"), "noise").expect("file");
    dir
}

#[test]
fn a_query_matches_anywhere_in_the_path() {
    let dir = repo();
    let found = search(dir.path(), "guard");
    assert!(
        found.iter().any(|p| p == "crates/aster-serve/src/guard.rs"),
        "{found:?}"
    );
}

#[test]
fn ignored_files_stay_out_of_the_menu() {
    let dir = repo();
    let found = search(dir.path(), "build");
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn root_entries_come_before_nested_ones() {
    let dir = repo();
    let found = search(dir.path(), "");
    let readme = found.iter().position(|p| p == "README.md").expect("README");
    let nested = found
        .iter()
        .position(|p| p.starts_with("crates/aster-serve/src"))
        .expect("nested path");
    assert!(readme < nested, "{found:?}");
}

#[test]
fn a_dropped_file_uri_becomes_a_repo_relative_mention() {
    let dir = repo();
    let uri = format!("file://{}/README.md", dir.path().display());
    assert_eq!(mention(dir.path(), &uri), Some("README.md".to_string()));
}

#[test]
fn an_escaped_space_survives_the_trip() {
    let dir = repo();
    fs::write(dir.path().join("a file.txt"), "x").expect("file");
    let uri = format!("file://{}/a%20file.txt", dir.path().display());
    assert_eq!(mention(dir.path(), &uri), Some("a file.txt".to_string()));
}

#[test]
fn a_path_that_is_not_there_is_not_mentioned() {
    let dir = repo();
    let uri = format!("file://{}/gone.txt", dir.path().display());
    assert_eq!(mention(dir.path(), &uri), None);
    assert_eq!(mention(dir.path(), "https://example.test/x"), None);
}

#[test]
fn a_paste_matching_one_repo_file_mentions_that_file() {
    let dir = repo();
    let staged = stage(dir.path(), "README.md", 6, b"# repo").expect("stage");
    assert_eq!(staged, "README.md", "the real file beats a copy of it");
}

#[test]
fn a_paste_from_elsewhere_is_written_somewhere_the_agent_can_read() {
    let dir = repo();
    let staged = stage(dir.path(), "shot.png", 3, b"png").expect("stage");
    assert!(staged.ends_with("-shot.png"), "{staged}");
    assert_eq!(fs::read(&staged).expect("staged file"), b"png");
    let _ = fs::remove_file(&staged);
}

#[test]
fn a_pasted_name_cannot_pick_the_directory() {
    assert_eq!(sanitize("../../etc/passwd"), "passwd");
    assert_eq!(sanitize(".."), "pasted");
    assert_eq!(sanitize("shot.png"), "shot.png");
}
