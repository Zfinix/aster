use super::*;

use crate::turn::tests::{calls, load, reply, result, user};
use crate::turn::turns;

fn report(lines: &[String]) -> Report {
    Report::build(1, &turns(&load(lines)))
}

#[test]
fn batch_factor_is_calls_over_rounds() {
    let report = report(&[
        user("2026-08-03T09:00:00Z", "go"),
        calls(
            "2026-08-03T09:00:01Z",
            &[("a", "read_file"), ("b", "read_file")],
        ),
        result("2026-08-03T09:00:01Z", "a", "contents"),
        result("2026-08-03T09:00:01Z", "b", "contents"),
        calls("2026-08-03T09:00:02Z", &[("c", "read_file")]),
        result("2026-08-03T09:00:02Z", "c", "contents"),
    ]);
    assert_eq!(report.rounds, 2);
    assert_eq!(report.calls, 3);
    assert_eq!(report.batch_factor, 1.5);
    assert_eq!(report.single_call_rate, 0.5);
}

#[test]
fn barren_rate_counts_results_not_rounds() {
    let report = report(&[
        user("2026-08-03T09:00:00Z", "go"),
        calls(
            "2026-08-03T09:00:01Z",
            &[("a", "search_files"), ("b", "search_files")],
        ),
        result("2026-08-03T09:00:01Z", "a", "no matches"),
        result("2026-08-03T09:00:01Z", "b", "a.rs"),
    ]);
    assert_eq!(report.barren_rate, 0.5);
    let search = report
        .tools
        .iter()
        .find(|t| t.name == "search_files")
        .unwrap();
    assert_eq!(search.calls, 2);
    assert_eq!(search.barren, 1);
}

#[test]
fn tools_are_ranked_by_how_often_they_are_called() {
    let report = report(&[
        user("2026-08-03T09:00:00Z", "go"),
        calls(
            "2026-08-03T09:00:01Z",
            &[
                ("a", "search_files"),
                ("b", "search_files"),
                ("c", "read_file"),
            ],
        ),
        result("2026-08-03T09:00:01Z", "a", "hit"),
        result("2026-08-03T09:00:01Z", "b", "hit"),
        result("2026-08-03T09:00:01Z", "c", "contents"),
    ]);
    assert_eq!(report.tools[0].name, "search_files");
    assert_eq!(report.tools[1].name, "read_file");
}

#[test]
fn an_empty_sweep_reports_zeroes_rather_than_dividing_by_zero() {
    let report = Report::build(0, &[]);
    assert_eq!(report.batch_factor, 0.0);
    assert_eq!(report.barren_rate, 0.0);
    assert!(render(&report).contains("turns 0"));
}

#[test]
fn comparison_knows_which_direction_is_better() {
    let before = report(&[
        user("2026-08-03T09:00:00Z", "go"),
        calls("2026-08-03T09:00:01Z", &[("a", "read_file")]),
        result("2026-08-03T09:00:01Z", "a", "contents"),
    ]);
    let after = report(&[
        user("2026-08-03T09:00:00Z", "go"),
        calls(
            "2026-08-03T09:00:01Z",
            &[("a", "read_file"), ("b", "read_file")],
        ),
        result("2026-08-03T09:00:01Z", "a", "contents"),
        result("2026-08-03T09:00:01Z", "b", "contents"),
    ]);
    let deltas = after.compare(&before);
    let batch = deltas.iter().find(|d| d.metric == "batch factor").unwrap();
    assert_eq!((batch.before, batch.after), (1.0, 2.0));
    assert_eq!(batch.improved(), Some(true), "batching more is better");
    let single = deltas
        .iter()
        .find(|d| d.metric == "single-call rounds")
        .unwrap();
    assert_eq!(
        single.improved(),
        Some(true),
        "fewer single-call rounds is better"
    );
}

#[test]
fn a_metric_that_did_not_move_is_neither_better_nor_worse() {
    let lines = [
        user("2026-08-03T09:00:00Z", "go"),
        calls("2026-08-03T09:00:01Z", &[("a", "read_file")]),
        result("2026-08-03T09:00:01Z", "a", "contents"),
    ];
    let deltas = report(&lines).compare(&report(&lines));
    assert!(deltas.iter().all(|d| d.improved().is_none()), "{deltas:?}");
    assert!(render_comparison(&deltas).contains("unchanged"));
}

#[test]
fn model_spellings_fold_into_one_row() {
    for spelling in [
        "deepseek-v4-flash",
        "deepseek/deepseek-v4-flash",
        "~deepseek/deepseek-v4-flash",
        "DeepSeek-V4-Flash",
    ] {
        assert_eq!(canonical_model(spelling), "deepseek-v4-flash");
    }
    assert_eq!(canonical_model("z-ai/glm-5.2:free"), "glm-5.2:free");
}

#[test]
fn a_model_with_no_tool_rounds_is_left_off_the_table() {
    let report = report(&[
        user("2026-08-03T09:00:00Z", "go"),
        reply("2026-08-03T09:00:03Z", "answer"),
    ]);
    assert!(report.models.is_empty(), "{:?}", report.models);
}

#[test]
fn render_reports_every_headline() {
    let report = report(&[
        user("2026-08-03T09:00:00Z", "go"),
        calls("2026-08-03T09:00:01Z", &[("a", "search_files")]),
        result("2026-08-03T09:00:01Z", "a", "no matches"),
        reply("2026-08-03T09:00:03Z", "answer"),
    ]);
    let out = render(&report);
    for expected in [
        "batch factor",
        "rounds/turn",
        "model rtt",
        "barren results",
        "search_files",
    ] {
        assert!(out.contains(expected), "{expected} missing from:\n{out}");
    }
}
