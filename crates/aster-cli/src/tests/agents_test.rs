use super::*;

#[test]
fn the_salvage_log_keeps_only_the_most_recent_lines() {
    let log = std::sync::Mutex::new(Vec::new());
    for n in 0..SALVAGE_LINES + 5 {
        push_salvage(&log, &format!("line {n}"));
    }
    let lines = log.lock().unwrap();
    assert_eq!(lines.len(), SALVAGE_LINES);
    assert_eq!(lines.first().map(String::as_str), Some("line 5"));
    assert_eq!(lines.last().map(String::as_str), Some("line 44"));
}

#[test]
fn a_timed_out_task_reports_its_trail_instead_of_nothing() {
    let report = salvage_report(300, "read_file src/chat.rs\nsearch_files queue").unwrap();
    assert!(report.contains("300s time limit"));
    assert!(report.contains("read_file src/chat.rs"));
}

#[test]
fn a_task_that_did_nothing_before_timing_out_has_no_salvage() {
    assert_eq!(salvage_report(300, ""), None);
}

#[test]
fn the_wrap_up_grace_scales_with_the_budget_within_bounds() {
    let secs = |s| std::time::Duration::from_secs(s);
    assert_eq!(wrap_up_grace(secs(30)), secs(15));
    assert_eq!(wrap_up_grace(secs(300)), secs(60));
    assert_eq!(wrap_up_grace(secs(150)), secs(30));
    assert_eq!(wrap_up_grace(secs(3600)), secs(60));
}
