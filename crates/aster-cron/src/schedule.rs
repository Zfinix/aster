//! Schedule definitions from `aster.yaml` and their validation.

use anyhow::{Context, Result};
use chrono::{Datelike, Timelike};

/// One entry under `schedules:` in `aster.yaml`.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Schedule {
    pub name: String,
    pub cron: String,
    pub agent: String,
    pub task: String,
    #[serde(default)]
    pub notify: bool,
}

/// Validate the whole `schedules` list: names must be unique and
/// lowercase-hyphen (the same rule agents follow), and every cron expression
/// must parse.
pub fn validate(schedules: &[Schedule]) -> Result<()> {
    let mut seen = std::collections::HashSet::new();
    for s in schedules {
        if !seen.insert(s.name.as_str()) {
            anyhow::bail!("duplicate schedule name: {}", s.name);
        }
        validate_name(&s.name)?;
        validate_cron(&s.cron)
            .with_context(|| format!("schedule {} has an invalid cron expression", s.name))?;
        if s.agent.trim().is_empty() {
            anyhow::bail!("schedule {} needs an agent", s.name);
        }
        if s.task.trim().is_empty() {
            anyhow::bail!("schedule {} needs a task", s.name);
        }
    }
    Ok(())
}

fn validate_name(name: &str) -> Result<()> {
    if name.len() > 64 {
        anyhow::bail!("schedule name exceeds 64 characters");
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        anyhow::bail!(
            "schedule name {name:?} must contain only lowercase letters, digits, and hyphens"
        );
    }
    Ok(())
}

/// A cron expression is valid when it expands to at least one calendar interval.
pub fn validate_cron(expr: &str) -> Result<()> {
    let intervals = calendar_intervals(expr)?;
    anyhow::ensure!(!intervals.is_empty(), "cron expression matches nothing");
    Ok(())
}

/// One calendar constraint, mirroring launchd's `StartCalendarInterval`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CalendarInterval {
    pub minute: u32,
    pub hour: u32,
    pub day: Option<u32>,
    pub month: Option<u32>,
    pub weekday: Option<u32>,
}

/// Parse a five-field cron expression into the calendar intervals it fires on.
/// Steps (`*/5`) and ranges (`1-5`) are rejected: launchd has no equivalent,
/// and firing at the wrong cadence is worse than refusing to install.
pub fn calendar_intervals(expr: &str) -> Result<Vec<CalendarInterval>> {
    let fields: Vec<&str> = expr.split_whitespace().collect();
    anyhow::ensure!(
        fields.len() == 5,
        "cron needs exactly 5 fields, got {}",
        fields.len()
    );

    let minutes = field_values(fields[0], 0, 59)?;
    let hours = field_values(fields[1], 0, 23)?;
    let days = field_values(fields[2], 1, 31)?;
    let months = field_values(fields[3], 1, 12)?;
    let weekdays = field_values(fields[4], 0, 6)?;

    let mut out = Vec::new();
    for &m in &minutes {
        for &h in &hours {
            for &d in &days {
                for &mo in &months {
                    for &w in &weekdays {
                        out.push(CalendarInterval {
                            minute: m,
                            hour: h,
                            day: (d != WILDCARD).then_some(d),
                            month: (mo != WILDCARD).then_some(mo),
                            weekday: (w != WILDCARD).then_some(w),
                        });
                    }
                }
            }
        }
    }
    // launchd rejects a plist that constrains both day-of-month and weekday.
    if out.iter().any(|c| c.day.is_some() && c.weekday.is_some()) {
        anyhow::bail!("cron may not constrain both day-of-month and weekday");
    }
    Ok(out)
}

const WILDCARD: u32 = u32::MAX;

fn field_values(field: &str, min: u32, max: u32) -> Result<Vec<u32>> {
    let field = field.trim();
    if field == "*" {
        return Ok(vec![WILDCARD]);
    }
    let mut out = Vec::new();
    for part in field.split(',') {
        if let Some(step) = part.strip_prefix("*/") {
            anyhow::bail!("cron steps are not supported: */{step}");
        }
        let value: u32 = part
            .parse()
            .with_context(|| format!("invalid cron field {field:?}"))?;
        anyhow::ensure!(
            (min..=max).contains(&value),
            "cron value {value} out of range {min}-{max}"
        );
        out.push(value);
    }
    Ok(out)
}

/// True when `expr` fires at `t` (local time), used by `aster cron list`.
pub fn matches_at(expr: &str, t: chrono::DateTime<chrono::Local>) -> bool {
    let Ok(intervals) = calendar_intervals(expr) else {
        return false;
    };
    let weekday = t.weekday().num_days_from_sunday();
    intervals.iter().any(|c| {
        c.minute == t.minute()
            && c.hour == t.hour()
            && c.day.is_none_or(|d| d == t.day())
            && c.month.is_none_or(|m| m == t.month())
            && c.weekday.is_none_or(|w| w == weekday)
    })
}

#[cfg(test)]
#[path = "tests/schedule_test.rs"]
mod tests;
