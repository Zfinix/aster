//! How far the inbox has been read, and the RFC 3339 times Photon stamps on
//! every message.

use std::collections::HashMap;

use serde_json::{Value, json};

use super::ChatKey;

/// Photon's lists are eventually consistent, so a message can show up a little
/// after newer ones. Each poll reads back this far behind the newest message
/// it has seen, and the ids it remembers keep the overlap from repeating.
pub(crate) const CURSOR_OVERLAP_MS: i64 = 60_000;

/// Where the inbox has been read up to, kept on disk so a restart neither
/// answers old messages again nor misses the ones that came in while it was
/// down.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct Cursor {
    /// `createdAt` of the newest message seen, in Unix milliseconds.
    pub since: i64,
    /// Ids inside the overlap window, with their `createdAt`.
    pub seen: HashMap<String, i64>,
    /// The chat that spoke last, so a wake-up after a restart has somewhere
    /// to land.
    pub last: Option<LastChat>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct LastChat {
    pub chat: ChatKey,
    pub agent: String,
}

impl Cursor {
    /// The `createdSince` bound for the next poll.
    pub(crate) fn query_since(&self) -> String {
        format_time((self.since - CURSOR_OVERLAP_MS).max(0))
    }

    /// True the first time an id is offered. Later offers are the overlap
    /// window being read again and are skipped.
    pub(crate) fn admit(&mut self, id: &str, created: i64) -> bool {
        if self.seen.contains_key(id) {
            return false;
        }
        self.seen.insert(id.to_string(), created);
        self.since = self.since.max(created);
        let floor = self.since - CURSOR_OVERLAP_MS;
        self.seen.retain(|_, at| *at >= floor);
        true
    }

    pub(crate) fn to_json(&self) -> Value {
        let mut seen: Vec<(&String, &i64)> = self.seen.iter().collect();
        seen.sort();
        json!({
            "since": format_time(self.since),
            "seen": seen.into_iter().map(|(id, at)| json!([id, format_time(*at)])).collect::<Vec<_>>(),
            "last": self.last.as_ref().map(|last| json!({
                "platform": last.chat.platform,
                "space": last.chat.space,
                "agent": last.agent,
            })),
        })
    }

    pub(crate) fn from_json(value: &Value) -> Option<Self> {
        let since = parse_time(value.get("since")?.as_str()?)?;
        let seen = value
            .get("seen")
            .and_then(Value::as_array)
            .map(|rows| {
                rows.iter()
                    .filter_map(|row| {
                        let id = row.get(0)?.as_str()?.to_string();
                        let at = parse_time(row.get(1)?.as_str()?)?;
                        Some((id, at))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let last = value.get("last").and_then(|last| {
            let text = |key: &str| last.get(key).and_then(Value::as_str).map(str::to_string);
            Some(LastChat {
                chat: ChatKey {
                    platform: text("platform")?,
                    space: text("space")?,
                },
                agent: text("agent")?,
            })
        });
        Some(Self { since, seen, last })
    }
}

/// Unix milliseconds for an RFC 3339 timestamp, the form every Photon time
/// takes. None for anything that is not one.
pub(crate) fn parse_time(text: &str) -> Option<i64> {
    let (date, rest) = text.split_once(['T', 't'])?;
    let mut date = date.splitn(3, '-');
    let year: i64 = date.next()?.parse().ok()?;
    let month: i64 = date.next()?.parse().ok()?;
    let day: i64 = date.next()?.parse().ok()?;
    let (clock, offset_ms) = match rest.strip_suffix(['Z', 'z']) {
        Some(clock) => (clock, 0),
        None => {
            let at = rest.rfind(['+', '-'])?;
            let (clock, offset) = rest.split_at(at);
            let sign = if offset.starts_with('-') { -1 } else { 1 };
            let (hours, minutes) = offset[1..].split_once(':')?;
            let minutes = hours.parse::<i64>().ok()? * 60 + minutes.parse::<i64>().ok()?;
            (clock, sign * minutes * 60_000)
        }
    };
    let (hms, fraction) = clock.split_once('.').unwrap_or((clock, ""));
    if !fraction.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let millis: i64 = fraction
        .chars()
        .chain("000".chars())
        .take(3)
        .collect::<String>()
        .parse()
        .ok()?;
    let mut hms = hms.splitn(3, ':');
    let hour: i64 = hms.next()?.parse().ok()?;
    let minute: i64 = hms.next()?.parse().ok()?;
    let second: i64 = hms.next()?.parse().ok()?;
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 60
    {
        return None;
    }
    let days = days_from_civil(year, month, day);
    Some((days * 86_400 + hour * 3_600 + minute * 60 + second) * 1_000 + millis - offset_ms)
}

/// Unix milliseconds as RFC 3339 in UTC with millisecond precision.
pub(crate) fn format_time(ms: i64) -> String {
    let days = ms.div_euclid(86_400_000);
    let in_day = ms.rem_euclid(86_400_000);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        in_day / 3_600_000,
        in_day / 60_000 % 60,
        in_day / 1_000 % 60,
        in_day % 1_000
    )
}

/// Days since 1970-01-01 for a proleptic Gregorian date (Howard Hinnant's
/// algorithm), so the bridge needs no date crate for one timestamp format.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month_from_march = (month + 9) % 12;
    let day_of_year = (153 * month_from_march + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let days = days + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_from_march = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_from_march + 2) / 5 + 1;
    let month = if month_from_march < 10 {
        month_from_march + 3
    } else {
        month_from_march - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}
