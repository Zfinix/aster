//! Tiny helpers shared across CLI commands.
use std::io;

use anyhow::Result;
use aster_ai::UsageSnapshot;
use serde_json::Value;

/// Map a prompt result to `Option`: `Interrupted` means cancelled, any other error propagates.
pub(crate) fn or_cancel<T>(result: io::Result<T>) -> Result<Option<T>> {
    match result {
        Ok(v) => Ok(Some(v)),
        Err(e) if e.kind() == io::ErrorKind::Interrupted => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Compact human-readable token counts: 1_234 -> "1.2k", 2_500_000 -> "2.5M".
pub(crate) fn human(n: u64) -> String {
    match n {
        0..=999 => n.to_string(),
        1_000..=999_999 => trim_zero(n as f64 / 1_000.0, 'k'),
        _ => trim_zero(n as f64 / 1_000_000.0, 'M'),
    }
}

/// A running time as a reader states it: seconds under a minute, `3m 29s` past
/// one. Four minutes of work should not read as a part number.
pub(crate) fn elapsed(secs: u64) -> String {
    match secs >= 60 {
        true => format!("{}m {}s", secs / 60, secs % 60),
        false => format!("{secs}s"),
    }
}

fn trim_zero(v: f64, suffix: char) -> String {
    let s = format!("{v:.1}");
    let s = s.strip_suffix(".0").map(str::to_string).unwrap_or(s);
    format!("{s}{suffix}")
}

/// The 6-field usage block repeated in every JSON output path.
pub(crate) fn usage_json(u: &UsageSnapshot) -> Value {
    serde_json::json!({
        "prompt_tokens": u.prompt_tokens,
        "completion_tokens": u.completion_tokens,
        "total_tokens": u.total_tokens,
        "requests": u.requests,
        "estimated_cost_usd": u.estimated_cost_usd,
        "estimated": u.estimated,
    })
}
