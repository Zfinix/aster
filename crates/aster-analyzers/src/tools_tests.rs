use std::fs;

use crate::Analyzer;
use crate::tools::{ast_edit_apply, ast_grep_search, security_scan};

fn fixture(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    for (name, body) in files {
        let path = dir.path().join(name);
        fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        fs::write(path, body).expect("write fixture");
    }
    dir
}

#[test]
fn search_finds_pattern_with_file_and_line() {
    let dir = fixture(&[("src/a.rs", "fn main() {\n    println!(\"hi\");\n}\n")]);
    let out = ast_grep_search(dir.path(), "println!($$$ARGS)", Some("rust")).expect("search");
    assert_eq!(
        out,
        format!(
            "{}:2: println!(\"hi\")",
            dir.path().join("src/a.rs").display()
        )
    );
}

#[test]
fn search_reports_no_matches() {
    let dir = fixture(&[("a.rs", "fn main() {}\n")]);
    let out = ast_grep_search(dir.path(), "nothing_here($X)", Some("rust")).expect("search");
    assert_eq!(out, "no matches");
}

#[test]
fn edit_rewrites_matches_in_place() {
    let dir = fixture(&[("a.rs", "fn main() {\n    dbg!(1);\n    dbg!(2);\n}\n")]);
    let out =
        ast_edit_apply(dir.path(), "dbg!($X)", "println!(\"$X\", $X)", Some("rust")).expect("edit");
    let after = fs::read_to_string(dir.path().join("a.rs")).expect("read");
    assert_eq!(
        after,
        "fn main() {\n    println!(\"1\", 1);\n    println!(\"2\", 2);\n}\n"
    );
    assert!(
        out.contains("1 file(s) changed, 2 match(es) replaced"),
        "{out}"
    );
    assert!(out.contains("+     println!(\"1\", 1);"), "{out}");
}

#[test]
fn edit_with_no_matches_changes_nothing() {
    let dir = fixture(&[("a.rs", "fn main() {}\n")]);
    let before = fs::read_to_string(dir.path().join("a.rs")).expect("read");
    let out = ast_edit_apply(dir.path(), "dbg!($X)", "x", Some("rust")).expect("edit");
    let after = fs::read_to_string(dir.path().join("a.rs")).expect("read");
    assert_eq!(before, after);
    assert_eq!(out, "no matches; nothing changed");
}

#[test]
fn security_scan_renders_findings_from_rules() {
    let dir = fixture(&[("a.rs", "fn main() {\n    let x = 1;\n}\n")]);
    let rules = "id: unused-var\nmessage: variable assigned but never used\nseverity: warning\nlanguage: Rust\nrule:\n  pattern: let $X = $Y\n";
    let scanner = crate::AstGrep::new(Some(rules.to_string()));
    let findings = scanner.analyze(dir.path()).expect("analyze");
    assert_eq!(findings.len(), 1);
    assert_eq!(
        (
            findings[0].file.as_str(),
            findings[0].line,
            findings[0].rule.as_str()
        ),
        (
            dir.path().join("a.rs").display().to_string().as_str(),
            2,
            "unused-var"
        )
    );
    // security_scan itself runs the registry; semgrep may be absent, so only
    // assert the shape, not the backend list.
    let out = security_scan(dir.path(), None).expect("scan");
    assert!(!out.is_empty());
}

#[test]
fn plan_computes_without_writing_and_commit_applies() {
    let dir = fixture(&[("a.rs", "fn main() {\n    dbg!(1);\n}\n")]);
    let plan =
        crate::tools::ast_edit_plan(dir.path(), "dbg!($X)", "println!(\"$X\", $X)", Some("rust"))
            .expect("plan");
    let before = fs::read_to_string(dir.path().join("a.rs")).expect("read");
    assert_eq!(before, "fn main() {\n    dbg!(1);\n}\n");
    assert_eq!(plan.changes.len(), 1);
    let out = crate::tools::ast_edit_commit(&plan).expect("commit");
    let after = fs::read_to_string(dir.path().join("a.rs")).expect("read");
    assert_eq!(after, "fn main() {\n    println!(\"1\", 1);\n}\n");
    assert!(out.contains("1 file(s) changed"), "{out}");
}

#[test]
fn security_scan_refuses_a_scope_outside_the_repo() {
    let dir = fixture(&[("a.rs", "fn main() {}\n")]);
    let outside = tempfile::tempdir().expect("tempdir");
    let err = security_scan(dir.path(), Some(outside.path())).expect_err("must refuse");
    assert!(
        err.to_string().contains("outside the repository"),
        "{err:#}"
    );
}

#[test]
fn security_scan_refuses_a_missing_scope() {
    let dir = fixture(&[("a.rs", "fn main() {}\n")]);
    let err =
        security_scan(dir.path(), Some(std::path::Path::new("nope/"))).expect_err("must refuse");
    assert!(err.to_string().contains("does not exist"), "{err:#}");
}
