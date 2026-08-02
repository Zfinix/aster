//! `run_tests`: detect the repo's test runner, run it sandboxed, and hand the
//! model structured results instead of raw stdout to guess at.

use std::path::Path;

use anyhow::{Result, bail};
use serde_json::{Value, json};

/// Combined stdout+stderr kept verbatim at the end of the structured result,
/// so the counts never hide the actual error text.
const TAIL_CHARS: usize = 2_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Runner {
    Cargo,
    Node,
    Pytest,
    Go,
}

impl Runner {
    fn name(self) -> &'static str {
        match self {
            Runner::Cargo => "cargo",
            Runner::Node => "node",
            Runner::Pytest => "pytest",
            Runner::Go => "go",
        }
    }
}

/// The command to run and how to read its output.
#[derive(Debug)]
pub(crate) struct TestCommand {
    pub runner: Runner,
    pub binary: String,
    pub args: Vec<String>,
}

/// Pick the runner from the repo's own manifests. An explicit `runner` wins;
/// detection order favours the manifest at the root.
pub(crate) fn detect(
    repo_root: &Path,
    runner: Option<&str>,
    filter: Option<&str>,
) -> Result<TestCommand> {
    let filter = filter.map(str::trim).filter(|f| !f.is_empty());
    match runner {
        Some("cargo") => Ok(cargo(filter)),
        Some("pytest") => Ok(pytest(filter)),
        Some("go") => Ok(go(filter)),
        Some(pm @ ("npm" | "bun" | "pnpm" | "yarn")) => Ok(node(pm, filter)),
        Some(other) => {
            bail!("unknown runner `{other}`; expected cargo, npm, bun, pnpm, yarn, pytest, or go")
        }
        None => {
            if repo_root.join("Cargo.toml").exists() {
                Ok(cargo(filter))
            } else if repo_root.join("package.json").exists() {
                Ok(node(node_package_manager(repo_root), filter))
            } else if repo_root.join("pyproject.toml").exists()
                || repo_root.join("pytest.ini").exists()
                || repo_root.join("setup.py").exists()
            {
                Ok(pytest(filter))
            } else if repo_root.join("go.mod").exists() {
                Ok(go(filter))
            } else {
                bail!(
                    "no test runner detected (looked for Cargo.toml, package.json, \
                     pyproject.toml, go.mod); pass `runner` explicitly"
                )
            }
        }
    }
}

fn node_package_manager(repo_root: &Path) -> &'static str {
    if repo_root.join("bun.lock").exists() || repo_root.join("bun.lockb").exists() {
        "bun"
    } else if repo_root.join("pnpm-lock.yaml").exists() {
        "pnpm"
    } else if repo_root.join("yarn.lock").exists() {
        "yarn"
    } else {
        "npm"
    }
}

fn cargo(filter: Option<&str>) -> TestCommand {
    let mut args = vec!["test".to_string()];
    if let Some(f) = filter {
        args.push(f.to_string());
    }
    TestCommand {
        runner: Runner::Cargo,
        binary: "cargo".into(),
        args,
    }
}

fn pytest(filter: Option<&str>) -> TestCommand {
    let mut args = vec!["-m".to_string(), "pytest".to_string()];
    if let Some(f) = filter {
        args.push("-k".to_string());
        args.push(f.to_string());
    }
    TestCommand {
        runner: Runner::Pytest,
        binary: "python3".into(),
        args,
    }
}

fn go(filter: Option<&str>) -> TestCommand {
    let mut args = vec!["test".to_string(), "./...".to_string()];
    if let Some(f) = filter {
        args.push("-run".to_string());
        args.push(f.to_string());
    }
    TestCommand {
        runner: Runner::Go,
        binary: "go".into(),
        args,
    }
}

fn node(pm: &str, filter: Option<&str>) -> TestCommand {
    let mut args = vec!["test".to_string()];
    if let Some(f) = filter {
        // npm needs `--` to forward to the underlying runner; the others take
        // the pattern directly and also tolerate the separator.
        if pm == "npm" {
            args.push("--".to_string());
        }
        args.push(f.to_string());
    }
    TestCommand {
        runner: Runner::Node,
        binary: pm.to_string(),
        args,
    }
}

/// Shape the raw output into counts, failing test names, and a verbatim tail.
/// Counts the parser cannot find stay null rather than pretending to be zero.
pub(crate) fn parse(runner: Runner, stdout: &str, stderr: &str, exit_code: i32) -> Value {
    let combined = format!("{stdout}\n{stderr}");
    let counts = match runner {
        Runner::Cargo => parse_cargo(&combined),
        Runner::Pytest => parse_pytest(&combined),
        Runner::Go => parse_go(&combined),
        Runner::Node => parse_node(&combined),
    };
    let tail: String = if combined.len() > TAIL_CHARS {
        format!("…{}", &combined[combined.len() - TAIL_CHARS..])
    } else {
        combined.trim().to_string()
    };
    json!({
        "runner": runner.name(),
        "exit_code": exit_code,
        "ok": exit_code == 0,
        "passed": counts.passed,
        "failed": counts.failed,
        "ignored": counts.ignored,
        "failures": counts.failures,
        "tail": tail,
    })
}

#[derive(Default)]
struct Counts {
    passed: Option<u64>,
    failed: Option<u64>,
    ignored: Option<u64>,
    failures: Vec<String>,
}

/// Sums every `test result:` line, one per suite, and collects the indented
/// names under each `failures:` block.
fn parse_cargo(out: &str) -> Counts {
    let mut counts = Counts::default();
    let mut in_failures = false;
    for line in out.lines() {
        let trimmed = line.trim();
        if trimmed == "failures:" {
            in_failures = true;
            continue;
        }
        if in_failures {
            if let Some(name) = line.strip_prefix("    ") {
                if !name.trim().is_empty() && !name.contains(' ') {
                    counts.failures.push(name.trim().to_string());
                }
                continue;
            }
            if !trimmed.is_empty() {
                in_failures = false;
            }
        }
        if let Some(rest) = trimmed.strip_prefix("test result:") {
            for part in rest.split(&[';', '.'][..]) {
                let part = part.trim();
                let mut words = part.split_whitespace();
                if let (Some(n), Some(kind)) = (words.next(), words.next())
                    && let Ok(n) = n.parse::<u64>()
                {
                    let slot = match kind {
                        "passed" => &mut counts.passed,
                        "failed" => &mut counts.failed,
                        "ignored" => &mut counts.ignored,
                        _ => continue,
                    };
                    *slot = Some(slot.unwrap_or(0) + n);
                }
            }
        }
    }
    counts.failures.sort();
    counts.failures.dedup();
    counts
}

/// Reads the `= X failed, Y passed in Zs =` summary and `FAILED name` lines.
fn parse_pytest(out: &str) -> Counts {
    let mut counts = Counts::default();
    for line in out.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("FAILED ") {
            let name = rest.split_whitespace().next().unwrap_or(rest);
            counts
                .failures
                .push(name.trim_end_matches(&['-', ' '][..]).to_string());
        }
        if trimmed.starts_with('=') && trimmed.ends_with('=') {
            for part in trimmed.trim_matches(&['=', ' '][..]).split(',') {
                let mut words = part.split_whitespace();
                if let (Some(n), Some(kind)) = (words.next(), words.next())
                    && let Ok(n) = n.parse::<u64>()
                {
                    match kind.trim_end_matches("in") {
                        "failed" => counts.failed = Some(n),
                        "passed" => counts.passed = Some(n),
                        "skipped" | "deselected" => {
                            counts.ignored = Some(counts.ignored.unwrap_or(0) + n)
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    counts
}

/// Collects `--- FAIL: name` lines; go has no per-test totals, so only the
/// failure count is derived.
fn parse_go(out: &str) -> Counts {
    let mut counts = Counts::default();
    for line in out.lines() {
        if let Some(rest) = line.trim().strip_prefix("--- FAIL: ") {
            let name = rest.split_whitespace().next().unwrap_or(rest);
            counts.failures.push(name.to_string());
        }
    }
    counts.failures.sort();
    counts.failures.dedup();
    if !counts.failures.is_empty() {
        counts.failed = Some(counts.failures.len() as u64);
    }
    counts
}

/// Best effort over jest and vitest summaries; other runners keep null counts
/// and let the exit code and tail speak.
fn parse_node(out: &str) -> Counts {
    let mut counts = Counts::default();
    for line in out.lines() {
        let trimmed = line.trim();
        let Some(rest) = trimmed
            .strip_prefix("Tests:")
            .or_else(|| trimmed.strip_prefix("Tests").filter(|r| r.starts_with(' ')))
        else {
            continue;
        };
        for part in rest.split(&[',', '|'][..]) {
            let mut words = part.split_whitespace();
            if let (Some(n), Some(kind)) = (words.next(), words.next())
                && let Ok(n) = n.parse::<u64>()
            {
                match kind {
                    "failed" => counts.failed = Some(n),
                    "passed" => counts.passed = Some(n),
                    "skipped" | "todo" => counts.ignored = Some(counts.ignored.unwrap_or(0) + n),
                    _ => {}
                }
            }
        }
    }
    counts
}

#[cfg(test)]
mod tests {
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
}
