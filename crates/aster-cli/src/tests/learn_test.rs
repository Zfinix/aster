use super::*;

use std::io::Write;

fn score(rounds: usize, calls: usize, active: u64) -> RunScore {
    RunScore {
        rounds,
        calls,
        active_secs: active,
        wall_secs: active,
        ..RunScore::default()
    }
}

fn reflection(task: Option<&str>, body: &str) -> Reflection {
    Reflection {
        task: task.map(str::to_string),
        update: None,
        title: Some("Sudoku speedrun".into()),
        description: Some("Use when the user asks to solve a sudoku".into()),
        skill: Some(body.to_string()),
        waste: vec![
            "took a screenshot after every tap".into(),
            "retried the same tap".into(),
        ],
        facts: Vec::new(),
    }
}

fn home() -> (tempfile::TempDir, PathBuf, MemoryStore) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("skills");
    let memory = aster_persist::Store::open(dir.path()).unwrap().memory();
    (dir, root, memory)
}

fn learn(
    root: &Path,
    memory: &MemoryStore,
    session: &str,
    turn: usize,
    s: RunScore,
    r: &Reflection,
) -> Applied {
    apply(root, memory, session, turn, s, r).unwrap()
}

#[test]
fn a_first_run_writes_a_skill_the_loader_accepts() {
    let (_dir, root, memory) = home();
    let applied = learn(
        &root,
        &memory,
        "s1",
        1,
        score(59, 82, 300),
        &reflection(Some("sudoku-speedrun"), "1. OCR the board.\n2. Solve."),
    );
    assert_eq!(applied.outcome, Outcome::New);
    let raw = std::fs::read_to_string(root.join("sudoku-speedrun/SKILL.md")).unwrap();
    assert!(raw.starts_with("---\nname: sudoku-speedrun\ndescription: Use when the user asks to solve a sudoku\n---\n"), "{raw}");
    assert!(
        raw.contains(
            "# Sudoku speedrun\n\n> Best so far: 59 rounds, 82 calls, 300s active (session s1, "
        ),
        "{raw}"
    );
    assert!(raw.ends_with("1. OCR the board.\n2. Solve.\n"), "{raw}");
    let set = aster_skills::SkillSet::discover(std::slice::from_ref(&root));
    assert!(set.get("sudoku-speedrun").is_some());
    let runs = Ledger::at(&root, "sudoku-speedrun").load();
    assert_eq!(runs.len(), 1);
    assert!(runs[0].best);
}

#[test]
fn a_better_run_replaces_the_body_and_a_worse_one_leaves_a_lesson() {
    let (_dir, root, memory) = home();
    learn(
        &root,
        &memory,
        "s1",
        1,
        score(59, 82, 300),
        &reflection(Some("sudoku-speedrun"), "1. Old way."),
    );
    let better = learn(
        &root,
        &memory,
        "s2",
        1,
        score(40, 61, 210),
        &reflection(Some("sudoku-speedrun"), "1. Fast way."),
    );
    assert_eq!(better.outcome, Outcome::Improved);
    assert_eq!(better.best.unwrap().rounds, 59);
    let raw = std::fs::read_to_string(root.join("sudoku-speedrun/SKILL.md")).unwrap();
    assert!(raw.contains("1. Fast way."), "{raw}");
    assert!(!raw.contains("Old way"), "{raw}");
    assert!(raw.contains("> Best so far: 40 rounds"), "{raw}");

    let same = learn(
        &root,
        &memory,
        "s3",
        1,
        score(40, 61, 210),
        &reflection(Some("sudoku-speedrun"), "1. Fast way, refined."),
    );
    assert_eq!(same.outcome, Outcome::Updated);

    let worse = learn(
        &root,
        &memory,
        "s4",
        1,
        score(71, 90, 400),
        &reflection(Some("sudoku-speedrun"), "1. Worse way."),
    );
    assert_eq!(worse.outcome, Outcome::Regressed);
    let raw = std::fs::read_to_string(root.join("sudoku-speedrun/SKILL.md")).unwrap();
    assert!(raw.contains("1. Fast way, refined."), "{raw}");
    assert!(!raw.contains("Worse way"), "{raw}");
    assert!(raw.contains("> Best so far: 40 rounds"), "{raw}");
    assert!(raw.contains("## Lessons\n- "), "{raw}");
    assert!(raw.contains("regression, 71 rounds vs best 40: took a screenshot after every tap; retried the same tap"), "{raw}");
    let runs = Ledger::at(&root, "sudoku-speedrun").load();
    assert_eq!(runs.iter().filter(|r| r.best).count(), 3);
    assert!(!runs[3].best);
}

#[test]
fn lessons_are_capped_and_the_same_turn_is_never_learned_twice() {
    let (_dir, root, memory) = home();
    learn(
        &root,
        &memory,
        "s1",
        1,
        score(10, 12, 30),
        &reflection(Some("t"), "1. Do it."),
    );
    for i in 0..7 {
        learn(
            &root,
            &memory,
            &format!("w{i}"),
            1,
            score(20, 30, 90),
            &reflection(Some("t"), "x"),
        );
    }
    let raw = std::fs::read_to_string(root.join("t/SKILL.md")).unwrap();
    assert_eq!(raw.matches("\n- ").count(), LESSONS_KEPT, "{raw}");
    let again = learn(
        &root,
        &memory,
        "s1",
        1,
        score(5, 5, 5),
        &reflection(Some("t"), "1. Even faster."),
    );
    assert_eq!(again.outcome, Outcome::AlreadyLearned);
    assert!(
        !std::fs::read_to_string(root.join("t/SKILL.md"))
            .unwrap()
            .contains("Even faster")
    );
}

#[test]
fn the_model_never_owns_the_record_line_and_no_task_writes_nothing() {
    let (_dir, root, memory) = home();
    let body = "# Title\n\n> Best so far: 1 round. Beat it.\n\n1. Step.";
    learn(
        &root,
        &memory,
        "s1",
        1,
        score(9, 9, 9),
        &reflection(Some("t"), body),
    );
    let raw = std::fs::read_to_string(root.join("t/SKILL.md")).unwrap();
    assert_eq!(raw.matches("> Best so far:").count(), 1, "{raw}");
    assert!(raw.contains("> Best so far: 9 rounds"), "{raw}");
    assert!(raw.contains("# Title\n"), "{raw}");

    let none = learn(
        &root,
        &memory,
        "s2",
        1,
        score(9, 9, 9),
        &reflection(None, "whatever"),
    );
    assert!(matches!(none.outcome, Outcome::Skipped { .. }));
    assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
}

#[test]
fn an_update_rewrites_the_learned_skill_it_names_instead_of_a_new_one() {
    let (_dir, root, memory) = home();
    learn(
        &root,
        &memory,
        "s1",
        1,
        score(59, 82, 300),
        &reflection(Some("sudoku-speedrun"), "1. Old way."),
    );
    let mut r = reflection(Some("solve-sudoku"), "1. Fast way.");
    r.update = Some("sudoku-speedrun".into());
    let applied = learn(&root, &memory, "s2", 1, score(40, 61, 210), &r);
    assert_eq!(applied.outcome, Outcome::Improved);
    assert_eq!(
        applied.path.as_deref(),
        Some(root.join("sudoku-speedrun/SKILL.md").as_path())
    );
    let raw = std::fs::read_to_string(root.join("sudoku-speedrun/SKILL.md")).unwrap();
    assert!(raw.starts_with("---\nname: sudoku-speedrun\n"), "{raw}");
    assert!(raw.contains("1. Fast way."), "{raw}");
    assert!(!root.join("solve-sudoku").exists());
    assert_eq!(Ledger::at(&root, "sudoku-speedrun").load().len(), 2);
}

#[test]
fn an_update_naming_a_skill_this_loop_did_not_write_falls_back_to_a_new_one() {
    let (_dir, root, memory) = home();
    std::fs::create_dir_all(root.join("hand-written")).unwrap();
    std::fs::write(
        root.join("hand-written/SKILL.md"),
        "---\nname: hand-written\ndescription: Use when the user asks to hand-write\n---\n\n1. Mine.\n",
    )
    .unwrap();
    let mut r = reflection(Some("solve-sudoku"), "1. Fast way.");
    r.update = Some("hand-written".into());
    let applied = learn(&root, &memory, "s1", 1, score(40, 61, 210), &r);
    assert_eq!(applied.outcome, Outcome::New);
    let untouched = std::fs::read_to_string(root.join("hand-written/SKILL.md")).unwrap();
    assert!(untouched.contains("1. Mine."), "{untouched}");
    assert!(!untouched.contains("Fast way"), "{untouched}");
    assert!(root.join("solve-sudoku/SKILL.md").exists());
}

#[test]
fn the_reflection_runs_on_the_endpoint_the_session_used() {
    let meta = SessionMeta {
        id: "s1".into(),
        v: 1,
        created_at: Utc::now(),
        cwd: "/r".into(),
        repo_root: "/r".into(),
        model: Some("qwen3".into()),
        base_url: Some("http://127.0.0.1:11434/v1".into()),
        aster_version: None,
        title: None,
        schedule: None,
    };
    let client = session_client(&crate::settings::Settings::default(), &meta).unwrap();
    assert_eq!(client.base_url(), "http://127.0.0.1:11434/v1");
    assert_eq!(client.model, "qwen3");
}

#[test]
fn a_reflection_is_read_through_fences_and_prose() {
    let raw =
        "Sure! ```json\n{\"task\": \"Play 2048\", \"skill\": \"1. Swipe.\", \"waste\": []}\n```";
    let parsed = parse_reflection(raw).unwrap();
    assert_eq!(parsed.task.as_deref(), Some("play-2048"));
    assert_eq!(parsed.skill.as_deref(), Some("1. Swipe."));
    assert!(parsed.facts.is_empty());
    let none = parse_reflection("{\"task\": null}").unwrap();
    assert!(none.task.is_none());
    assert!(parse_reflection("no json here").is_err());
}

#[test]
fn the_digest_keeps_the_scored_turn_without_the_chat_plumbing() {
    let mut file = tempfile::Builder::new()
        .suffix(".jsonl")
        .tempfile()
        .unwrap();
    writeln!(file, r#"{{"type":"session","id":"s1","v":1,"created_at":"2026-08-03T09:00:00Z","cwd":"/r","repo_root":"/r"}}"#).unwrap();
    writeln!(file, r#"{{"type":"message","role":"user","content":"The user is talking to you through a Telegram chat.\n\n[msg 7]\nplay sudoku","ts":"2026-08-03T09:00:00Z"}}"#).unwrap();
    writeln!(file, r#"{{"type":"message","role":"assistant","tool_calls":[{{"id":"a","type":"function","function":{{"name":"run_command","arguments":"{{\"command\":\"asterctl\"}}"}}}}],"ts":"2026-08-03T09:00:01Z"}}"#).unwrap();
    writeln!(file, r#"{{"type":"message","role":"tool","tool_call_id":"a","content":"stdout:\nreceipt: posted (tap)\nchanged: +0 -0 pkg=x\n{}","ts":"2026-08-03T09:00:02Z"}}"#, "x".repeat(900)).unwrap();
    writeln!(
        file,
        r#"{{"type":"message","role":"assistant","content":"done","ts":"2026-08-03T09:00:03Z"}}"#
    )
    .unwrap();
    writeln!(file, r#"{{"type":"message","role":"user","content":"[msg 8]\nthanks","ts":"2026-08-03T09:01:00Z"}}"#).unwrap();
    writeln!(
        file,
        r#"{{"type":"message","role":"assistant","content":"welcome","ts":"2026-08-03T09:01:01Z"}}"#
    )
    .unwrap();
    file.flush().unwrap();
    let transcript = SessionTranscript::load(file.path()).unwrap();

    let (index, turn) = select_turn(&transcript, "1").unwrap();
    assert_eq!((index, turn.calls.len()), (1, 1));
    let digest = turn_digest(&transcript, 1);
    assert!(
        digest.starts_with("<user>play sudoku</user>\n<call n=1 tool=run_command>"),
        "{digest}"
    );
    assert!(digest.contains("changed: +0 -0 pkg=x"), "{digest}");
    assert!(!digest.contains("Telegram chat"), "{digest}");
    assert!(!digest.contains("thanks"), "{digest}");
    assert_eq!(select_turn(&transcript, "last").unwrap().0, 2);
    assert!(select_turn(&transcript, "9").is_err());

    let prompt = user_prompt(&score(3, 1, 3), &[], &digest);
    assert!(prompt.contains("- (none yet)"));
    assert!(!prompt.contains("PREVIOUS BEST"));
}
