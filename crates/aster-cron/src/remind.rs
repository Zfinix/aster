//! One-shot reminders: `aster remind "text" "in 30m" | "at 18:00"`.
//! A reminder is a schedule with a count of one, not a second subsystem.

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Datelike, Duration, Local, Timelike};

/// Parse a reminder time: `in 30m`, `in 2h`, `in 1h30m`, or `at 18:00`
/// (today, or tomorrow if that time has already passed).
pub fn parse_when(when: &str) -> Result<DateTime<Local>> {
    let when = when.trim();
    if let Some(rest) = when.strip_prefix("in ") {
        return now_plus(&rest.trim().to_ascii_lowercase());
    }
    if let Some(rest) = when.strip_prefix("at ") {
        return at_time(rest.trim());
    }
    bail!("use \"in 30m\", \"in 2h\", or \"at 18:00\"")
}

fn now_plus(spec: &str) -> Result<DateTime<Local>> {
    let mut total = Duration::zero();
    let mut number = String::new();
    for c in spec.chars() {
        if c.is_ascii_digit() {
            number.push(c);
        } else {
            let mins: i64 = number
                .parse()
                .map_err(|_| anyhow::anyhow!("invalid duration {spec:?}"))?;
            number.clear();
            total += match c {
                'm' => Duration::minutes(mins),
                'h' => Duration::hours(mins),
                'd' => Duration::days(mins),
                _ => bail!("invalid duration unit {c:?} in {spec:?} (use m, h, or d)"),
            };
        }
    }
    anyhow::ensure!(number.is_empty(), "invalid duration {spec:?}");
    anyhow::ensure!(total > Duration::zero(), "reminder must be in the future");
    Ok(Local::now() + total)
}

fn at_time(hhmm: &str) -> Result<DateTime<Local>> {
    let Some((h, m)) = hhmm.split_once(':') else {
        bail!("invalid time {hhmm:?} (use HH:MM)");
    };
    let hour: u32 = h
        .parse()
        .map_err(|_| anyhow::anyhow!("invalid time {hhmm:?}"))?;
    let minute: u32 = m
        .parse()
        .map_err(|_| anyhow::anyhow!("invalid time {hhmm:?}"))?;
    anyhow::ensure!(hour < 24 && minute < 60, "invalid time {hhmm:?}");

    let now = Local::now();
    let mut target = now
        .with_hour(hour)
        .and_then(|t| t.with_minute(minute))
        .and_then(|t| t.with_second(0))
        .and_then(|t| t.with_nanosecond(0))
        .context("invalid time")?;
    if target <= now {
        target += Duration::days(1);
    }
    Ok(target)
}

/// The cron expression that fires once at `t`: a pinned minute, hour, day,
/// and month, every year. The fire command removes its own entry, so the
/// wildcard year never repeats.
pub fn one_shot_cron(t: DateTime<Local>) -> String {
    format!("{} {} {} {} *", t.minute(), t.hour(), t.day(), t.month())
}

/// The argv the reminder entry runs: post the notification, then remove the
/// schedule so it fires exactly once.
pub fn fire_args(aster_bin: &str, id: &str, text: &str) -> Vec<String> {
    vec![
        aster_bin.to_string(),
        "remind".to_string(),
        "--fire".to_string(),
        id.to_string(),
        "--text".to_string(),
        text.to_string(),
    ]
}

#[cfg(test)]
#[path = "tests/remind_test.rs"]
mod tests;
