use super::*;
use serde_json::json;

fn case() -> Case {
    Case {
        name: "finds the thing".into(),
        prompt: "where is it?".into(),
        must_mention: Some("is_skipped".into()),
        calls: vec!["search_files".into()],
        avoids: vec!["edit_file".into()],
        at_most: vec![("read_file".into(), 2)],
        max_rounds: Some(2),
    }
}

fn transcript(events: &[Value]) -> Transcript {
    let mut transcript = Transcript::default();
    for event in events {
        transcript.push(event);
    }
    transcript
}

fn call(name: &str) -> Value {
    json!({ "type": "tool_call", "id": name, "name": name, "arguments": "{}" })
}

fn result() -> Value {
    json!({ "type": "tool_result", "id": "x", "result": "ok" })
}

fn done(reply: &str) -> Value {
    json!({ "type": "done", "reply": reply, "usage": { "estimated_cost_usd": 0.01 } })
}

#[test]
fn calls_before_any_result_share_a_round() {
    let t = transcript(&[
        call("search_files"),
        call("read_file"),
        result(),
        result(),
        call("read_file"),
        result(),
        done("ok"),
    ]);
    assert_eq!(
        (t.tools.clone(), t.rounds, t.cost_usd, t.finished),
        (
            vec![
                "search_files".to_string(),
                "read_file".to_string(),
                "read_file".to_string()
            ],
            2,
            Some(0.01),
            true
        )
    );
}

#[test]
fn a_clean_run_passes() {
    let t = transcript(&[
        call("search_files"),
        result(),
        json!({ "type": "token", "content": "It is `IS_SKIPPED`." }),
        done(""),
    ]);
    assert_eq!(case().failures(&t), Vec::<String>::new());
}

#[test]
fn every_broken_assertion_is_named() {
    let t = transcript(&[
        call("read_file"),
        result(),
        call("read_file"),
        result(),
        call("read_file"),
        call("edit_file"),
        result(),
        done("no idea"),
    ]);
    assert_eq!(
        case().failures(&t),
        vec![
            "never called search_files",
            "called edit_file",
            "read_file called 3 times, limit 2",
            "3 rounds, limit 2",
            "answer never mentions \"is_skipped\"",
        ]
    );
}

#[test]
fn an_error_or_a_missing_answer_fails_the_case() {
    let errored = transcript(&[json!({ "type": "error", "message": "no credentials" })]);
    assert_eq!(
        case().failures(&errored)[0],
        "aster stopped: no credentials"
    );
    let silent = transcript(&[call("search_files")]);
    assert_eq!(
        case().failures(&silent)[0],
        "aster exited without an answer"
    );
}

fn outcome(case: &str, rounds: usize, failures: &[&str]) -> Outcome {
    Outcome {
        case: case.into(),
        failures: failures.iter().map(|f| f.to_string()).collect(),
        tools: vec!["read_file".into(); rounds],
        rounds,
        secs: 1.5,
        cost_usd: Some(0.01),
    }
}

#[test]
fn repeats_report_a_pass_count_and_each_failure() {
    let run = ModelRun {
        model: "glm".into(),
        outcomes: vec![
            outcome("batches", 1, &[]),
            outcome("batches", 3, &["3 rounds, limit 2"]),
            outcome("batches", 2, &[]),
        ],
    };
    let out = render_live(&[run]);
    assert!(out.contains("FAIL 2/3  batches"), "{out}");
    assert!(out.contains("rounds 2"), "{out}");
    assert!(out.contains("run 2: 3 rounds, limit 2"), "{out}");
    assert!(out.contains("$0.0300"), "{out}");
}

#[test]
fn a_model_with_no_reported_usage_is_never_shown_as_free() {
    let mut unpriced = outcome("a", 1, &[]);
    unpriced.cost_usd = None;
    let run = ModelRun {
        model: "kimi".into(),
        outcomes: vec![unpriced],
    };
    assert!(run.cost_usd().is_none());
    assert!(render_live(&[run]).contains('?'));
}

#[test]
fn the_sweep_names_the_passing_model_with_fewest_rounds() {
    let model = |name: &str, rounds: usize, failures: &[&str]| ModelRun {
        model: name.into(),
        outcomes: vec![outcome("a", rounds, failures)],
    };
    let out = render_live(&[
        model("chatty", 4, &[]),
        model("broken", 1, &["called edit_file"]),
        model("lean", 2, &[]),
    ]);
    assert!(
        out.contains("fewest rounds with every case passing: lean"),
        "{out}"
    );
}

#[cfg(unix)]
#[test]
fn a_case_runs_the_binary_and_reads_its_stream() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempfile::tempdir().unwrap();
    let bin = tmp.path().join("aster");
    let events = [
        call("search_files"),
        call("read_file"),
        result(),
        result(),
        done("it is is_skipped"),
    ]
    .iter()
    .map(|e| format!("echo '{e}'"))
    .collect::<Vec<_>>()
    .join("\n");
    std::fs::write(&bin, format!("#!/bin/sh\necho noise >&2\n{events}\n")).unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();

    let target = Target {
        bin,
        repo: tmp.path().to_path_buf(),
    };
    let outcome = run_case(&target, &case(), Some("m")).unwrap();
    assert_eq!(
        (outcome.failures, outcome.rounds, outcome.cost_usd),
        (Vec::<String>::new(), 1, Some(0.01))
    );
}

#[cfg(unix)]
#[test]
fn a_crash_reports_the_last_line_of_stderr() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempfile::tempdir().unwrap();
    let bin = tmp.path().join("aster");
    std::fs::write(&bin, "#!/bin/sh\necho 'no model configured' >&2\nexit 2\n").unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();

    let target = Target {
        bin,
        repo: tmp.path().to_path_buf(),
    };
    let outcome = run_case(&target, &case(), None).unwrap();
    assert_eq!(outcome.failures[0], "aster stopped: no model configured");
}

#[test]
fn repo_root_climbs_out_of_a_crate() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::create_dir_all(root.join(".git")).unwrap();
    let krate = root.join("crates/aster-eval");
    std::fs::create_dir_all(&krate).unwrap();

    assert_eq!(repo_root(&krate), root);
    assert_eq!(repo_root(root), root);
}

#[test]
fn repo_root_falls_back_to_the_directory_given() {
    let tmp = tempfile::tempdir().unwrap();
    let loose = tmp.path().join("no-git-here");
    std::fs::create_dir_all(&loose).unwrap();

    assert_eq!(repo_root(&loose), loose);
}
