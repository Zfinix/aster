use super::*;

use std::io::Write;

pub(crate) fn load(lines: &[String]) -> SessionTranscript {
    let mut file = tempfile::Builder::new()
        .suffix(".jsonl")
        .tempfile()
        .unwrap();
    writeln!(
        file,
        r#"{{"type":"session","id":"s1","v":1,"created_at":"2026-08-03T09:00:00Z","cwd":"/r","repo_root":"/r","model":"test/model"}}"#
    )
    .unwrap();
    for line in lines {
        writeln!(file, "{line}").unwrap();
    }
    file.flush().unwrap();
    SessionTranscript::load(file.path()).unwrap()
}

pub(crate) fn user(at: &str, text: &str) -> String {
    format!(r#"{{"type":"message","role":"user","content":"{text}","ts":"{at}"}}"#)
}

pub(crate) fn reply(at: &str, text: &str) -> String {
    format!(r#"{{"type":"message","role":"assistant","content":"{text}","ts":"{at}"}}"#)
}

pub(crate) fn calls(at: &str, tools: &[(&str, &str)]) -> String {
    let calls: Vec<String> = tools
        .iter()
        .map(|(id, name)| {
            format!(
                r#"{{"id":"{id}","type":"function","function":{{"name":"{name}","arguments":"{{}}"}}}}"#
            )
        })
        .collect();
    format!(
        r#"{{"type":"message","role":"assistant","tool_calls":[{}],"ts":"{at}"}}"#,
        calls.join(",")
    )
}

pub(crate) fn result(at: &str, id: &str, content: &str) -> String {
    format!(
        r#"{{"type":"message","role":"tool","tool_call_id":"{id}","content":"{content}","ts":"{at}"}}"#
    )
}

#[test]
fn a_turn_runs_until_the_next_user_message() {
    let transcript = load(&[
        user("2026-08-03T09:00:00Z", "first"),
        reply("2026-08-03T09:00:02Z", "answer"),
        user("2026-08-03T09:01:00Z", "second"),
        reply("2026-08-03T09:01:03Z", "answer"),
    ]);
    let turns = turns(&transcript);
    assert_eq!(turns.len(), 2);
    assert_eq!(turns[0].wall(), 2.0);
    assert_eq!(turns[1].wall(), 3.0);
}

#[test]
fn batches_record_calls_per_round_not_per_call() {
    let transcript = load(&[
        user("2026-08-03T09:00:00Z", "go"),
        calls(
            "2026-08-03T09:00:01Z",
            &[("a", "read_file"), ("b", "read_file")],
        ),
        result("2026-08-03T09:00:01Z", "a", "contents"),
        result("2026-08-03T09:00:01Z", "b", "contents"),
        calls("2026-08-03T09:00:03Z", &[("c", "read_file")]),
        result("2026-08-03T09:00:03Z", "c", "contents"),
    ]);
    let turns = turns(&transcript);
    assert_eq!(turns[0].batches, vec![2, 1]);
    assert_eq!(turns[0].rounds(), 2);
    assert_eq!(turns[0].calls.len(), 3);
}

#[test]
fn latency_measures_the_model_not_the_tools() {
    let transcript = load(&[
        user("2026-08-03T09:00:00Z", "go"),
        // 2s of model time, then a tool that takes 5s.
        calls("2026-08-03T09:00:02Z", &[("a", "run_command")]),
        result("2026-08-03T09:00:07Z", "a", "done"),
        // 3s more of model time.
        reply("2026-08-03T09:00:10Z", "answer"),
    ]);
    let turns = turns(&transcript);
    assert_eq!(turns[0].latencies, vec![2.0, 3.0]);
    assert_eq!(turns[0].calls[0].duration, Some(5.0));
}

#[test]
fn results_that_answer_nothing_are_barren() {
    assert!(barren("no matches"));
    assert!(barren("no files matched"));
    assert!(barren("   "));
    assert!(barren(
        "note: src/nope.rs does not exist. Nearest paths:\n  src/yep.rs"
    ));
    assert!(!barren("a.rs\n> 3  needle"));
    assert!(
        !barren(
            "note: src/nope does not exist, so the whole repository was searched instead.\n\na.rs\n> 1  hit"
        ),
        "a widened search still returns hits"
    );
}

#[test]
fn a_human_wait_does_not_count_as_active_time() {
    let transcript = load(&[
        user("2026-08-03T09:00:00Z", "go"),
        calls("2026-08-03T09:00:01Z", &[("a", "ask_user")]),
        result("2026-08-03T09:10:01Z", "a", "yes"),
        reply("2026-08-03T09:10:03Z", "answer"),
    ]);
    let turns = turns(&transcript);
    assert_eq!(turns[0].wall(), 603.0);
    assert_eq!(turns[0].active(), 3.0);
}

#[test]
fn messages_before_the_first_user_message_belong_to_no_turn() {
    let transcript = load(&[
        reply("2026-08-03T09:00:00Z", "seeded"),
        user("2026-08-03T09:00:01Z", "go"),
        reply("2026-08-03T09:00:02Z", "answer"),
    ]);
    assert_eq!(turns(&transcript).len(), 1);
}

#[test]
fn a_result_with_no_matching_call_is_kept_as_unknown() {
    let transcript = load(&[
        user("2026-08-03T09:00:00Z", "go"),
        result("2026-08-03T09:00:01Z", "orphan", "contents"),
    ]);
    let turns = turns(&transcript);
    assert_eq!(turns[0].calls[0].tool, "unknown");
    assert_eq!(turns[0].calls[0].duration, None);
}
