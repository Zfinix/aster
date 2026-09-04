use crate::schedule::{calendar_intervals, matches_at, validate, validate_cron};

use anyhow::Result;

fn sched(name: &str, cron: &str) -> crate::Schedule {
    crate::Schedule {
        name: name.into(),
        cron: cron.into(),
        agent: "scout".into(),
        task: "look".into(),
        notify: false,
    }
}

#[test]
fn daily_at_nine_expands_to_one_interval() -> Result<()> {
    let intervals = calendar_intervals("0 9 * * *")?;
    assert_eq!(
        intervals,
        vec![crate::schedule::CalendarInterval {
            minute: 0,
            hour: 9,
            day: None,
            month: None,
            weekday: None,
        }]
    );
    Ok(())
}

#[test]
fn steps_and_ranges_are_rejected() {
    assert!(calendar_intervals("*/5 * * * *").is_err());
    assert!(calendar_intervals("1-5 * * * *").is_err());
    assert!(calendar_intervals("0 9 * *").is_err());
    assert!(calendar_intervals("99 * * * *").is_err());
}

#[test]
fn validate_cron_accepts_lists() {
    assert!(validate_cron("0 9,18 * * 1-5").is_err()); // ranges rejected
    assert!(validate_cron("0 9,18 * * *").is_ok());
}

#[test]
fn names_must_be_lowercase_hyphen() {
    assert!(validate(&[sched("nightly-review", "0 9 * * *")]).is_ok());
    assert!(validate(&[sched("Nightly", "0 9 * * *")]).is_err());
    assert!(validate(&[sched("a", "0 9 * * *"), sched("a", "0 10 * * *")]).is_err());
    assert!(validate(&[sched("ok", "bad cron")]).is_err());
}

#[test]
fn matches_at_respects_fields() {
    use chrono::TimeZone;
    let t = chrono::Local.with_ymd_and_hms(2026, 9, 4, 9, 0, 0).unwrap();
    assert!(matches_at("0 9 * * *", t));
    assert!(!matches_at("30 9 * * *", t));
}
