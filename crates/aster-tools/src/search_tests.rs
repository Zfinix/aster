use super::*;
use std::fs;
use tempfile::tempdir;

fn run(repo: &Path, query: &str, max_hits: usize) -> Vec<Hit> {
    search(&ToolProbe::default(), repo, repo, query, max_hits).unwrap()
}

#[test]
fn search_finds_substring() {
    let repo = tempdir().unwrap();
    fs::write(repo.path().join("a.txt"), "hello world\nfoo\n").unwrap();
    let hits = run(repo.path(), "hello", 10);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].path, "a.txt");
    assert_eq!(hits[0].line, 1);
    assert_eq!(hits[0].text, "hello world");
}

#[test]
fn search_respects_gitignore() {
    let repo = tempdir().unwrap();
    fs::write(repo.path().join(".gitignore"), "*.secret\n").unwrap();
    fs::write(repo.path().join("foo.secret"), "needle here\n").unwrap();
    fs::write(repo.path().join("bar.txt"), "needle found\n").unwrap();
    let hits = run(repo.path(), "needle", 10);
    assert!(hits.iter().any(|h| h.path == "bar.txt"), "{hits:?}");
    assert!(
        !hits.iter().any(|h| h.path.contains("secret")),
        "gitignored file leaked: {hits:?}"
    );
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
    for name in ["a.txt", "b.txt", "c.txt"] {
        fs::write(repo.path().join(name), "match line\n".repeat(100)).unwrap();
    }
    let hits = run(repo.path(), "match", 5);
    assert_eq!(hits.len(), 5, "{hits:?}");
}

#[test]
fn search_no_matches() {
    let repo = tempdir().unwrap();
    fs::write(repo.path().join("a.txt"), "nothing here\n").unwrap();
    assert!(run(repo.path(), "zzzz", 10).is_empty());
}

#[test]
fn search_regex_pattern() {
    let repo = tempdir().unwrap();
    fs::write(repo.path().join("a.rs"), "fn main() {}\nfn helper() {}\n").unwrap();
    let hits = run(repo.path(), r"fn\s+main", 10);
    assert_eq!(hits.len(), 1, "{hits:?}");
    assert_eq!(hits[0].line, 1);
}

#[test]
fn one_hot_file_does_not_spend_the_whole_budget() {
    let repo = tempdir().unwrap();
    fs::write(repo.path().join("hot.txt"), "needle\n".repeat(100)).unwrap();
    fs::write(repo.path().join("cold.txt"), "needle\n").unwrap();
    let hits = run(repo.path(), "needle", 80);
    assert!(
        hits.iter().any(|h| h.path == "cold.txt"),
        "the quiet file was crowded out: {hits:?}"
    );
    assert_eq!(
        hits.iter().filter(|h| h.path == "hot.txt").count(),
        PER_FILE
    );
}

#[test]
fn lowercase_query_ignores_case_uppercase_does_not() {
    let repo = tempdir().unwrap();
    fs::write(repo.path().join("a.rs"), "Explored\nexplored\n").unwrap();
    assert_eq!(run(repo.path(), "explored", 10).len(), 2);
    let exact = run(repo.path(), "Explored", 10);
    assert_eq!(exact.len(), 1, "{exact:?}");
    assert_eq!(exact[0].line, 1);
}

#[test]
fn render_groups_by_file_with_context() {
    let repo = tempdir().unwrap();
    fs::write(repo.path().join("a.rs"), "one\ntwo\nneedle\nfour\nfive\n").unwrap();
    let out = render(repo.path(), &run(repo.path(), "needle", 10), 1);
    assert!(out.starts_with("a.rs\n"), "{out}");
    assert!(out.contains("  2  two"), "{out}");
    assert!(out.contains("> 3  needle"), "{out}");
    assert!(out.contains("  4  four"), "{out}");
    assert!(!out.contains("one"), "context should stop at 1 line: {out}");
}

#[test]
fn render_separates_windows_that_do_not_touch() {
    let repo = tempdir().unwrap();
    let mut content = String::from("needle\n");
    content.push_str(&"filler\n".repeat(20));
    content.push_str("needle\n");
    fs::write(repo.path().join("a.rs"), content).unwrap();
    let out = render(repo.path(), &run(repo.path(), "needle", 10), 1);
    assert!(out.contains("--"), "{out}");
}

#[test]
fn rg_and_the_embedded_walker_agree() {
    let probe = ToolProbe::detect();
    let Some(_) = probe.rg else {
        return;
    };
    let repo = tempdir().unwrap();
    fs::write(
        repo.path().join("hot.rs"),
        "Needle\nneedle\nneedle\nneedle\n",
    )
    .unwrap();
    fs::write(repo.path().join("cold.rs"), "needle\n").unwrap();

    let with_rg = search(&probe, repo.path(), repo.path(), "needle", 80).unwrap();
    let embedded = run(repo.path(), "needle", 80);

    let key = |h: &Hit| (h.path.clone(), h.line);
    let mut a: Vec<_> = with_rg.iter().map(key).collect();
    let mut b: Vec<_> = embedded.iter().map(key).collect();
    a.sort();
    b.sort();
    assert_eq!(a, b, "rg: {with_rg:?}\nembedded: {embedded:?}");
    assert!(
        with_rg.iter().all(|h| !h.path.starts_with('/')),
        "{with_rg:?}"
    );
}

#[test]
fn a_single_file_base_still_returns_its_hits() {
    let repo = tempdir().unwrap();
    fs::create_dir(repo.path().join("src")).unwrap();
    let file = repo.path().join("src/a.rs");
    fs::write(&file, "one\nneedle\nthree\n").unwrap();
    fs::write(repo.path().join("src/b.rs"), "needle\n").unwrap();

    for probe in [ToolProbe::default(), ToolProbe::detect()] {
        let hits = search(&probe, repo.path(), &file, "needle", 80).unwrap();
        assert_eq!(hits.len(), 1, "{probe:?} {hits:?}");
        assert_eq!(hits[0].path, "src/a.rs", "{probe:?} {hits:?}");
        assert_eq!(hits[0].line, 2, "{probe:?} {hits:?}");
    }
}

#[test]
fn render_says_so_when_there_is_nothing() {
    let repo = tempdir().unwrap();
    assert_eq!(render(repo.path(), &[], 3), "no matches");
}
