use super::*;

fn snapshot(prompt: u64, completion: u64) -> UsageSnapshot {
    UsageSnapshot {
        prompt_tokens: prompt,
        completion_tokens: completion,
        total_tokens: prompt + completion,
        requests: 1,
        estimated_cost_usd: None,
        cost_is_estimate: false,
        estimated: false,
    }
}

#[test]
fn round_usage_is_the_delta_between_snapshots() {
    let usage = round_usage(snapshot(100, 10), snapshot(450, 35)).unwrap();
    assert_eq!(usage.prompt_tokens, 350);
    assert_eq!(usage.completion_tokens, 25);
}

#[test]
fn round_usage_is_none_when_the_counter_did_not_move() {
    assert!(round_usage(snapshot(100, 10), snapshot(100, 10)).is_none());
}

#[tokio::test]
async fn explore_runs_mixed_lookups_in_one_call() {
    let repo = tempfile::tempdir().unwrap();
    std::fs::write(repo.path().join("a.rs"), "fn needle() {}\n").unwrap();
    std::fs::write(repo.path().join("b.rs"), "mod other;\n").unwrap();
    let out = run_tool_with(
        repo.path(),
        &mut false,
        &mut Policy::permissive(),
        None,
        &SessionCtx::default(),
        "explore",
        json!({ "steps": [
            { "tool": "read_file", "args": { "path": "b.rs" } },
            { "tool": "search_files", "args": { "query": "needle" } },
        ]}),
    )
    .await;
    assert!(out.contains("[1] read_file b.rs"), "{out}");
    assert!(out.contains("mod other;"), "{out}");
    assert!(out.contains("[2] search_files needle"), "{out}");
    assert!(out.contains("a.rs"), "{out}");
}

#[tokio::test]
async fn explore_reports_steps_in_the_order_they_were_sent() {
    let repo = tempfile::tempdir().unwrap();
    for name in ["one.rs", "two.rs", "three.rs"] {
        std::fs::write(repo.path().join(name), format!("// {name}\n")).unwrap();
    }
    let out = run_tool_with(
        repo.path(),
        &mut false,
        &mut Policy::permissive(),
        None,
        &SessionCtx::default(),
        "explore",
        json!({ "steps": [
            { "tool": "read_file", "args": { "path": "three.rs" } },
            { "tool": "read_file", "args": { "path": "one.rs" } },
            { "tool": "read_file", "args": { "path": "two.rs" } },
        ]}),
    )
    .await;
    let order: Vec<usize> = ["three.rs", "one.rs", "two.rs"]
        .iter()
        .map(|n| out.find(n).unwrap_or_else(|| panic!("{n} missing:\n{out}")))
        .collect();
    assert!(order.windows(2).all(|w| w[0] < w[1]), "{out}");
}

#[tokio::test]
async fn explore_without_steps_answers_with_the_shape_instead_of_failing() {
    let repo = tempfile::tempdir().unwrap();
    let out = run_tool_with(
        repo.path(),
        &mut false,
        &mut Policy::permissive(),
        None,
        &SessionCtx::default(),
        "explore",
        json!({}),
    )
    .await;
    assert!(!out.starts_with("error: "), "{out}");
    assert!(out.contains("`steps` array"), "{out}");
    assert!(out.contains("read_file"), "{out}");
}

#[tokio::test]
async fn explore_accepts_steps_as_a_json_string() {
    let repo = tempfile::tempdir().unwrap();
    std::fs::write(repo.path().join("a.rs"), "fn needle() {}\n").unwrap();
    let out = run_tool_with(
        repo.path(),
        &mut false,
        &mut Policy::permissive(),
        None,
        &SessionCtx::default(),
        "explore",
        json!({ "steps": "[{\"tool\":\"read_file\",\"args\":{\"path\":\"a.rs\"}}]" }),
    )
    .await;
    assert!(out.contains("needle"), "{out}");
}

#[tokio::test]
async fn explore_refuses_to_run_what_needs_the_sequential_path() {
    let repo = tempfile::tempdir().unwrap();
    let out = run_tool_with(
        repo.path(),
        &mut false,
        &mut Policy::permissive(),
        None,
        &SessionCtx::default(),
        "explore",
        json!({ "steps": [
            { "tool": "run_command", "args": { "command": "rm", "args": ["-rf", "/"] } },
            { "tool": "read_file", "args": { "path": "/etc/hosts" } },
        ]}),
    )
    .await;
    assert_eq!(out.matches("call it on its own").count(), 2, "{out}");
    assert!(out.contains("`run_command` is not a lookup"), "{out}");
    assert!(!out.contains("localhost"), "outside read leaked: {out}");
}

#[tokio::test]
async fn explore_reads_outside_the_repo_in_yolo() {
    let repo = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let path = outside.path().join("notes.txt");
    std::fs::write(&path, "outside the repo\n").unwrap();
    let ctx = SessionCtx {
        yolo: true,
        ..SessionCtx::default()
    };
    let out = run_tool_with(
        repo.path(),
        &mut false,
        &mut Policy::permissive(),
        None,
        &ctx,
        "explore",
        json!({ "steps": [
            { "tool": "read_file", "args": { "path": path.to_string_lossy() } },
        ]}),
    )
    .await;
    assert!(out.contains("outside the repo"), "{out}");
    assert!(!out.contains("on its own"), "{out}");
}

#[tokio::test]
async fn explore_names_the_tool_a_step_left_out() {
    let repo = tempfile::tempdir().unwrap();
    let out = run_tool_with(
        repo.path(),
        &mut false,
        &mut Policy::permissive(),
        None,
        &SessionCtx::default(),
        "explore",
        json!({ "steps": [{ "path": "a.rs" }] }),
    )
    .await;
    assert!(out.contains("no tool named"), "{out}");
    assert!(out.contains("read_file"), "{out}");
}

#[tokio::test]
async fn explore_takes_the_other_names_models_use_for_a_step() {
    let repo = tempfile::tempdir().unwrap();
    std::fs::write(repo.path().join("a.rs"), "fn only() {}\n").unwrap();
    let out = run_tool_with(
        repo.path(),
        &mut false,
        &mut Policy::permissive(),
        None,
        &SessionCtx::default(),
        "explore",
        json!({ "steps": [
            { "name": "read_file", "arguments": { "path": "a.rs" } },
            { "tool": "read_file", "args": "{\"path\": \"a.rs\"}" },
        ]}),
    )
    .await;
    assert!(out.contains("[1] read_file a.rs"), "{out}");
    assert!(out.contains("[2] read_file a.rs"), "{out}");
    assert!(!out.contains("on its own"), "{out}");
}

#[tokio::test]
async fn explore_with_empty_steps_suggests_a_single_lookup() {
    let repo = tempfile::tempdir().unwrap();
    let out = run_tool_with(
        repo.path(),
        &mut false,
        &mut Policy::permissive(),
        None,
        &SessionCtx::default(),
        "explore",
        json!({ "steps": [] }),
    )
    .await;
    assert!(!out.starts_with("error: "), "{out}");
    assert!(out.contains("empty"), "{out}");
}

#[tokio::test]
async fn a_failing_step_does_not_sink_the_others() {
    let repo = tempfile::tempdir().unwrap();
    std::fs::write(repo.path().join("real.rs"), "fn real() {}\n").unwrap();
    let out = run_tool_with(
        repo.path(),
        &mut false,
        &mut Policy::permissive(),
        None,
        &SessionCtx::default(),
        "explore",
        json!({ "steps": [
            { "tool": "read_file", "args": { "path": "nope.rs" } },
            { "tool": "read_file", "args": { "path": "real.rs" } },
        ]}),
    )
    .await;
    assert!(out.contains("does not exist"), "{out}");
    assert!(out.contains("fn real()"), "{out}");
}

#[test]
fn valid_arguments_are_left_alone() {
    let raw = r#"{"command":"bash","args":["-lc","echo \"hi\"\n"]}"#;
    let parsed = parse_arguments(raw).unwrap();
    assert_eq!(parsed["args"][1], "echo \"hi\"\n");
}

#[test]
fn a_shell_quote_escape_loses_the_backslash() {
    let raw = r#"{"args":["-lc","git diff -- \'crates/aster-cli/src/tui/\' | head"]}"#;
    let parsed = parse_arguments(raw).unwrap();
    assert_eq!(
        parsed["args"][1],
        "git diff -- 'crates/aster-cli/src/tui/' | head"
    );
}

#[test]
fn a_regex_escape_keeps_its_backslash() {
    let raw = r#"{"args":["-lc","grep -E '^\s*fn \w+' src"]}"#;
    let parsed = parse_arguments(raw).unwrap();
    assert_eq!(parsed["args"][1], r"grep -E '^\s*fn \w+' src");
}

#[test]
fn a_backslash_outside_a_string_is_still_a_syntax_error() {
    assert!(parse_arguments(r#"{"a": \1}"#).is_err());
}

#[test]
fn an_unrepairable_error_reports_what_the_model_sent() {
    let error = parse_arguments(r#"{"a": "b""#).unwrap_err().to_string();
    assert!(error.contains("EOF"), "{error}");
}

#[test]
fn an_escaped_quote_does_not_end_the_string_early() {
    let raw = r#"{"args":["say \"hi\" then \s"]}"#;
    assert_eq!(
        parse_arguments(raw).unwrap()["args"][0],
        r#"say "hi" then \s"#
    );
}

// A model that omits `command` used to lose the whole round to an error. The
// argv it did send is enough to run, so it runs.
#[test]
fn a_command_sent_as_a_list_is_split_into_binary_and_args() {
    let args = json!({ "command": ["bash", "-lc", "echo hi"] });
    assert_eq!(
        command_argv(&args),
        Some((
            "bash".to_string(),
            vec!["-lc".to_string(), "echo hi".to_string()]
        ))
    );
}

#[test]
fn a_binary_left_in_args_becomes_the_command() {
    let args = json!({ "args": ["npm", "run", "dev"] });
    assert_eq!(
        command_argv(&args),
        Some((
            "npm".to_string(),
            vec!["run".to_string(), "dev".to_string()]
        ))
    );
}

// A leading flag is not a binary, so it is treated as a flag for a shell and
// the rest of the argv becomes the line that shell runs.
#[test]
fn a_leading_flag_runs_through_a_shell() {
    assert_eq!(
        command_argv(&json!({ "args": ["-lc", "npm run dev"] })),
        Some((
            "bash".to_string(),
            vec!["-lc".to_string(), "npm run dev".to_string()]
        ))
    );
}

// A whole shell line left in `command` runs instead of erroring.
#[test]
fn a_shell_line_in_command_runs_through_bash() {
    assert_eq!(
        command_argv(&json!({ "command": "tail -n 50 SKILL.md | wc -l" })),
        Some((
            "bash".to_string(),
            vec!["-lc".to_string(), "tail -n 50 SKILL.md | wc -l".to_string()]
        ))
    );
}

#[test]
fn a_blank_command_falls_through_to_the_args() {
    let args = json!({ "command": "  ", "args": ["cargo", "test"] });
    assert_eq!(
        command_argv(&args),
        Some(("cargo".to_string(), vec!["test".to_string()]))
    );
}

#[test]
fn a_well_formed_call_is_left_alone() {
    let args = json!({ "command": "cargo", "args": ["test", ""] });
    assert_eq!(
        command_argv(&args),
        Some(("cargo".to_string(), vec!["test".to_string(), String::new()]))
    );
}

#[test]
fn an_unrecoverable_call_is_told_which_shape_to_send() {
    assert!(
        MISSING_COMMAND.contains("command:`bash` with args"),
        "{MISSING_COMMAND}"
    );
}

#[test]
fn read_window_caps_an_open_ended_read_and_says_where_to_resume() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("big.rs");
    let body: String = (1..=READ_WINDOW_LINES + 50)
        .map(|n| format!("line {n}\n"))
        .collect();
    std::fs::write(&path, body).unwrap();
    let out = read_numbered(&path, None, None).unwrap();
    assert!(out.contains(&format!("line {READ_WINDOW_LINES}")));
    assert!(!out.contains(&format!("line {}", READ_WINDOW_LINES + 1)));
    assert!(out.contains(&format!("start_line={}", READ_WINDOW_LINES + 1)));
}

#[test]
fn read_window_leaves_short_files_whole() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("small.rs");
    std::fs::write(&path, "one\ntwo\nthree\n").unwrap();
    let out = read_numbered(&path, None, None).unwrap();
    assert!(out.contains("three"));
    assert!(!out.contains("start_line="));
}

#[test]
fn a_repeat_read_of_an_unchanged_file_points_at_the_earlier_copy() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("stable.rs");
    std::fs::write(&path, "fn main() {}\n").unwrap();
    let ctx = SessionCtx::default();
    let first = cached_read(&ctx, &path, None, None).unwrap();
    assert!(first.contains("fn main"));
    let second = cached_read(&ctx, &path, None, None).unwrap();
    assert!(second.contains("unchanged since you read it"));
    assert!(!second.contains("fn main"));
}

#[test]
fn a_changed_file_is_read_again() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("edited.rs");
    std::fs::write(&path, "before\n").unwrap();
    let ctx = SessionCtx::default();
    cached_read(&ctx, &path, None, None).unwrap();
    // Rewind the recorded mtime rather than sleeping for the clock.
    if let Ok(mut reads) = ctx.reads.lock() {
        for value in reads.values_mut() {
            *value = Some(std::time::SystemTime::UNIX_EPOCH);
        }
    }
    std::fs::write(&path, "after\n").unwrap();
    let again = cached_read(&ctx, &path, None, None).unwrap();
    assert!(again.contains("after"));
}

#[test]
fn document_read_converts_rtf_to_markdown() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("note.rtf");
    std::fs::write(&path, r"{\rtf1\ansi Hello aster}").unwrap();
    let out = read_numbered(&path, None, None).unwrap();
    assert!(out.contains("Hello aster"), "{out}");
    assert!(!out.contains(r"\rtf1"), "{out}");
}

#[test]
fn document_read_leaves_csv_raw() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("data.csv");
    std::fs::write(&path, "a,b\n1,2\n").unwrap();
    let out = read_numbered(&path, None, None).unwrap();
    // The numbered line holds the raw bytes, not a rendered markdown table.
    assert!(out.contains("| a,b"), "{out}");
    assert!(!out.contains("---"), "{out}");
}

#[test]
fn document_read_reports_unknown_binary_files() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("blob.bin");
    std::fs::write(&path, [0xff, 0xfe, 0x00, 0x01, 0x80]).unwrap();
    let err = read_numbered(&path, None, None).unwrap_err();
    assert!(format!("{err:#}").contains("binary file"), "{err:#}");
}

#[test]
fn truncate_head_keeps_the_verdict_at_the_end() {
    let noisy = format!("{}FAILED: 2 tests", "warning\n".repeat(500));
    let kept = truncate_head(&noisy, 100);
    assert!(kept.ends_with("FAILED: 2 tests"));
    assert!(kept.starts_with("... [truncated]"));
}

#[test]
fn environment_note_finds_nested_bun_lockfile() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("editors/vscode")).unwrap();
    std::fs::write(dir.path().join("editors/vscode/bun.lock"), "").unwrap();
    let note = environment_note(dir.path()).expect("a note");
    assert!(note.contains("`bun`"));
    assert!(note.contains("editors/vscode"));
}

#[test]
fn environment_note_has_platform_and_date_without_lockfiles() {
    let dir = tempfile::tempdir().unwrap();
    let note = environment_note(dir.path()).expect("a note");
    assert!(note.contains("- Platform: "), "{note}");
    assert!(note.contains("- Today's date: "), "{note}");
    assert!(!note.contains("lockfile"), "{note}");
}

#[test]
fn environment_note_snapshots_git_state() {
    let dir = tempfile::tempdir().unwrap();
    let git = |args: &[&str]| {
        assert!(
            std::process::Command::new("git")
                .arg("-C")
                .arg(dir.path())
                .args(args)
                .env("GIT_AUTHOR_NAME", "t")
                .env("GIT_AUTHOR_EMAIL", "t@t")
                .env("GIT_COMMITTER_NAME", "t")
                .env("GIT_COMMITTER_EMAIL", "t@t")
                .output()
                .unwrap()
                .status
                .success()
        );
    };
    git(&["init", "-b", "trunk"]);
    std::fs::write(dir.path().join("a.txt"), "committed").unwrap();
    git(&["add", "a.txt"]);
    git(&["commit", "-m", "feat: first"]);
    std::fs::write(dir.path().join("b.txt"), "untracked").unwrap();

    let note = environment_note(dir.path()).expect("a note");
    assert!(note.contains("- Git branch: trunk"), "{note}");
    assert!(note.contains("feat: first"), "{note}");
    assert!(note.contains("?? b.txt"), "{note}");
}

fn generate_test_command_output(
    stdout: &str,
    stderr: &str,
    exit_code: i32,
) -> aster_sandbox::CommandOutput {
    aster_sandbox::CommandOutput {
        stdout: stdout.into(),
        stderr: stderr.into(),
        exit_code: Some(exit_code),
        timed_out: false,
    }
}

#[test]
fn command_coaching_flags_pipe_masked_build_failure() {
    let out = generate_test_command_output(
        "error[E0308]: mismatched types\n  --> src/main.rs:4:5\n",
        "",
        0,
    );
    let notes = command_coaching(&out, true);
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(notes[0].contains("exit code 0 comes from"), "{}", notes[0]);
    assert!(notes[0].contains("error[E0308]"), "{}", notes[0]);
}

#[test]
fn command_coaching_surfaces_first_error_on_failure() {
    let out = generate_test_command_output("", "warning: x\nerror: linker failed\n", 1);
    let notes = command_coaching(&out, true);
    assert!(notes[0].contains("first error"), "{notes:?}");
    assert!(notes[0].contains("linker failed"), "{notes:?}");
}

#[test]
fn command_coaching_marks_auth_failures_as_non_retryable() {
    let out = generate_test_command_output("", "Unauthorized. Please run 'railway login'\n", 1);
    let notes = command_coaching(&out, true);
    assert!(
        notes.iter().any(|n| n.contains("auth failure")),
        "{notes:?}"
    );
}

#[test]
fn command_coaching_names_the_sandbox_on_denials() {
    let out = generate_test_command_output(
        "",
        "error: bun is unable to write files to tempdir: PermissionDenied\n",
        1,
    );
    let notes = command_coaching(&out, true);
    assert!(notes.iter().any(|n| n.contains("sandbox")), "{notes:?}");
    let unsandboxed = command_coaching(&out, false);
    assert!(
        !unsandboxed.iter().any(|n| n.contains("sandbox")),
        "{unsandboxed:?}"
    );
}

#[test]
fn command_coaching_ignores_ssh_publickey_denials() {
    let out = generate_test_command_output("", "Permission denied (publickey).\n", 255);
    let notes = command_coaching(&out, true);
    assert!(!notes.iter().any(|n| n.contains("sandbox")), "{notes:?}");
}

#[test]
fn command_coaching_stays_quiet_on_clean_output() {
    let out = generate_test_command_output("all good\n", "", 0);
    assert!(command_coaching(&out, true).is_empty());
}

#[test]
fn limits_come_from_the_agent_block() {
    let agent = crate::settings::Agent {
        max_tool_rounds: Some(9),
        command_timeout_secs: Some(11),
        compact_budget_chars: Some(64_000),
    };
    let limits = Limits::resolve(&agent);
    assert_eq!(limits.max_tool_rounds, 9);
    assert_eq!(limits.command_timeout_secs, 11);
    assert_eq!(limits.compact_budget_chars, 64_000);
}

#[test]
fn limits_default_to_room_for_real_work() {
    let limits = Limits::default();
    assert!(limits.max_tool_rounds >= 60);
    assert!(limits.command_timeout_secs >= 120);
}

/// Bouncing "give me options" back to the model made it retry the tool in
/// a loop, so a question with nothing to pick declines instead.
#[tokio::test]
async fn a_question_without_options_declines_rather_than_asking_again() {
    let (tx, mut rx) = mpsc::channel::<UiRequest>(1);
    let result = ask_user(Some(&tx), "", "which one?", &[]).await.unwrap();
    assert!(result.contains("declined"), "{result}");
    assert!(rx.try_recv().is_err(), "the UI is never troubled");
}

/// One option is not a choice: it is answered without a round trip.
#[tokio::test]
async fn a_single_option_is_taken_without_asking() {
    let (tx, mut rx) = mpsc::channel::<UiRequest>(1);
    let opts = ["sqlite".to_string()];
    let result = ask_user(Some(&tx), "", "which one?", &opts).await.unwrap();
    assert!(result.contains("sqlite"), "{result}");
    assert!(rx.try_recv().is_err(), "the UI is never troubled");
}

fn args(path: &str, search: Option<&str>, replace: &str) -> Value {
    match search {
        Some(s) => json!({ "path": path, "search": s, "replace": replace }),
        None => json!({ "path": path, "replace": replace }),
    }
}

/// Unwraps the approval these tests expect; a question here is a bug.
fn approval(req: UiRequest) -> ApprovalRequest {
    match req {
        UiRequest::Approval(req) | UiRequest::PlanApproval(req) => req,
        UiRequest::Question(_) => panic!("expected an approval, got a question"),
    }
}

async fn run_tool(repo: &Path, name: &str, arguments: Value) -> String {
    exec_tool(
        repo,
        &mut false,
        &mut Policy::permissive(),
        &Grants::default(),
        None,
        name,
        &arguments.to_string(),
        &mut Vec::new(),
        &SessionCtx::default(),
        None,
    )
    .await
    .text
}

/// A policy in `plan`, the mode the plan tools are reached from.
fn plan_policy() -> Policy {
    Policy::compile(&aster_policy::PermissionsConfig {
        mode: aster_policy::Mode::Plan,
        ..Default::default()
    })
    .unwrap()
}

/// Runs a tool against a shared ctx, edit gate and policy, for the plan
/// tools whose whole point is the state they leave behind.
async fn run_tool_with(
    repo: &Path,
    allow_edits: &mut bool,
    policy: &mut Policy,
    approver: Option<&UiSender>,
    ctx: &SessionCtx,
    name: &str,
    arguments: Value,
) -> String {
    exec_tool(
        repo,
        allow_edits,
        policy,
        &Grants::default(),
        approver,
        name,
        &arguments.to_string(),
        &mut Vec::new(),
        ctx,
        None,
    )
    .await
    .text
}

#[tokio::test]
async fn read_only_call_matches_the_sequential_path() {
    let repo = tempfile::tempdir().unwrap();
    std::fs::write(repo.path().join("a.txt"), "one\ntwo\n").unwrap();
    // A fresh context per path: the same one would answer the second read
    // from its cache, which is the point of the cache, not a mismatch.
    let args = json!({ "path": "a.txt" });
    let parallel = read_only_call(
        repo.path(),
        &Policy::permissive(),
        &SessionCtx::default(),
        "read_file",
        &args.to_string(),
    )
    .unwrap();
    let sequential = run_tool_with(
        repo.path(),
        &mut false,
        &mut Policy::permissive(),
        None,
        &SessionCtx::default(),
        "read_file",
        args,
    )
    .await;
    assert_eq!(parallel, sequential);
}

#[test]
fn read_only_call_defers_outside_paths_and_stateful_tools() {
    let repo = tempfile::tempdir().unwrap();
    let ctx = SessionCtx::default();
    let policy = Policy::permissive();
    let outside = json!({ "path": "/etc/hosts" }).to_string();
    assert!(read_only_call(repo.path(), &policy, &ctx, "read_file", &outside).is_none());
    assert!(read_only_call(repo.path(), &policy, &ctx, "run_command", "{}").is_none());
    assert!(read_only_call(repo.path(), &policy, &ctx, "edit_file", "{}").is_none());
}

fn steps(pairs: &[(&str, &str)]) -> Value {
    json!({
        "steps": pairs
            .iter()
            .map(|(label, status)| json!({ "label": label, "status": status }))
            .collect::<Vec<_>>()
    })
}

#[tokio::test]
async fn update_plan_stores_every_step_with_its_status() {
    let repo = tempfile::tempdir().unwrap();
    let ctx = SessionCtx::default();
    let out = run_tool_with(
        repo.path(),
        &mut false,
        &mut Policy::permissive(),
        None,
        &ctx,
        "update_plan",
        steps(&[("read the code", "done"), ("write the fix", "in_progress")]),
    )
    .await;

    assert!(out.contains("✔ read the code"), "{out}");
    assert!(out.contains("◼ write the fix"), "{out}");
    assert!(
        out.contains("2 tasks (1 done, 1 in progress, 0 open)"),
        "{out}"
    );
    assert_eq!(ctx.plan.lock().unwrap().steps.len(), 2);
}

#[tokio::test]
async fn update_plan_replaces_rather_than_appends() {
    let repo = tempfile::tempdir().unwrap();
    let ctx = SessionCtx::default();
    for pairs in [
        &[("first", "pending")][..],
        &[("second", "pending"), ("third", "pending")][..],
    ] {
        run_tool_with(
            repo.path(),
            &mut false,
            &mut Policy::permissive(),
            None,
            &ctx,
            "update_plan",
            steps(pairs),
        )
        .await;
    }

    let plan = ctx.plan.lock().unwrap();
    assert_eq!(plan.steps.len(), 2, "the second call replaced the first");
    assert_eq!(plan.steps[0].label, "second");
}

#[tokio::test]
async fn update_plan_rejects_an_unknown_status() {
    let repo = tempfile::tempdir().unwrap();
    let ctx = SessionCtx::default();
    let out = run_tool_with(
        repo.path(),
        &mut false,
        &mut Policy::permissive(),
        None,
        &ctx,
        "update_plan",
        steps(&[("do it", "almost")]),
    )
    .await;

    assert!(out.starts_with("error:"), "{out}");
    assert!(
        out.contains("in_progress"),
        "the error lists valid ones: {out}"
    );
    assert!(
        ctx.plan.lock().unwrap().steps.is_empty(),
        "nothing was stored"
    );
}

#[tokio::test]
async fn update_plan_needs_at_least_one_step() {
    let repo = tempfile::tempdir().unwrap();
    let out = run_tool(repo.path(), "update_plan", json!({ "steps": [] })).await;
    assert!(out.starts_with("error:"), "{out}");
}

#[tokio::test]
async fn exit_plan_mode_needs_a_plan_first() {
    let repo = tempfile::tempdir().unwrap();
    let out = run_tool(repo.path(), "exit_plan_mode", json!({})).await;
    assert!(out.contains("update_plan"), "{out}");
}

#[tokio::test]
async fn approving_the_plan_unlocks_editing() {
    let repo = tempfile::tempdir().unwrap();
    let ctx = SessionCtx::default();
    let mut allow_edits = false;
    let mut policy = plan_policy();

    let (tx, mut rx) = mpsc::channel::<UiRequest>(1);
    let prompt = tokio::spawn(async move {
        let req = approval(rx.recv().await.unwrap());
        assert!(req.preview.contains("◻ ship it"), "{}", req.preview);
        let _ = req.respond.send(Answer::Yes);
    });

    run_tool_with(
        repo.path(),
        &mut allow_edits,
        &mut policy,
        None,
        &ctx,
        "update_plan",
        steps(&[("ship it", "pending")]),
    )
    .await;
    let out = run_tool_with(
        repo.path(),
        &mut allow_edits,
        &mut policy,
        Some(&tx),
        &ctx,
        "exit_plan_mode",
        json!({}),
    )
    .await;

    prompt.await.unwrap();
    assert!(out.contains("edit mode is now active"), "{out}");
    assert!(allow_edits, "approval promotes the turn to edit mode");
    assert_eq!(
        policy.mode(),
        aster_policy::Mode::Edit,
        "the policy has to follow the edit gate, or commands stay denied"
    );
}

#[tokio::test]
async fn approving_the_plan_lets_the_same_turn_run_commands() {
    let repo = tempfile::tempdir().unwrap();
    let ctx = SessionCtx::default();
    let mut allow_edits = false;
    let mut policy = plan_policy();

    let action = aster_policy::Action::Exec {
        binary: "cargo",
        args: &["test"],
    };
    assert!(
        matches!(
            policy.evaluate(&action),
            aster_policy::Decision::Deny { .. }
        ),
        "plan mode denies commands until the plan is approved"
    );

    let (tx, mut rx) = mpsc::channel::<UiRequest>(1);
    let prompt = tokio::spawn(async move {
        let _ = approval(rx.recv().await.unwrap()).respond.send(Answer::Yes);
    });

    run_tool_with(
        repo.path(),
        &mut allow_edits,
        &mut policy,
        None,
        &ctx,
        "update_plan",
        steps(&[("ship it", "pending")]),
    )
    .await;
    run_tool_with(
        repo.path(),
        &mut allow_edits,
        &mut policy,
        Some(&tx),
        &ctx,
        "exit_plan_mode",
        json!({}),
    )
    .await;
    prompt.await.unwrap();

    assert_eq!(policy.evaluate(&action), aster_policy::Decision::Allow);
}

#[tokio::test]
async fn rejecting_the_plan_leaves_editing_locked() {
    let repo = tempfile::tempdir().unwrap();
    let ctx = SessionCtx::default();
    let mut allow_edits = false;
    let mut policy = plan_policy();

    let (tx, mut rx) = mpsc::channel::<UiRequest>(1);
    let prompt = tokio::spawn(async move {
        let _ = approval(rx.recv().await.unwrap()).respond.send(Answer::No);
    });

    run_tool_with(
        repo.path(),
        &mut allow_edits,
        &mut policy,
        None,
        &ctx,
        "update_plan",
        steps(&[("ship it", "pending")]),
    )
    .await;
    let out = run_tool_with(
        repo.path(),
        &mut allow_edits,
        &mut policy,
        Some(&tx),
        &ctx,
        "exit_plan_mode",
        json!({}),
    )
    .await;

    prompt.await.unwrap();
    assert!(out.contains("stay in plan mode"), "{out}");
    assert!(!allow_edits);
    assert_eq!(policy.mode(), aster_policy::Mode::Plan);
}

#[tokio::test]
async fn exit_plan_mode_is_refused_once_already_editing() {
    let repo = tempfile::tempdir().unwrap();
    let out = run_tool_with(
        repo.path(),
        &mut true,
        &mut Policy::permissive(),
        None,
        &SessionCtx::default(),
        "exit_plan_mode",
        json!({}),
    )
    .await;
    assert!(out.contains("already in edit mode"), "{out}");
}

#[tokio::test]
async fn ask_user_relays_the_chosen_option() {
    let repo = tempfile::tempdir().unwrap();
    let (tx, mut rx) = mpsc::channel::<UiRequest>(1);
    let prompt = tokio::spawn(async move {
        let UiRequest::Question(req) = rx.recv().await.unwrap() else {
            panic!("expected a question");
        };
        assert_eq!(req.header, "Storage");
        assert_eq!(req.options, ["sqlite", "postgres"]);
        let _ = req.respond.send(Some("postgres".to_string()));
    });

    let out = run_tool_with(
        repo.path(),
        &mut false,
        &mut Policy::permissive(),
        Some(&tx),
        &SessionCtx::default(),
        "ask_user",
        json!({
            "header": "Storage",
            "question": "Which database?",
            "options": ["sqlite", "postgres"]
        }),
    )
    .await;

    prompt.await.unwrap();
    assert!(out.contains("postgres"), "{out}");
}

#[tokio::test]
async fn ask_user_tells_the_agent_to_decide_when_headless() {
    let repo = tempfile::tempdir().unwrap();
    let out = run_tool(
        repo.path(),
        "ask_user",
        json!({ "question": "Which database?", "options": ["sqlite"] }),
    )
    .await;

    assert!(
        !out.starts_with("error:"),
        "a missing UI is not an error: {out}"
    );
    assert!(out.contains("no interactive UI"), "{out}");
}

#[tokio::test]
async fn a_declined_question_does_not_stall_the_turn() {
    let repo = tempfile::tempdir().unwrap();
    let (tx, mut rx) = mpsc::channel::<UiRequest>(1);
    // Dropping the responder is how a dismissed picker answers.
    let prompt = tokio::spawn(async move { drop(rx.recv().await.unwrap()) });

    let out = run_tool_with(
        repo.path(),
        &mut false,
        &mut Policy::permissive(),
        Some(&tx),
        &SessionCtx::default(),
        "ask_user",
        json!({ "question": "Which database?", "options": ["sqlite", "postgres"] }),
    )
    .await;

    prompt.await.unwrap();
    assert!(out.contains("declined"), "{out}");
}

fn sample_repo() -> tempfile::TempDir {
    let repo = tempfile::tempdir().unwrap();
    fs::create_dir_all(repo.path().join("crates/aster-cli/src/tui")).unwrap();
    fs::write(
        repo.path().join("crates/aster-cli/src/tui/composer.rs"),
        "fn compose() {}\n",
    )
    .unwrap();
    repo
}

#[tokio::test]
async fn a_missing_read_path_suggests_real_ones_instead_of_failing() {
    let repo = sample_repo();

    let out = run_tool(
        repo.path(),
        "read_file",
        json!({ "path": "crates/ui/src/composer.rs" }),
    )
    .await;

    assert!(!out.starts_with("error: "), "{out}");
    assert!(
        out.contains("crates/aster-cli/src/tui/composer.rs"),
        "{out}"
    );
}

#[tokio::test]
async fn a_missing_search_dir_widens_to_the_whole_repo() {
    let repo = sample_repo();

    let out = run_tool(
        repo.path(),
        "search_files",
        json!({ "query": "compose", "dir": "crates/aster-tui" }),
    )
    .await;

    assert!(
        out.starts_with("note: crates/aster-tui does not exist"),
        "{out}"
    );
    assert!(out.contains("composer.rs"), "{out}");
}

#[tokio::test]
async fn find_files_locates_a_file_by_name() {
    let repo = sample_repo();

    let out = run_tool(
        repo.path(),
        "find_files",
        json!({ "pattern": "composer.rs" }),
    )
    .await;

    assert_eq!(out, "crates/aster-cli/src/tui/composer.rs");
}

#[tokio::test]
async fn an_unknown_tool_names_the_real_ones() {
    let repo = tempfile::tempdir().unwrap();

    let out = run_tool(repo.path(), "search_file", json!({ "query": "x" })).await;

    assert!(out.starts_with("error: unknown tool: search_file"), "{out}");
    assert!(out.contains("search_files"), "{out}");
    assert!(out.contains("find_files"), "{out}");
}

#[tokio::test]
async fn edit_file_creates_a_missing_file_without_search() {
    let repo = tempfile::tempdir().unwrap();
    let policy = Policy::permissive();
    let mut edited = Vec::new();

    let out = edit_file(
        repo.path(),
        &policy,
        None,
        &SessionCtx::default(),
        &args("docs/notes/test.md", None, "# Test\n"),
        &mut edited,
    )
    .await
    .unwrap();

    assert!(out.starts_with("created docs/notes/test.md"), "{out}");
    assert_eq!(
        fs::read_to_string(repo.path().join("docs/notes/test.md")).unwrap(),
        "# Test\n"
    );
    assert_eq!(edited, ["docs/notes/test.md"]);
}

#[tokio::test]
async fn outside_reads_are_approved_by_the_front_end() {
    let repo = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let target = outside.path().join("notes.txt");
    fs::write(&target, "hello").unwrap();
    let policy = Policy::permissive();

    let (tx, mut rx) = mpsc::channel::<UiRequest>(1);
    let answer = tokio::spawn(async move {
        let req = approval(rx.recv().await.unwrap());
        assert!(
            req.preview.contains("outside the repository"),
            "{}",
            req.preview
        );
        let _ = req.respond.send(Answer::Yes);
    });

    let resolved = resolve_for_read(
        repo.path(),
        &policy,
        &Grants::default(),
        Some(&tx),
        &SessionCtx::default(),
        &target.to_string_lossy(),
    )
    .await
    .unwrap();

    answer.await.unwrap();
    assert_eq!(resolved, target.canonicalize().unwrap());
}

#[tokio::test]
async fn a_grant_covers_the_rest_of_the_directory() {
    let repo = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("a.txt"), "a").unwrap();
    fs::write(outside.path().join("b.txt"), "b").unwrap();
    let policy = Policy::permissive();
    let grants = Grants::default();

    let (tx, mut rx) = mpsc::channel::<UiRequest>(1);
    let prompts = tokio::spawn(async move {
        let mut seen = 0;
        while let Some(req) = rx.recv().await {
            seen += 1;
            let _ = approval(req).respond.send(Answer::Yes);
        }
        seen
    });

    for name in ["a.txt", "b.txt"] {
        let path = outside.path().join(name);
        resolve_for_read(
            repo.path(),
            &policy,
            &grants,
            Some(&tx),
            &SessionCtx::default(),
            &path.to_string_lossy(),
        )
        .await
        .unwrap();
    }
    drop(tx);

    assert_eq!(
        prompts.await.unwrap(),
        1,
        "the second read should be covered"
    );
    assert_eq!(grants.granted(), [outside.path().canonicalize().unwrap()]);
}

#[tokio::test]
async fn configured_directories_never_prompt() {
    let repo = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("a.txt"), "a").unwrap();

    let permissions = aster_policy::PermissionsConfig {
        additional_directories: vec![outside.path().to_string_lossy().into_owned()],
        ..Default::default()
    };

    let resolved = resolve_for_read(
        repo.path(),
        &Policy::permissive(),
        &configured_grants(&permissions, repo.path()),
        None,
        &SessionCtx::default(),
        &outside.path().join("a.txt").to_string_lossy(),
    )
    .await
    .unwrap();

    assert_eq!(
        resolved,
        outside.path().join("a.txt").canonicalize().unwrap()
    );
}

#[tokio::test]
async fn outside_reads_are_denied_without_an_approver() {
    let repo = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let target = outside.path().join("notes.txt");
    fs::write(&target, "hello").unwrap();

    let err = resolve_for_read(
        repo.path(),
        &Policy::permissive(),
        &Grants::default(),
        None,
        &SessionCtx::default(),
        &target.to_string_lossy(),
    )
    .await
    .unwrap_err()
    .to_string();

    assert!(err.contains("needs the user's approval"), "{err}");
}

/// The protected globs are repo-relative, so an absolute path to the same
/// file must not slip past them.
#[tokio::test]
async fn a_protected_file_stays_protected_through_an_absolute_path() {
    let repo = tempfile::tempdir().unwrap();
    let target = repo.path().join(".env");
    fs::write(&target, "SECRET=1").unwrap();
    let policy = Policy::compile(&aster_policy::PermissionsConfig {
        deny: vec!["Edit(.env)".into()],
        ..Default::default()
    })
    .unwrap();

    let err = edit_file(
        repo.path(),
        &policy,
        None,
        &SessionCtx::default(),
        &args(&target.to_string_lossy(), Some("SECRET=1"), "SECRET=2"),
        &mut Vec::new(),
    )
    .await
    .unwrap_err()
    .to_string();

    assert!(err.contains("blocked by policy"), "{err}");
    assert_eq!(fs::read_to_string(&target).unwrap(), "SECRET=1");
}

#[tokio::test]
async fn outside_writes_are_approved_by_the_front_end() {
    let repo = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let target = outside.path().join("notes.txt");
    fs::write(&target, "keep me").unwrap();
    let ctx = SessionCtx::default();

    let (tx, mut rx) = mpsc::channel::<UiRequest>(1);
    let answer = tokio::spawn(async move {
        let req = approval(rx.recv().await.unwrap());
        assert!(
            req.preview.contains("outside the repository"),
            "{}",
            req.preview
        );
        let _ = req.respond.send(Answer::Yes);
    });

    edit_file(
        repo.path(),
        &Policy::permissive(),
        Some(&tx),
        &ctx,
        &args(&target.to_string_lossy(), Some("keep me"), "changed"),
        &mut Vec::new(),
    )
    .await
    .unwrap();

    answer.await.unwrap();
    assert_eq!(fs::read_to_string(&target).unwrap(), "changed");
    assert_eq!(
        ctx.write_grants.granted(),
        [outside.path().canonicalize().unwrap()]
    );
}

#[tokio::test]
async fn outside_writes_are_denied_without_an_approver() {
    let repo = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let target = outside.path().join("notes.txt");
    fs::write(&target, "keep me").unwrap();

    let err = edit_file(
        repo.path(),
        &Policy::permissive(),
        None,
        &SessionCtx::default(),
        &args(&target.to_string_lossy(), Some("keep me"), "changed"),
        &mut Vec::new(),
    )
    .await
    .unwrap_err()
    .to_string();

    assert!(err.contains("needs the user's approval"), "{err}");
    assert_eq!(fs::read_to_string(&target).unwrap(), "keep me");
}

/// A read grant is not a write grant: approving `read_file` on a directory
/// must still leave `edit_file` asking.
#[tokio::test]
async fn a_read_grant_does_not_cover_a_write() {
    let repo = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let target = outside.path().join("notes.txt");
    fs::write(&target, "keep me").unwrap();
    let grants = Grants::new([outside.path().canonicalize().unwrap()]);

    resolve_for_read(
        repo.path(),
        &Policy::permissive(),
        &grants,
        None,
        &SessionCtx::default(),
        &target.to_string_lossy(),
    )
    .await
    .unwrap();

    let err = edit_file(
        repo.path(),
        &Policy::permissive(),
        None,
        &SessionCtx::default(),
        &args(&target.to_string_lossy(), Some("keep me"), "changed"),
        &mut Vec::new(),
    )
    .await
    .unwrap_err()
    .to_string();

    assert!(err.contains("needs the user's approval"), "{err}");
}

#[tokio::test]
async fn yolo_writes_outside_the_repo_without_asking() {
    let repo = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let target = outside.path().join("new.txt");
    let ctx = SessionCtx {
        yolo: true,
        ..SessionCtx::default()
    };

    edit_file(
        repo.path(),
        &Policy::permissive(),
        None,
        &ctx,
        &args(&target.to_string_lossy(), None, "written"),
        &mut Vec::new(),
    )
    .await
    .unwrap();

    assert_eq!(fs::read_to_string(&target).unwrap(), "written");
}

#[tokio::test]
async fn one_approval_covers_the_rest_of_the_directory() {
    let repo = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let ctx = SessionCtx::default();

    let (tx, mut rx) = mpsc::channel::<UiRequest>(1);
    let prompts = tokio::spawn(async move {
        let mut seen = 0;
        while let Some(req) = rx.recv().await {
            seen += 1;
            let _ = approval(req).respond.send(Answer::Yes);
        }
        seen
    });

    for name in ["a.txt", "b.txt"] {
        edit_file(
            repo.path(),
            &Policy::permissive(),
            Some(&tx),
            &ctx,
            &args(&outside.path().join(name).to_string_lossy(), None, "x"),
            &mut Vec::new(),
        )
        .await
        .unwrap();
    }
    drop(tx);

    assert_eq!(
        prompts.await.unwrap(),
        1,
        "the second write should be covered"
    );
}

#[tokio::test]
async fn edit_file_refuses_to_clobber_an_existing_file() {
    let repo = tempfile::tempdir().unwrap();
    fs::write(repo.path().join("test.md"), "keep me").unwrap();
    let policy = Policy::permissive();

    let err = edit_file(
        repo.path(),
        &policy,
        None,
        &SessionCtx::default(),
        &args("test.md", None, "gone"),
        &mut Vec::new(),
    )
    .await
    .unwrap_err()
    .to_string();

    assert!(err.contains("already exists"), "{err}");
    assert_eq!(
        fs::read_to_string(repo.path().join("test.md")).unwrap(),
        "keep me"
    );
}

#[test]
fn a_repeat_lookup_is_answered_with_a_pointer() {
    let ctx = SessionCtx::default();
    let args = r#"{"query":"emit","dir":"src"}"#;
    assert!(!is_repeat_lookup(&ctx, "search_files", args));
    assert!(is_repeat_lookup(&ctx, "search_files", args));
}

#[test]
fn different_arguments_are_not_a_repeat() {
    let ctx = SessionCtx::default();
    assert!(!is_repeat_lookup(&ctx, "search_files", r#"{"query":"a"}"#));
    assert!(!is_repeat_lookup(&ctx, "search_files", r#"{"query":"b"}"#));
}

#[test]
fn a_command_clears_the_lookup_cache() {
    let ctx = SessionCtx::default();
    let args = r#"{"pattern":"*.rs"}"#;
    assert!(!is_repeat_lookup(&ctx, "find_files", args));
    // A command may have created or deleted files, so the earlier answer is
    // no longer trustworthy and the repeat has to run for real.
    assert!(!is_repeat_lookup(&ctx, "run_command", "{}"));
    assert!(!is_repeat_lookup(&ctx, "find_files", args));
}

#[test]
fn read_file_is_left_to_its_own_mtime_cache() {
    let ctx = SessionCtx::default();
    let args = r#"{"path":"a.rs"}"#;
    assert!(!is_repeat_lookup(&ctx, "read_file", args));
    assert!(!is_repeat_lookup(&ctx, "read_file", args));
}

#[test]
fn no_progress_corrects_then_aborts_on_identical_rounds() {
    let mut np = NoProgress::default();
    assert!(matches!(np.feed(1, false, true), RoundVerdict::Continue));
    assert!(matches!(np.feed(1, false, true), RoundVerdict::Continue));
    assert!(matches!(np.feed(1, false, true), RoundVerdict::Correct));
    // Corrected once; another three identical rounds are the hard abort.
    assert!(matches!(np.feed(1, false, true), RoundVerdict::Continue));
    assert!(matches!(np.feed(1, false, true), RoundVerdict::Continue));
    assert!(matches!(np.feed(1, false, true), RoundVerdict::Abort));
}

#[test]
fn no_progress_corrects_then_aborts_on_error_storm() {
    let mut np = NoProgress::default();
    // Different failing calls still count as an error storm.
    assert!(matches!(np.feed(1, true, true), RoundVerdict::Continue));
    assert!(matches!(np.feed(2, true, true), RoundVerdict::Continue));
    assert!(matches!(np.feed(3, true, true), RoundVerdict::Correct));
    assert!(matches!(np.feed(4, true, true), RoundVerdict::Continue));
    assert!(matches!(np.feed(5, true, true), RoundVerdict::Continue));
    assert!(matches!(np.feed(6, true, true), RoundVerdict::Abort));
}

#[test]
fn no_progress_a_differing_round_resets_the_streak() {
    let mut np = NoProgress::default();
    assert!(matches!(np.feed(1, false, true), RoundVerdict::Continue));
    assert!(matches!(np.feed(1, false, true), RoundVerdict::Continue));
    // A new round breaks the streak.
    assert!(matches!(np.feed(2, false, true), RoundVerdict::Continue));
    assert!(matches!(np.feed(1, false, true), RoundVerdict::Continue));
    assert!(matches!(np.feed(1, false, true), RoundVerdict::Continue));
    assert!(matches!(np.feed(1, false, true), RoundVerdict::Correct));
}

#[test]
fn no_progress_a_good_round_resets_the_error_streak() {
    let mut np = NoProgress::default();
    assert!(matches!(np.feed(1, true, true), RoundVerdict::Continue));
    assert!(matches!(np.feed(2, true, true), RoundVerdict::Continue));
    // A round with a non-error result resets the storm.
    assert!(matches!(np.feed(3, false, true), RoundVerdict::Continue));
    assert!(matches!(np.feed(4, true, true), RoundVerdict::Continue));
    assert!(matches!(np.feed(5, true, true), RoundVerdict::Continue));
    assert!(matches!(np.feed(6, true, true), RoundVerdict::Correct));
}

/// The wandering case the repetition guard cannot see: every round differs, so
/// the signature never repeats, and nothing ever gets edited.
#[test]
fn no_progress_nudges_then_wraps_on_lookup_only_rounds() {
    let mut np = NoProgress::default();
    for round in 0..BARREN_ROUNDS - 1 {
        assert!(matches!(
            np.feed(round as u64, false, false),
            RoundVerdict::Continue
        ));
    }
    let nudged = np.feed(BARREN_ROUNDS as u64, false, false);
    assert!(matches!(nudged, RoundVerdict::Nudge(n) if n == BARREN_ROUNDS));
    // Nudged once; a second barren allotment ends the turn with an answer.
    for round in 0..BARREN_ROUNDS - 1 {
        assert!(matches!(
            np.feed(100 + round as u64, false, false),
            RoundVerdict::Continue
        ));
    }
    assert!(matches!(np.feed(999, false, false), RoundVerdict::Wrap));
}

#[test]
fn no_progress_acting_resets_the_barren_streak() {
    let mut np = NoProgress::default();
    for round in 0..BARREN_ROUNDS - 1 {
        assert!(matches!(
            np.feed(round as u64, false, false),
            RoundVerdict::Continue
        ));
    }
    // One edit clears the streak, so a long read before a change is fine.
    assert!(matches!(np.feed(50, false, true), RoundVerdict::Continue));
    for round in 0..BARREN_ROUNDS - 1 {
        assert!(matches!(
            np.feed(100 + round as u64, false, false),
            RoundVerdict::Continue
        ));
    }
    assert!(matches!(np.feed(999, false, false), RoundVerdict::Nudge(_)));
}

#[test]
fn a_round_of_only_lookups_is_not_productive() {
    let lookup = |name: &str| (name.to_string(), "{}".to_string(), "ok".to_string());
    assert!(!is_productive_round(&[
        lookup("search_files"),
        lookup("read_file"),
        lookup("explore"),
    ]));
    // One edit beside the reads makes the whole round count.
    assert!(is_productive_round(&[
        lookup("search_files"),
        lookup("edit_file"),
    ]));
    assert!(is_productive_round(&[lookup("run_command")]));
}

#[test]
fn budget_notice_names_what_is_left() {
    let notice = budget_notice(30, 60);
    assert!(notice.contains("30 tool rounds into this turn"));
    assert!(notice.contains("30 remain"));
}

#[test]
fn round_signature_hashes_name_args_and_result() {
    let a = vec![(
        "run_command".to_string(),
        r#"{"cmd":"ls"}"#.to_string(),
        "ok".to_string(),
    )];
    let b = vec![(
        "run_command".to_string(),
        r#"{"cmd":"ls"}"#.to_string(),
        "ok".to_string(),
    )];
    assert_eq!(round_signature(&a), round_signature(&b));
    let c = vec![(
        "run_command".to_string(),
        r#"{"cmd":"ls"}"#.to_string(),
        "error: boom".to_string(),
    )];
    assert_ne!(round_signature(&a), round_signature(&c));
}

#[test]
fn a_title_survives_the_wrappers_a_model_adds() {
    assert_eq!(
        clean_title("\"Fix the sandbox seccomp filter\"").unwrap(),
        "Fix the sandbox seccomp filter"
    );
    assert_eq!(
        clean_title("## Name the conversation.\nextra").unwrap(),
        "Name the conversation"
    );
    assert_eq!(
        clean_title("  `Rename chat sessions`  ").unwrap(),
        "Rename chat sessions"
    );
}

#[test]
fn a_rambling_or_empty_title_is_rejected() {
    assert!(clean_title("").is_none());
    assert!(clean_title("\"\"").is_none());
    assert!(clean_title(&"a".repeat(TITLE_MAX_CHARS + 1)).is_none());
}

#[test]
fn the_titler_sees_the_user_turns_in_full_and_clips_the_assistant() {
    let history = vec![
        ChatMessage {
            role: "user".into(),
            content: "how does naming work".into(),
        },
        ChatMessage {
            role: "assistant".into(),
            content: "x".repeat(900).into(),
        },
    ];
    let ctx = title_context(&history);
    assert!(ctx.contains("user: how does naming work"));
    assert!(ctx.contains("[truncated]"));
}

fn user(text: &str) -> ChatMessage {
    ChatMessage {
        role: "user".into(),
        content: text.into(),
    }
}

// A first prompt that already states the task is the whole topic. Waiting for
// a second turn left the session showing its opening line in the picker while
// the user was still working in it.
#[test]
fn an_opening_message_that_states_the_task_names_the_session_on_turn_one() {
    for opener in [
        "fix the sandbox seccomp filter",
        "add dark mode",
        "why is aster chat dropping the last token of every reply?",
        "使用者會話標題應該在第一次提問後就生成",
    ] {
        assert_eq!(turns_before_naming(&[user(opener)]), 1, "{opener}");
    }
}

#[test]
fn a_thin_opener_still_waits_for_the_turn_that_says_what_it_is_about() {
    for opener in ["hi", "Hey!", "help", "continue", "ok", "fix it", "why?", ""] {
        assert_eq!(turns_before_naming(&[user(opener)]), 2, "{opener:?}");
    }
}

// The greeting list matches a bare opener, not a prefix: a message that starts
// with "hi there" and then says what it wants is still nameable.
#[test]
fn a_greeting_followed_by_the_actual_task_names_the_session() {
    assert_eq!(
        turns_before_naming(&[user("hi there, can you fix the seccomp filter")]),
        1
    );
}

#[test]
fn a_history_with_no_user_turn_is_not_named_yet() {
    let history = vec![ChatMessage {
        role: "assistant".into(),
        content: "hello".into(),
    }];
    assert_eq!(turns_before_naming(&history), 2);
}

#[test]
fn environment_note_lists_task_runners_and_scripts() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("Justfile"), "build:\n\techo hi\n").unwrap();
    std::fs::write(
        dir.path().join("package.json"),
        r#"{"scripts":{"dev":"vite","build":"vite build","check":"tsc --noEmit"}}"#,
    )
    .unwrap();
    let note = environment_note(dir.path()).expect("a note");
    assert!(note.contains("Justfile present"), "{note}");
    assert_eq!(note.matches("just <name>").count(), 1, "{note}");
    assert!(note.contains("build, check, dev"), "{note}");
}

#[test]
fn credential_grants_are_seeded_from_the_configured_pairs() {
    let permissions = aster_policy::PermissionsConfig {
        allow_credentials: vec!["gh:~/.config/gh".into(), "aws : ~/.aws".into()],
        ..Default::default()
    };
    let grants = configured_credentials(&permissions, Path::new("/tmp/repo"));
    let home = dirs::home_dir().expect("a home directory");
    assert!(grants.allows("gh", &home.join(".config/gh")));
    // Whitespace around either half is tolerated.
    assert!(grants.allows("aws", &home.join(".aws")));
    // The pairing is the point: gh's approval is not aws's.
    assert!(!grants.allows("gh", &home.join(".aws")));
}

#[test]
fn a_malformed_credential_entry_is_dropped_not_fatal() {
    let permissions = aster_policy::PermissionsConfig {
        allow_credentials: vec!["no-colon-here".into(), "gh:~/.config/gh".into()],
        ..Default::default()
    };
    let grants = configured_credentials(&permissions, Path::new("/tmp/repo"));
    let home = dirs::home_dir().expect("a home directory");
    assert_eq!(grants.granted().len(), 1);
    assert!(grants.allows("gh", &home.join(".config/gh")));
}

/// A skills root holding one skill, so `/name` has something to resolve to.
fn skill_set(dir: &std::path::Path, name: &str) -> aster_skills::SkillSet {
    let root = dir.join("skills").join(name);
    std::fs::create_dir_all(&root).expect("dirs");
    std::fs::write(
        root.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: Does the thing.\n---\n\nBody.\n"),
    )
    .expect("skill");
    aster_skills::SkillSet::discover(&[dir.join("skills")])
}

#[test]
fn a_leading_skill_name_becomes_the_ask_that_applies_it() {
    let dir = tempfile::tempdir().expect("tempdir");
    let skills = skill_set(dir.path(), "write-tests");
    assert_eq!(
        expand_skill("/write-tests", &skills),
        "Use the \"write-tests\" skill:"
    );
}

#[test]
fn what_follows_the_name_is_carried_along_as_the_task() {
    let dir = tempfile::tempdir().expect("tempdir");
    let skills = skill_set(dir.path(), "write-tests");
    assert_eq!(
        expand_skill("/write-tests the parser", &skills),
        "Use the \"write-tests\" skill: the parser"
    );
}

#[test]
fn a_name_no_skill_answers_to_is_left_alone() {
    let dir = tempfile::tempdir().expect("tempdir");
    let skills = skill_set(dir.path(), "write-tests");
    assert_eq!(expand_skill("/compact", &skills), "/compact");
}

// A dropped path opens with a slash too, and is not an ask for anything.
#[test]
fn a_path_is_not_a_skill() {
    let dir = tempfile::tempdir().expect("tempdir");
    let skills = skill_set(dir.path(), "write-tests");
    assert_eq!(
        expand_skill("/Users/chizi/write-tests", &skills),
        "/Users/chizi/write-tests"
    );
}

fn msgs(n: usize) -> Vec<ChatMessage> {
    (0..n)
        .map(|i| ChatMessage {
            role: if i % 2 == 0 {
                "user".into()
            } else {
                "assistant".into()
            },
            content: format!("m{i}").into(),
        })
        .collect()
}

#[test]
fn a_history_no_longer_than_the_kept_tail_has_nothing_to_fold() {
    assert!(!can_compact(&msgs(COMPACT_KEEP_TAIL)));
}

#[test]
fn a_history_past_the_kept_tail_can_be_folded() {
    assert!(can_compact(&msgs(COMPACT_KEEP_TAIL + 1)));
}

#[test]
fn an_empty_history_cannot_be_folded() {
    assert!(!can_compact(&[]));
}

// The TUI, the VS Code panel, and the desktop app all render this arg in place
// of the command line. It went unasked for, so all three fell back forever.
#[test]
fn run_command_asks_for_the_description_every_surface_renders() {
    let tools = tool_defs(true, true);
    let run = tools
        .iter()
        .find(|t| t["function"]["name"] == "run_command")
        .expect("run_command is defined");
    let params = &run["function"]["parameters"];
    assert!(
        params["properties"]["description"].is_object(),
        "{params:#}"
    );
    assert_eq!(params["required"], json!(["command", "description"]));
}

// A preview the user cannot get back to is a preview they lose the moment the
// tab closes, so every surface needs the target to put in the reply.
#[test]
fn open_preview_is_offered_and_asks_for_a_target() {
    let tools = tool_defs(true, true);
    let preview = tools
        .iter()
        .find(|t| t["function"]["name"] == "open_preview")
        .expect("open_preview is defined");
    let params = &preview["function"]["parameters"];
    assert!(params["properties"]["target"].is_object(), "{params:#}");
    assert_eq!(params["required"], json!(["target"]));
}
