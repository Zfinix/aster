use super::*;
use std::fs;
use tempfile::tempdir;

#[test]
fn search_finds_substring() {
    let repo = tempdir().unwrap();
    fs::write(repo.path().join("a.txt"), "hello world\nfoo\n").unwrap();
    let probe = ToolProbe::default();
    let out = search(&probe, repo.path(), repo.path(), "hello", 10).unwrap();
    assert!(out.contains("a.txt:1:"), "{out}");
    assert!(out.contains("hello world"), "{out}");
}

#[test]
fn search_respects_gitignore() {
    let repo = tempdir().unwrap();
    fs::write(repo.path().join(".gitignore"), "*.secret\n").unwrap();
    fs::write(repo.path().join("foo.secret"), "needle here\n").unwrap();
    fs::write(repo.path().join("bar.txt"), "needle found\n").unwrap();
    let probe = ToolProbe::default();
    let out = search(&probe, repo.path(), repo.path(), "needle", 10).unwrap();
    assert!(out.contains("bar.txt"), "{out}");
    assert!(!out.contains("foo.secret"), "gitignored file leaked: {out}");
}

#[test]
fn search_empty_query_errors() {
    let repo = tempdir().unwrap();
    let probe = ToolProbe::default();
    assert!(search(&probe, repo.path(), repo.path(), "  ", 10).is_err());
}

#[test]
fn search_truncates_at_max() {
    let repo = tempdir().unwrap();
    let mut content = String::new();
    for _ in 0..100 {
        content.push_str("match line\n");
    }
    fs::write(repo.path().join("big.txt"), content).unwrap();
    let probe = ToolProbe::default();
    let out = search(&probe, repo.path(), repo.path(), "match", 5).unwrap();
    assert_eq!(out.lines().count(), 5, "{out}");
}

#[test]
fn search_no_matches() {
    let repo = tempdir().unwrap();
    fs::write(repo.path().join("a.txt"), "nothing here\n").unwrap();
    let probe = ToolProbe::default();
    let out = search(&probe, repo.path(), repo.path(), "zzzz", 10).unwrap();
    assert_eq!(out, "no matches");
}

#[test]
fn search_regex_pattern() {
    let repo = tempdir().unwrap();
    fs::write(repo.path().join("a.rs"), "fn main() {}\nfn helper() {}\n").unwrap();
    let probe = ToolProbe::default();
    let out = search(&probe, repo.path(), repo.path(), r"fn\s+main", 10).unwrap();
    assert!(out.contains("a.rs:1:"), "{out}");
    assert!(!out.contains("helper"), "{out}");
}
