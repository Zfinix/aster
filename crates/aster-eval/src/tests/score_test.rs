use super::*;

use crate::turn::tests::{calls_with_args, load, reply, result, user};
use crate::turn::turns;

const TAP: &str = r#"{"command":"asterctl","args":["tap","360,800"]}"#;
const SHOT: &str = r#"{"command":"asterctl","args":["shot"]}"#;
const MAP: &str = r#"{"command":"asterctl","args":["map"]}"#;

fn scored() -> RunScore {
    let transcript = load(&[
        user("2026-08-03T09:00:00Z", "play"),
        calls_with_args(
            "2026-08-03T09:00:01Z",
            &[("a", "run_command", MAP), ("b", "run_command", SHOT)],
        ),
        result("2026-08-03T09:00:02Z", "a", "stdout: pkg=x elements=3"),
        result(
            "2026-08-03T09:00:02Z",
            "b",
            "stdout: shot /c/1.png (720x720, 9 bytes)",
        ),
        calls_with_args(
            "2026-08-03T09:00:05Z",
            &[("c", "run_command", TAP), ("d", "run_command", TAP)],
        ),
        result(
            "2026-08-03T09:00:06Z",
            "c",
            "stdout: receipt: posted\\nchanged: +0 -0 pkg=x",
        ),
        result("2026-08-03T09:00:06Z", "d", "error: no element 4"),
        calls_with_args(
            "2026-08-03T09:00:08Z",
            &[
                ("e", "ask_user", "{}"),
                ("f", "run_command", SHOT),
                ("g", "search_files", "{}"),
            ],
        ),
        result("2026-08-03T09:00:18Z", "e", "yes"),
        result(
            "2026-08-03T09:00:18Z",
            "f",
            "stdout: shot /c/2.png (720x720, 9 bytes)",
        ),
        result("2026-08-03T09:00:18Z", "g", "no matches"),
        reply("2026-08-03T09:00:20Z", "done"),
    ]);
    let turns = turns(&transcript);
    RunScore::of(&turns[0])
}

#[test]
fn a_run_score_counts_every_kind_of_waste() {
    let score = scored();
    assert_eq!(score.rounds, 3);
    assert_eq!(score.calls, 7);
    assert_eq!(score.repeated, 2);
    assert_eq!(score.no_change, 1);
    assert_eq!(score.errors, 1);
    assert_eq!(score.screenshots, 2);
    assert_eq!(score.barren, 1);
    assert_eq!(score.wall_secs, 20);
    assert!(score.active_secs < score.wall_secs);
}

#[test]
fn better_means_fewer_rounds_then_calls_then_seconds() {
    let base = RunScore {
        rounds: 5,
        calls: 9,
        active_secs: 40,
        ..RunScore::default()
    };
    let fewer_rounds = RunScore {
        rounds: 4,
        calls: 20,
        active_secs: 90,
        ..RunScore::default()
    };
    let fewer_calls = RunScore {
        calls: 8,
        active_secs: 90,
        ..base
    };
    let quicker = RunScore {
        active_secs: 30,
        ..base
    };
    assert!(fewer_rounds.better_than(&base));
    assert!(fewer_calls.better_than(&base));
    assert!(quicker.better_than(&base));
    assert!(!base.better_than(&base));
}

#[test]
fn the_line_reads_as_one_sentence() {
    let line = scored().line();
    assert!(line.starts_with("3 rounds, 7 calls, "), "{line}");
    assert!(
        line.contains("1 errors, 1 no-change, 2 repeated, 2 shots"),
        "{line}"
    );
}
