use super::*;

fn case() -> Case {
    Case {
        name: "finds the thing".into(),
        prompt: "where is it?".into(),
        must_mention: Some("is_skipped".into()),
        calls: vec!["search_files".into()],
        avoids: vec!["edit_file".into()],
        at_most: vec![("find_files".into(), 2)],
    }
}

#[test]
fn every_assertion_reaches_the_generated_file() {
    let out = render_eval(&[case()], Some("z-ai/glm-5.2"));
    assert!(
        out.contains(r#"setupAgent({ model: "z-ai/glm-5.2" })"#),
        "{out}"
    );
    assert!(
        out.contains(r#"run.tool("search_files").toBeCalled()"#),
        "{out}"
    );
    assert!(
        out.contains(r#"run.tool("edit_file").toNotBeCalled()"#),
        "{out}"
    );
    assert!(out.contains("count(run, \"find_files\")"), "{out}");
    assert!(out.contains(".toBeLessThanOrEqual(2)"), "{out}");
    assert!(out.contains(r#"run.toMention("is_skipped")"#), "{out}");
    assert!(out.contains("run.toComplete()"), "{out}");
}

#[test]
fn without_a_model_the_workspace_default_is_used() {
    let out = render_eval(&[case()], None);
    assert!(out.contains("setupAgent()"), "{out}");
}

#[test]
fn a_prompt_with_quotes_stays_valid_typescript() {
    let mut awkward = case();
    awkward.prompt = r#"what does "activate" do? use `x`"#.into();
    awkward.name = "quotes \"inside\" the name".into();
    let out = render_eval(&[awkward], None);
    assert!(out.contains(r#"\"activate\""#), "{out}");
    assert!(!out.contains("test(\"quotes \"inside\""), "{out}");
}

#[test]
fn the_shipped_cases_render() {
    let out = render_eval(&default_cases(), Some("m"));
    assert_eq!(out.matches("agent.run(").count(), default_cases().len());
}

fn report(cases: &[(&str, &str, &str, Vec<&str>)]) -> serde_json::Value {
    serde_json::json!({
        "data": {
            "tests": cases.iter().map(|(name, status, _, _)| serde_json::json!({
                "name": name, "status": status, "durationMs": 1500.0,
            })).collect::<Vec<_>>(),
            "results": cases.iter().map(|(_, _, harness, tools)| serde_json::json!({
                "terminal": {
                    "harness": harness,
                    "payload": { "usage": { "costUsd": 0.01 } },
                },
                "toolCalls": tools,
            })).collect::<Vec<_>>(),
        }
    })
}

#[test]
fn case_names_and_tools_are_joined_across_both_arrays() {
    let run = summarise(
        "kimi",
        &report(&[
            ("finds it", "pass", "aster", vec!["find_files", "read_file"]),
            ("batches", "fail", "aster", vec!["read_file", "read_file"]),
        ]),
    );
    assert_eq!((run.passed(), run.failed()), (1, 1));
    assert_eq!(run.outcomes[0].case, "finds it");
    assert_eq!(run.outcomes[0].tool_summary(), "find_files×1 read_file×1");
    assert_eq!(run.outcomes[1].tool_summary(), "read_file×2");
    assert!(!run.ok());
}

#[test]
fn a_run_against_oris_own_agent_never_counts_as_a_pass() {
    // The feature failing to boot leaves Ori on its built-in harness, where
    // every assertion passes against the wrong subject.
    let run = summarise("kimi", &report(&[("finds it", "pass", "pi", vec!["bash"])]));
    assert_eq!((run.passed(), run.failed()), (0, 1));
    assert_eq!(run.outcomes[0].wrong_harness.as_deref(), Some("pi"));
}

#[test]
fn a_report_without_results_is_a_failure_not_an_empty_pass() {
    let run = summarise("kimi", &serde_json::json!({ "ok": true }));
    assert_eq!(run.passed(), 0);
    assert!(!run.ok());
}

#[test]
fn the_table_names_each_case_and_its_tools() {
    let runs = vec![summarise(
        "glm",
        &report(&[
            ("finds it", "pass", "aster", vec!["find_files"]),
            ("batches", "fail", "aster", vec!["read_file", "read_file"]),
        ]),
    )];
    let out = render_live(&runs);
    assert!(out.contains("pass finds it"), "{out}");
    assert!(out.contains("FAIL batches"), "{out}");
    assert!(out.contains("read_file×2"), "{out}");
    assert!(out.contains("glm       1     1"), "{out}");
    assert!(out.contains("$0.02"), "{out}");
}

#[test]
fn a_model_with_no_reported_usage_is_never_shown_as_free() {
    let report = serde_json::json!({
        "data": {
            "tests": [{ "name": "a", "status": "pass", "durationMs": 1000.0 }],
            "results": [{ "terminal": { "harness": "aster" }, "toolCalls": ["read_file"] }],
        }
    });
    let run = summarise("kimi", &report);
    assert!(run.cost_usd().is_none());
    assert!(render_live(&[run]).contains("?"));
}

#[test]
fn the_sweep_names_the_model_that_passed_with_fewest_calls() {
    let lean = summarise(
        "lean",
        &report(&[("a", "pass", "aster", vec!["read_file"])]),
    );
    let chatty = summarise(
        "chatty",
        &report(&[(
            "a",
            "pass",
            "aster",
            vec!["read_file", "read_file", "read_file"],
        )]),
    );
    let out = render_live(&[chatty, lean]);
    assert!(
        out.contains("fewest calls with every case passing: lean"),
        "{out}"
    );
}

#[test]
fn a_failing_model_never_wins_the_sweep() {
    let broken = summarise("broken", &report(&[("a", "fail", "aster", vec![])]));
    let working = summarise(
        "working",
        &report(&[("a", "pass", "aster", vec!["read_file", "read_file"])]),
    );
    let out = render_live(&[broken, working]);
    assert!(out.contains("passing: working"), "{out}");
}

#[test]
fn repo_root_climbs_out_of_the_eval_workspace() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::create_dir_all(root.join(".git")).unwrap();
    let evals = root.join("crates/aster-eval/evals");
    std::fs::create_dir_all(&evals).unwrap();

    assert_eq!(repo_root(&evals), root);
    assert_eq!(repo_root(root), root);
}

#[test]
fn repo_root_falls_back_to_the_directory_given() {
    let tmp = tempfile::tempdir().unwrap();
    let loose = tmp.path().join("no-git-here");
    std::fs::create_dir_all(&loose).unwrap();

    assert_eq!(repo_root(&loose), loose);
}
