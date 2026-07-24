use aster_analyzers::{AstGrep, Severity};

#[test]
fn embedded_astgrep_finds_matches_without_a_binary() {
    let base = std::env::temp_dir().join(format!("aster-ag-{}", std::process::id()));
    let src = base.join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(
        src.join("main.rs"),
        "fn a(s: &str) -> u16 { s.parse().unwrap() }\nfn b(v: &Vec<i32>) -> i32 { *v.first().unwrap() }\n",
    )
    .unwrap();

    let rules = "id: no-unwrap\nlanguage: rust\nseverity: warning\nmessage: avoid unwrap\nrule:\n  pattern: $X.unwrap()\n";

    let findings = AstGrep::default().scan(&base, rules).expect("scan");
    std::fs::remove_dir_all(&base).ok();

    assert_eq!(
        findings.len(),
        2,
        "expected 2 unwrap hits, got {findings:?}"
    );
    assert!(findings.iter().all(|f| f.rule == "no-unwrap"));
    assert!(
        findings
            .iter()
            .all(|f| matches!(f.severity, Severity::Warning))
    );
    assert!(findings.iter().any(|f| f.line == 1) && findings.iter().any(|f| f.line == 2));
}
