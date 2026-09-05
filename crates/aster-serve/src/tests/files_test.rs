use super::*;

use std::fs;
use std::path::Path;

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
    let staged = stage(dir.path(), "staged-test.png", 3, b"png").expect("stage");
    assert!(staged.ends_with("staged-test.png"), "{staged}");
    assert!(
        Path::new(&staged).starts_with(std::env::temp_dir().join("aster-pasted")),
        "{staged}"
    );
    assert_eq!(fs::read(&staged).expect("staged file"), b"png");
    let _ = fs::remove_file(&staged);
}

#[test]
fn a_second_paste_of_the_same_name_keeps_both() {
    let dir = repo();
    let first = stage(dir.path(), "collision-test.png", 3, b"png").expect("stage");
    let second = stage(dir.path(), "collision-test.png", 4, b"png!").expect("stage");
    assert!(first.ends_with("collision-test.png"), "{first}");
    assert!(second.ends_with("collision-test-1.png"), "{second}");
    assert_eq!(fs::read(&first).expect("first"), b"png");
    assert_eq!(fs::read(&second).expect("second"), b"png!");
    let _ = fs::remove_file(&first);
    let _ = fs::remove_file(&second);
}

#[test]
fn a_pasted_name_cannot_pick_the_directory() {
    assert_eq!(sanitize("../../etc/passwd"), "passwd");
    assert_eq!(sanitize(".."), "pasted");
    assert_eq!(sanitize("shot.png"), "shot.png");
}

#[test]
fn a_staged_paste_outside_the_repo_is_still_previewable() {
    let dir = repo();
    let staged = stage(dir.path(), "preview-test.png", 4, b"png!").expect("stage");
    let file = preview(dir.path(), &staged).expect("preview");
    assert_eq!(
        file.image,
        Some("data:image/png;base64,cG5nIQ==".to_string())
    );
}

#[test]
fn a_staged_document_previews_as_a_data_url_with_its_size() {
    let dir = repo();
    let staged = stage(dir.path(), "report.pdf", 5, b"%PDF-").expect("stage");
    let file = preview(dir.path(), &staged).expect("preview");
    assert_eq!(
        file.doc,
        Some("data:application/pdf;base64,JVBERi0=".to_string())
    );
    assert_eq!(file.image, None);
    assert_eq!(file.size, Some(5));
}

#[test]
fn a_preview_survives_a_space_in_the_path() {
    let dir = repo();
    fs::write(dir.path().join("my notes.md"), "# hi\n").expect("file");
    let file = preview(dir.path(), "my notes.md").expect("preview");
    assert_eq!(file.content, "# hi\n");
    assert!(!file.truncated);
}

#[test]
fn an_image_preview_is_a_data_url_not_text() {
    let dir = repo();
    fs::write(dir.path().join("shot.png"), b"\x89PNG bytes").expect("file");
    let file = preview(dir.path(), "shot.png").expect("preview");
    assert_eq!(
        file.image.as_deref(),
        Some("data:image/png;base64,iVBORyBieXRlcw==")
    );
    assert_eq!(file.content, "");
}

#[test]
fn an_image_serves_its_bytes_with_a_mime() {
    let dir = repo();
    fs::write(dir.path().join("shot.png"), b"\x89PNG bytes").expect("file");
    let (mime, bytes) = serve(dir.path(), "shot.png").expect("served");
    assert_eq!(mime, "image/png");
    assert_eq!(bytes, b"\x89PNG bytes");
}

#[test]
fn a_document_serves_its_bytes_without_rendering_anything() {
    let dir = repo();
    let staged = stage(dir.path(), "report.pdf", 5, b"%PDF-").expect("stage");
    let (mime, bytes) = serve(dir.path(), &staged).expect("served");
    assert_eq!(mime, "application/pdf");
    assert_eq!(bytes, b"%PDF-");
}

#[test]
fn serving_stays_inside_the_repo() {
    let dir = repo();
    assert_eq!(serve(dir.path(), "../outside.txt"), None);
    assert_eq!(serve(dir.path(), "gone.png"), None);
}

#[test]
fn a_preview_stays_inside_the_repo_and_is_bounded() {
    let dir = repo();
    assert_eq!(preview(dir.path(), "../outside.txt"), None);
    let many = "line\n".repeat(400);
    fs::write(dir.path().join("big.rs"), &many).expect("file");
    let file = preview(dir.path(), "big.rs").expect("preview");
    assert!(file.truncated);
    assert_eq!(file.content.lines().count(), 200);
}
