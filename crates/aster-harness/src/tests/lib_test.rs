use super::*;
use aster_models::Finding;

fn finding(file: &str, line: i32, category: &str, severity: &str, conf: f32) -> Finding {
    Finding {
        file_path: file.to_string(),
        line,
        start_line: None,
        side: Some("right".to_string()),
        severity: severity.to_string(),
        category: category.to_string(),
        title: format!("{category} @ {file}:{line}"),
        description: String::new(),
        suggestion: String::new(),
        code_snippet: None,
        confidence: Some(conf),
    }
}

#[test]
fn truncate_never_splits_utf8() {
    // Multi-byte chars; an arbitrary byte cut would panic without the boundary walk.
    let s = "é".repeat(1000);
    for max in [1, 2, 3, 101, 999] {
        let _ = truncate(&s, max);
    }
}

#[test]
fn salvage_candidates_recovers_complete_objects_from_truncated_array() {
    // Production failure shape: stream died after the second object, so array never closes.
    let truncated = r#"{"candidates":[{"file":"a.rs","line":208,"defect_class":"correctness","severity":"critical","title":"t1","failure_scenario":"s1","suggestion":"f1","code_snippet":"c1"},{"file":"b.rs","line":300,"defect_class":"correctness","severity":"critical","title":"t2","failure_scenario":"s2","suggestion":"f2"},{"file":"c.rs","line":1,"defect_class":"perf","severity":"low","title":"t3","failure_sc"#;
    let list = salvage_candidates(truncated).unwrap();
    assert_eq!(list.candidates.len(), 2);
    assert_eq!(list.candidates[0].file, "a.rs");
    assert_eq!(list.candidates[1].line, 300);
}

#[test]
fn salvage_candidates_ignores_braces_inside_strings() {
    let tricky = r#"{"candidates":[{"file":"a.rs","line":1,"defect_class":"x","severity":"low","title":"has } and { and \" quote","failure_scenario":"s","suggestion":"f"},{"file":"b.rs","#;
    let list = salvage_candidates(tricky).unwrap();
    assert_eq!(list.candidates.len(), 1);
    assert_eq!(list.candidates[0].title, "has } and { and \" quote");
}

#[test]
fn salvage_candidates_returns_none_when_nothing_whole() {
    assert!(salvage_candidates(r#"{"candidates":[{"file":"a.rs","li"#).is_none());
    assert!(salvage_candidates("not json at all").is_none());
}

#[test]
fn extract_json_strips_fences_and_prose() {
    let fenced = "```json\n{\"a\":1}\n```";
    assert_eq!(extract_json(fenced), "{\"a\":1}");
    let prose = "Sure, here it is: {\"a\":1} hope that helps";
    assert_eq!(extract_json(prose), "{\"a\":1}");
}

#[test]
fn diff_for_file_extracts_matching_section() {
    let diff = "diff --git a/src/foo.rs b/src/foo.rs\n\
                --- a/src/foo.rs\n\
                +++ b/src/foo.rs\n\
                @@ -1,2 +1,2 @@\n\
                -let x = 1;\n\
                +let x = 2;\n\
                diff --git a/src/bar.rs b/src/bar.rs\n\
                --- a/src/bar.rs\n\
                +++ b/src/bar.rs\n\
                @@ -1 +1 @@\n\
                +other\n";
    let foo = diff_for_file(diff, "src/foo.rs").unwrap();
    assert!(foo.contains("+let x = 2;"));
    assert!(!foo.contains("+other"));
    assert!(diff_for_file(diff, "src/missing.rs").is_none());
}

#[test]
fn diff_for_file_does_not_suffix_false_match() {
    let diff = "diff --git a/src/myfoo.rs b/src/myfoo.rs\n\
                --- a/src/myfoo.rs\n\
                +++ b/src/myfoo.rs\n\
                @@ -1 +1 @@\n\
                +wrong file\n";
    assert!(diff_for_file(diff, "foo.rs").is_none());
}

#[test]
fn diff_for_file_keeps_body_lines_that_look_like_headers() {
    // An added line `++ Heading` renders as `+++ Heading`; it is body, not a header.
    let diff = "diff --git a/README.md b/README.md\n\
                --- a/README.md\n\
                +++ b/README.md\n\
                @@ -1,2 +1,3 @@\n\
                 intro\n\
                +++ Installation\n\
                +done\n";
    let out = diff_for_file(diff, "README.md").unwrap();
    assert!(out.contains("+++ Installation"));
    assert!(out.contains("+done"));
}

#[test]
fn paths_match_respects_component_boundary() {
    assert!(paths_match("src/foo.rs", "foo.rs"));
    assert!(paths_match("foo.rs", "foo.rs"));
    assert!(!paths_match("src/myfoo.rs", "foo.rs"));
    assert!(!paths_match("foobar.rs", "bar.rs"));
}

#[test]
fn shape_report_keeps_distinct_findings_on_same_line() {
    let mut a = finding("a.rs", 10, "correctness", "high", 0.9);
    a.title = "off-by-one in loop bound".into();
    let mut b = finding("a.rs", 10, "correctness", "high", 0.9);
    b.title = "missing null check".into();
    let out = shape_report(vec![(0, a), (1, b)]);
    assert_eq!(out.len(), 2);
}

#[test]
fn shape_report_dedups_same_defect_keeps_higher_rank() {
    let ordered = vec![
        (0, finding("a.rs", 10, "correctness", "low", 0.6)),
        (1, finding("a.rs", 10, "correctness", "high", 0.9)),
        (2, finding("a.rs", 11, "correctness", "low", 0.5)),
    ];
    let out = shape_report(ordered);
    assert_eq!(out.len(), 2);
    assert_eq!(out[0].severity, "high");
}

#[test]
fn shape_report_merges_static_and_model_finding_on_same_defect() {
    let mut sast = finding("db.rs", 42, "semgrep", "high", 0.9);
    sast.title = "sql-injection".into();
    let mut llm = finding("db.rs", 42, "security", "critical", 0.8);
    llm.title = "SQL injection in query builder".into();
    let out = shape_report(vec![(0, sast), (1, llm)]);
    assert_eq!(out.len(), 1, "same defect from two sources reports once");
}

#[test]
fn titles_overlap_matches_same_defect_only() {
    assert!(titles_overlap(
        "sql-injection",
        "SQL injection in query builder"
    ));
    assert!(titles_overlap("panic on unwrap", "unwrap panic"));
    assert!(!titles_overlap(
        "off-by-one in loop bound",
        "missing null check"
    ));
    assert!(!titles_overlap("database error", "error handling"));
}

#[test]
fn is_common_ident_skips_noise_names() {
    assert!(is_common_ident("get"));
    assert!(is_common_ident("new"));
    assert!(is_common_ident("id"));
    assert!(is_common_ident("unwrap"));
    assert!(!is_common_ident("charge_customer"));
    assert!(!is_common_ident("validate_token"));
}

#[test]
fn shape_report_ranks_by_severity_at_equal_confidence() {
    let ordered = vec![
        (0, finding("a.rs", 1, "x", "low", 0.9)),
        (1, finding("b.rs", 1, "x", "critical", 0.9)),
        (2, finding("c.rs", 1, "x", "medium", 0.9)),
    ];
    let out = shape_report(ordered);
    assert_eq!(out[0].severity, "critical");
    assert_eq!(out[1].severity, "medium");
    assert_eq!(out[2].severity, "low");
}

#[test]
fn shape_report_confidence_can_outrank_severity() {
    // severity x confidence: critical 5*0.2=1.0 ranks below medium 3*0.9=2.7.
    let ordered = vec![
        (0, finding("a.rs", 1, "x", "critical", 0.2)),
        (1, finding("b.rs", 1, "x", "medium", 0.9)),
    ];
    let out = shape_report(ordered);
    assert_eq!(out[0].severity, "medium");
}

#[test]
fn is_simple_ident_rejects_operators_and_leading_digits() {
    assert!(is_simple_ident("foo_bar"));
    assert!(is_simple_ident("_private"));
    assert!(!is_simple_ident("1abc"));
    assert!(!is_simple_ident("a.b"));
    assert!(!is_simple_ident(""));
    assert!(!is_simple_ident("a(b)"));
}

#[test]
fn is_test_path_detects_common_conventions() {
    assert!(is_test_path("src/foo_test.rs"));
    assert!(is_test_path("tests/integration.rs"));
    assert!(is_test_path("src/__tests__/x.ts"));
    assert!(is_test_path("app/foo.spec.ts"));
    assert!(!is_test_path("src/foo.rs"));
}
