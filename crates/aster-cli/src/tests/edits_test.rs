use super::*;

const REPLY: &str = "<<<<<<< SEARCH\nlet a = 1;\n=======\nlet a = 2;\n>>>>>>> REPLACE\n";

#[test]
fn resolve_anywhere_classifies_in_repo_and_outside() {
    let repo = tempfile::tempdir().unwrap();
    fs::write(repo.path().join("inside.rs"), "").unwrap();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("elsewhere.txt"), "").unwrap();

    let (_, scope) = resolve_anywhere(repo.path(), "inside.rs").unwrap();
    assert_eq!(scope, Scope::InRepo);

    let path = outside.path().join("elsewhere.txt");
    let (resolved, scope) = resolve_anywhere(repo.path(), &path.to_string_lossy()).unwrap();
    assert_eq!(scope, Scope::Outside);
    assert_eq!(resolved, path.canonicalize().unwrap());
}

#[test]
fn resolve_anywhere_expands_a_leading_tilde() {
    let repo = tempfile::tempdir().unwrap();
    let home = dirs::home_dir().unwrap();
    let (resolved, scope) = resolve_anywhere(repo.path(), "~").unwrap();
    assert_eq!(resolved, home.canonicalize().unwrap());
    assert_eq!(scope, Scope::Outside);
}

#[test]
fn resolve_new_accepts_a_missing_nested_path() {
    let repo = tempfile::tempdir().unwrap();
    let (target, scope) = resolve_new_anywhere(repo.path(), "src/deep/new.rs").unwrap();
    assert!(target.ends_with("src/deep/new.rs"));
    assert!(!target.exists());
    assert_eq!(scope, Scope::InRepo);
}

#[test]
fn resolve_new_reports_paths_leaving_the_repo_instead_of_failing() {
    let repo = tempfile::tempdir().unwrap();
    for path in ["../outside.rs", "src/../../outside.rs", "/etc/passwd"] {
        let (_, scope) = resolve_new_anywhere(repo.path(), path).unwrap();
        assert_eq!(scope, Scope::Outside, "{path}");
    }
}

#[test]
fn parse_blocks_single_block() {
    let blocks = parse_blocks(REPLY).unwrap();
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].search, "let a = 1;");
    assert_eq!(blocks[0].replace, "let a = 2;");
}

#[test]
fn parse_blocks_tolerates_surrounding_prose() {
    let reply = format!("Here is the fix:\n```\n{REPLY}```\ndone");
    assert_eq!(parse_blocks(&reply).unwrap().len(), 1);
}

#[test]
fn parse_blocks_unterminated_fails() {
    assert!(parse_blocks("<<<<<<< SEARCH\nx\n=======\ny\n").is_err());
}

#[test]
fn apply_block_replaces_unique_match() {
    let block = EditBlock {
        search: "b".into(),
        replace: "c".into(),
    };
    assert_eq!(apply_block("a b", &block).unwrap(), "a c");
}

#[test]
fn apply_block_ambiguous_match_fails() {
    let block = EditBlock {
        search: "a".into(),
        replace: "c".into(),
    };
    assert!(apply_block("a a", &block).is_err());
}

#[test]
fn apply_block_mismatch_embeds_closest_region() {
    let content = "fn alpha() {}\nfn beta(count: usize) -> usize {\n    count + 1\n}\n";
    let block = EditBlock {
        search: "fn beta(count: u32) -> u32 {".into(),
        replace: "fn beta(count: u64) -> u64 {".into(),
    };
    let err = format!("{:#}", apply_block(content, &block).unwrap_err());
    assert!(err.contains("Closest match"), "{err}");
    assert!(err.contains("fn beta(count: usize) -> usize {"), "{err}");
    assert!(err.contains("Re-issue the edit"), "{err}");
}

#[test]
fn apply_block_mismatch_without_similar_text_says_so() {
    let block = EditBlock {
        search: "completely unrelated text".into(),
        replace: "x".into(),
    };
    let err = format!("{:#}", apply_block("zzz\nqqq\n", &block).unwrap_err());
    assert!(err.contains("nothing similar"), "{err}");
}

#[test]
fn closest_region_survives_indentation_drift() {
    let content = "one\ntwo\n        let value = compute(input);\nfour\n";
    let region = closest_region(content, "let value = compute(input);").unwrap();
    assert!(region.contains("compute(input)"), "{region}");
    assert!(region.contains("lines 1-"), "{region}");
}
