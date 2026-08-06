use super::*;
use std::fs;
use tempfile::tempdir;

#[test]
fn list_sorts_entries() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("c.txt"), "").unwrap();
    fs::write(dir.path().join("a.txt"), "").unwrap();
    fs::write(dir.path().join("b.txt"), "").unwrap();
    let probe = ToolProbe::default();
    let out = list(&probe, dir.path(), 10).unwrap();
    let entries: Vec<&str> = out.lines().collect();
    assert_eq!(entries, vec!["a.txt", "b.txt", "c.txt"], "{out}");
}

#[test]
fn list_truncates_at_max() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("a.txt"), "").unwrap();
    fs::write(dir.path().join("b.txt"), "").unwrap();
    fs::write(dir.path().join("c.txt"), "").unwrap();
    let probe = ToolProbe::default();
    let out = list(&probe, dir.path(), 2).unwrap();
    let entries: Vec<&str> = out.lines().collect();
    assert_eq!(entries.len(), 2, "{out}");
    assert_eq!(entries, vec!["a.txt", "b.txt"], "{out}");
}

#[test]
fn list_marks_directories() {
    let dir = tempdir().unwrap();
    fs::create_dir(dir.path().join("subdir")).unwrap();
    fs::write(dir.path().join("file.txt"), "").unwrap();
    let probe = ToolProbe::default();
    let out = list(&probe, dir.path(), 10).unwrap();
    assert!(
        out.contains("subdir/"),
        "directory should have trailing slash: {out}"
    );
    assert!(out.contains("file.txt"), "{out}");
}

#[test]
fn list_empty_dir() {
    let dir = tempdir().unwrap();
    let probe = ToolProbe::default();
    let out = list(&probe, dir.path(), 10).unwrap();
    assert!(out.is_empty(), "{out}");
}
