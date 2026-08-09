use super::*;

#[test]
fn detects_cargo_from_manifest() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("Cargo.toml"), "[package]").unwrap();
    let cmd = detect(dir.path(), None, Some("budget")).unwrap();
    assert_eq!(cmd.runner, Runner::Cargo);
    assert_eq!(cmd.binary, "cargo");
    assert_eq!(cmd.args, vec!["test", "budget"]);
}

#[test]
fn detects_bun_from_lockfile() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("package.json"), "{}").unwrap();
    std::fs::write(dir.path().join("bun.lock"), "").unwrap();
    let cmd = detect(dir.path(), None, None).unwrap();
    assert_eq!(cmd.binary, "bun");
}

#[test]
fn npm_filter_goes_after_separator() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("package.json"), "{}").unwrap();
    let cmd = detect(dir.path(), None, Some("thread")).unwrap();
    assert_eq!(cmd.args, vec!["test", "--", "thread"]);
}

#[test]
fn unknown_runner_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    assert!(detect(dir.path(), Some("mvn"), None).is_err());
    assert!(detect(dir.path(), None, None).is_err());
}

#[test]
fn parses_cargo_output_across_suites() {
    let out = "\
running 3 tests
test budget::tests::a ... ok
test budget::tests::b ... FAILED
test budget::tests::c ... ok

failures:

---- budget::tests::b stdout ----
assertion failed

failures:
    budget::tests::b

test result: FAILED. 2 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out

running 1 test
test other ... ok

test result: ok. 1 passed; 0 failed; 3 ignored; 0 measured
";
    let v = parse(Runner::Cargo, out, "", 101);
    assert_eq!(v["passed"], 3);
    assert_eq!(v["failed"], 1);
    assert_eq!(v["ignored"], 3);
    assert_eq!(v["failures"][0], "budget::tests::b");
    assert_eq!(v["ok"], false);
}

#[test]
fn parses_pytest_summary() {
    let out = "\
FAILED tests/test_api.py::test_auth - AssertionError
==== 1 failed, 12 passed, 2 skipped in 3.21s ====
";
    let v = parse(Runner::Pytest, out, "", 1);
    assert_eq!(v["failed"], 1);
    assert_eq!(v["passed"], 12);
    assert_eq!(v["ignored"], 2);
    assert_eq!(v["failures"][0], "tests/test_api.py::test_auth");
}

#[test]
fn parses_go_failures() {
    let out = "\
--- FAIL: TestBudget (0.00s)
    budget_test.go:10: want 3, got 4
--- FAIL: TestEvict (0.01s)
ok  \texample.com/pkg\t0.2s
FAIL\texample.com/other\t0.3s
";
    let v = parse(Runner::Go, out, "", 1);
    assert_eq!(v["failed"], 2);
    assert_eq!(v["failures"][0], "TestBudget");
    assert!(v["passed"].is_null());
}

#[test]
fn parses_jest_summary() {
    let out = "Tests:       2 failed, 46 passed, 48 total";
    let v = parse(Runner::Node, out, "", 1);
    assert_eq!(v["failed"], 2);
    assert_eq!(v["passed"], 46);
}

#[test]
fn unparsed_counts_stay_null() {
    let v = parse(Runner::Node, "some unknown runner output", "", 0);
    assert!(v["passed"].is_null());
    assert!(v["failed"].is_null());
    assert_eq!(v["ok"], true);
}
