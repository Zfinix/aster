//! Photon 2.0 provider: one bridge for the iMessage, WhatsApp, and Telegram
//! lines Photon hosts. Inbound messages come from polling `listMessages`, which
//! needs no public URL and works behind carrier NAT, so the bridge runs on the
//! phone itself. Each conversation keeps its own `aster acp` agent, the way the
//! Telegram bridge does, and replies go back as plain-text sends.

use std::collections::{HashMap, HashSet};
use std::env;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use anyhow::{Context, Result, anyhow, bail, ensure};
use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot};
use tokio::task::AbortHandle;
use ulid::Ulid;

use crate::bridge::{Agent, Answer, Turn, TurnEvent, TurnOutcome};
use crate::channel::{
    MODES, Skills, console_text, console_tool, discover_skill_commands, skill_prompt, truncate,
    workspace_name,
};
use crate::telegram::{Wakeup, is_stop_request, parse_wakeup};

/// Photon 2.0's REST origin; `ASTER_PHOTON_API` points the bridge elsewhere.
pub const SPECTRUM_API: &str = "https://api.photon.codes/v1";

/// The platforms answered when none are named: the ones a person chats on.
const SPECTRUM_PLATFORMS: &[&str] = &["imessage", "whatsapp", "telegram"];

const CHUNK_LIMIT: usize = 3500;

/// How often the inbox is read: quickly while a conversation is moving, then
/// easing off to the slow end once it goes quiet.
const POLL_FAST: Duration = Duration::from_secs(2);
const POLL_SLOW: Duration = Duration::from_secs(5);

/// Photon's lists are eventually consistent, so a message can show up a little
/// after newer ones. Each poll reads back this far behind the newest message
/// it has seen, and the ids it remembers keep the overlap from repeating.
pub(crate) const CURSOR_OVERLAP_MS: i64 = 60_000;

const PAGE_SIZE: &str = "100";

/// Pages read per poll before handing back to the loop, so a backlog cannot
/// starve the wake-up watcher or hold a turn's replies.
const MAX_PAGES: usize = 20;

/// Typing shows for a few seconds on each platform, so it is refreshed for as
/// long as the turn runs, and only while nothing is waiting on the person.
const TYPING_EVERY: Duration = Duration::from_secs(8);

/// Retries for a send or upload. Polls are not retried in place; the next
/// poll is the retry.
const SEND_ATTEMPTS: usize = 3;

/// The longest a Retry-After is honoured before the next try anyway.
const RETRY_AFTER_CAP: Duration = Duration::from_secs(60);

/// The largest inbound file staged for the agent, and the largest screenshot
/// sent back. Both are far above a phone photo or screenshot.
const ATTACHMENT_LIMIT: usize = 20 * 1024 * 1024;

/// Staged files outlive their turn so a re-read stays possible, then go after
/// a day.
const STAGED_TTL: Duration = Duration::from_secs(24 * 60 * 60);
const STAGED_SWEEP: Duration = Duration::from_secs(60 * 60);
const STAGED_PREFIX: &str = "spectrum-";

const WAKE_POLL: Duration = Duration::from_secs(2);
const ERRAND_CHARS: usize = 90;

/// What happens to screenshots: `end` sends the last one with the reply, `on`
/// sends each as it is taken, `off` keeps them all back.
const PHOTOS: &[&str] = &["end", "on", "off"];

const ASIDE_NOTE: &str = "The user sent this from the same chat while you are working, so it is \
     part of the task in hand, not a new one. Take it in at this point in the work: if it changes \
     what you should do, say so in a line and adjust; if it is only context, say you have it and \
     carry on. Do not start over and do not answer it as a separate job.";

const SPECTRUM_SYSTEM: &str = "\
The user is talking to you through a chat app on their phone (via aster remote \
and Photon), not a terminal. Replies arrive as plain text messages, so keep \
them short and conversational and lead with the answer. Markdown does not \
render: never use tables, headers, bold markers, or long code blocks. \
The steps you take are not shown, only your final message, so make it the \
summary of the turn: the outcome, anything still pending, and anything the \
person has to decide, in three short lines or fewer. \
Approval prompts and questions reach the user as plain text. They answer \
yes, always, or no, or pick an option by its number; if one is denied or \
skipped, take the hint and do not immediately re-request it. \
Files the user sends arrive as @path mentions. The last screenshot you take \
in a turn is sent along with your reply.";

#[derive(Clone)]
pub struct SpectrumConfig {
    /// A project API key (`pho_sk_…`) with `project:read` and `project:write`.
    pub api_key: String,
    pub project_id: String,
    pub api_base: String,
    /// Platforms answered; the rest of the project's traffic is left alone.
    pub platforms: Vec<String>,
    /// Sender handles allowed to drive the agent, optionally scoped to one
    /// platform as `imessage:+15551234567`.
    pub allowed_senders: Vec<String>,
    pub bin: PathBuf,
    pub repo_root: PathBuf,
    pub mode: String,
}

/// One conversation: Photon gives every chat a Space, and a Space id is only
/// meaningful on its platform.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ChatKey {
    pub platform: String,
    pub space: String,
}

impl ChatKey {
    /// The key as a file name and session name: Space ids are already plain,
    /// but anything else is folded so a path can never climb out.
    pub(crate) fn slug(&self) -> String {
        let safe = |text: &str| -> String {
            text.chars()
                .map(|c| {
                    if c.is_ascii_alphanumeric() || c == '_' {
                        c
                    } else {
                        '-'
                    }
                })
                .take(64)
                .collect()
        };
        format!("{}-{}", safe(&self.platform), safe(&self.space))
    }
}

/// A message worth answering, reduced to what the bridge acts on.
#[derive(Debug, PartialEq)]
pub(crate) struct Inbound {
    pub id: String,
    pub chat: ChatKey,
    /// The agent's own user in the Space, which is who replies are sent as.
    pub agent: String,
    pub sender: String,
    pub parts: Vec<Part>,
}

#[derive(Debug, PartialEq)]
pub(crate) enum Part {
    Text(String),
    Attachment(String),
}

/// A person writing to the agent in a direct chat on a platform this bridge
/// answers, with something in it to read. Everything else in the inbox (the
/// agent's own sends, read receipts, typing, group chats) is None.
pub(crate) fn parse_inbound(message: &Value, platforms: &[String]) -> Option<Inbound> {
    let field =
        |value: &Value, key: &str| value.get(key).and_then(Value::as_str).map(str::to_string);
    if field(message, "status").as_deref() != Some("received") {
        return None;
    }
    let platform = field(message, "platform")?;
    if !platforms.is_empty() && !platforms.iter().any(|p| p == &platform) {
        return None;
    }
    let sent_by = message.get("sentBy")?;
    let received_by = message.get("receivedBy")?;
    let space = message.get("space")?;
    if field(sent_by, "kind").as_deref() != Some("human")
        || field(received_by, "kind").as_deref() != Some("agent")
        || field(space, "kind").as_deref() != Some("direct")
    {
        return None;
    }
    let sender_id = field(sent_by, "id")?;
    let agent = field(received_by, "id")?;
    if sender_id == agent {
        return None;
    }
    let parts = content_parts(message.get("content")?);
    if parts.is_empty() {
        return None;
    }
    Some(Inbound {
        id: field(message, "id")?,
        chat: ChatKey {
            platform,
            space: field(space, "id")?,
        },
        agent,
        sender: field(sent_by, "handle")?,
        parts,
    })
}

/// Text and attachments out of one content arm; a group is its parts in order.
fn content_parts(content: &Value) -> Vec<Part> {
    match content.get("type").and_then(Value::as_str) {
        Some("text") => content
            .get("text")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(|text| vec![Part::Text(text.to_string())])
            .unwrap_or_default(),
        Some("attachment") => content
            .get("attachmentId")
            .and_then(Value::as_str)
            .map(|id| vec![Part::Attachment(id.to_string())])
            .unwrap_or_default(),
        Some("group") => content
            .get("parts")
            .and_then(Value::as_array)
            .map(|parts| parts.iter().flat_map(content_parts).collect())
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// Whether a sender is on the allowlist. Phone numbers compare by digits, so
/// `+1 (555) 123-4567` and `15551234567` are one person; anything else (an
/// email, a Telegram id) compares without case.
pub(crate) fn is_allowed(allowed: &[String], platform: &str, handle: &str) -> bool {
    let handle = normalize_handle(handle);
    if handle.is_empty() {
        return false;
    }
    allowed.iter().any(|entry| {
        let (scope, wanted) = match entry.split_once(':') {
            Some((scope, wanted)) if is_platform(scope) => (Some(scope), wanted),
            _ => (None, entry.as_str()),
        };
        scope.is_none_or(|scope| scope.eq_ignore_ascii_case(platform))
            && normalize_handle(wanted) == handle
    })
}

fn is_platform(name: &str) -> bool {
    [
        "imessage",
        "whatsapp",
        "whatsapp_business",
        "telegram",
        "sms",
        "email",
    ]
    .iter()
    .any(|p| p.eq_ignore_ascii_case(name))
}

pub(crate) fn normalize_handle(handle: &str) -> String {
    let handle = handle.trim();
    let phone = handle.chars().any(|c| c.is_ascii_digit())
        && handle
            .chars()
            .all(|c| c.is_ascii_digit() || " +-().".contains(c));
    if phone {
        handle.chars().filter(char::is_ascii_digit).collect()
    } else {
        handle.to_lowercase()
    }
}

/// What a message from outside the allowlist earns: silence. The line's number
/// is public, so anyone can message it, and every answer confirms a live agent
/// and hands over the recipe for getting in. The exception is a line nobody is
/// allowed on yet, which has to tell whoever asks their own handle, or its
/// owner can never allow themselves.
pub(crate) fn stranger_reply(allowed: &[String], handle: &str) -> Option<String> {
    if !allowed.is_empty() {
        return None;
    }
    Some(format!(
        "This line isn't set up yet. Your handle is {handle}. Restart it with --sender {handle} to allow it."
    ))
}

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

/// A reply as plain-text messages of at most `limit` bytes. The chat apps on
/// this bridge show markdown as typed, so the markers that only make sense
/// rendered go, and a line longer than a whole message is cut, at a space
/// where there is one, rather than sent over the limit.
pub(crate) fn plain_chunks(text: &str, limit: usize) -> Vec<String> {
    pack_lines(text.lines().filter_map(plain_line), limit)
}

/// Text as messages of at most `limit` bytes, every line kept as written.
/// Approvals and questions go this way: what the person agrees to has to be
/// exactly what runs, fences and asterisks included.
pub(crate) fn verbatim_chunks(text: &str, limit: usize) -> Vec<String> {
    pack_lines(text.lines().map(str::to_string), limit)
}

fn pack_lines(lines: impl Iterator<Item = String>, limit: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();
    for line in lines {
        if line.len() > limit {
            if !current.trim().is_empty() {
                chunks.push(std::mem::take(&mut current));
            }
            current.clear();
            chunks.extend(split_long(&line, limit));
            continue;
        }
        if !current.is_empty() && current.len() + 1 + line.len() > limit {
            chunks.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push('\n');
        }
        current.push_str(&line);
    }
    if !current.trim().is_empty() {
        chunks.push(current);
    }
    chunks
        .into_iter()
        .map(|chunk| chunk.trim().to_string())
        .filter(|chunk| !chunk.is_empty())
        .collect()
}

/// One markdown line as it should read unrendered; None for a code fence.
fn plain_line(line: &str) -> Option<String> {
    if line.trim_start().starts_with("```") {
        return None;
    }
    let trimmed = line.trim_start();
    let heading = trimmed.trim_start_matches('#');
    let line = if heading.len() < trimmed.len() && heading.starts_with(' ') {
        heading.trim_start()
    } else {
        line
    };
    Some(line.replace("**", ""))
}

fn split_long(line: &str, limit: usize) -> Vec<String> {
    let mut pieces = Vec::new();
    let mut rest = line;
    while rest.len() > limit {
        let mut cut = limit;
        while !rest.is_char_boundary(cut) {
            cut -= 1;
        }
        if let Some(space) = rest[..cut].rfind(' ').filter(|at| *at > 0) {
            cut = space;
        }
        pieces.push(rest[..cut].trim_end().to_string());
        rest = rest[cut..].trim_start();
    }
    if !rest.is_empty() {
        pieces.push(rest.to_string());
    }
    pieces
}

/// An approval as a text message, with the three words that answer it.
pub(crate) fn approval_text(preview: &str, scope: Option<&str>) -> String {
    let mut text = format!("Needs your go-ahead:\n{}", truncate(preview.trim(), 1500));
    if let Some(scope) = scope.filter(|scope| !scope.trim().is_empty()) {
        text.push_str(&format!("\nScope: {scope}"));
    }
    text.push_str("\nReply yes, always, or no.");
    text
}

/// The answer a reply gives to a pending approval. None when the reply is not
/// an answer at all, which the caller treats as a no plus a new message.
pub(crate) fn parse_approval(text: &str) -> Option<Answer> {
    let word = text
        .trim()
        .trim_end_matches(['.', '!'])
        .trim()
        .to_lowercase();
    match word.as_str() {
        "y" | "yes" | "yeah" | "yep" | "ok" | "okay" | "allow" | "approve" | "go" | "go ahead"
        | "sure" | "👍" => Some(Answer::Allow),
        "always" | "always allow" | "a" => Some(Answer::AlwaysAllow),
        "n" | "no" | "nope" | "deny" | "don't" | "dont" | "👎" => Some(Answer::Deny),
        _ => None,
    }
}

/// A question as a text message, options numbered so a reply can be one digit.
pub(crate) fn question_text(header: &str, question: &str, options: &[String]) -> String {
    let mut lines = Vec::new();
    if !header.trim().is_empty() {
        lines.push(header.trim().to_string());
    }
    lines.push(question.trim().to_string());
    for (n, option) in options.iter().enumerate() {
        lines.push(format!("{}. {option}", n + 1));
    }
    lines.push(if options.is_empty() {
        "Reply with your answer, or skip.".to_string()
    } else {
        "Reply with a number or your own answer, or skip.".to_string()
    });
    lines.join("\n")
}

/// The answer a reply gives to a question: an option by number or by name,
/// the reply itself as a custom answer, or None for skip.
pub(crate) fn resolve_choice(text: &str, options: &[String]) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.eq_ignore_ascii_case("skip") {
        return None;
    }
    let number = trimmed.trim_end_matches(['.', ')']);
    if let Ok(n) = number.parse::<usize>()
        && (1..=options.len()).contains(&n)
    {
        return Some(options[n - 1].clone());
    }
    options
        .iter()
        .find(|option| option.eq_ignore_ascii_case(trimmed))
        .cloned()
        .or_else(|| Some(trimmed.to_string()))
}

/// The PNGs an `asterctl shot` names in a tool's output, one per
/// `shot <path> (WxH, N bytes)` line.
pub(crate) fn shot_paths(output: &str) -> Vec<String> {
    output
        .lines()
        .filter_map(|line| {
            let words: Vec<&str> = line.split_whitespace().collect();
            let at = words
                .windows(2)
                .position(|pair| pair[0] == "shot" && pair[1].ends_with(".png"))?;
            Some(words[at + 1].to_string())
        })
        .collect()
}

/// The upload body Photon takes: JSON metadata, then the file bytes, as the
/// two parts of one `multipart/related` request.
pub(crate) fn related_body(
    boundary: &str,
    metadata: &Value,
    content_type: &str,
    bytes: &[u8],
) -> Vec<u8> {
    let mut body = Vec::with_capacity(bytes.len() + 256);
    body.extend_from_slice(
        format!("--{boundary}\r\nContent-Type: application/json\r\n\r\n{metadata}\r\n").as_bytes(),
    );
    body.extend_from_slice(
        format!("--{boundary}\r\nContent-Type: {content_type}\r\n\r\n").as_bytes(),
    );
    body.extend_from_slice(bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    body
}

/// A file extension for staged media, from the Content-Type it was served as.
pub(crate) fn extension_for(content_type: &str) -> &'static str {
    let essence = content_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    match essence.as_str() {
        "image/png" => "png",
        "image/jpeg" | "image/jpg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/heic" => "heic",
        "image/heif" => "heif",
        "application/pdf" => "pdf",
        "text/plain" => "txt",
        "text/csv" => "csv",
        "application/json" => "json",
        "audio/mpeg" => "mp3",
        "audio/mp4" | "audio/m4a" | "audio/x-m4a" => "m4a",
        "video/mp4" => "mp4",
        "video/quicktime" => "mov",
        _ => "bin",
    }
}

fn image_type(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "webp" => Some("image/webp"),
        _ => None,
    }
}

/// Seconds from a Retry-After header, capped; the HTTP-date form is rare
/// enough that the caller's own backoff covers it.
pub(crate) fn retry_after(header: Option<&str>) -> Option<Duration> {
    let seconds: u64 = header?.trim().parse().ok()?;
    Some(Duration::from_secs(seconds).min(RETRY_AFTER_CAP))
}

/// The status codes Photon's own clients retry.
fn retryable(status: u16) -> bool {
    matches!(status, 408 | 429 | 502 | 503 | 504)
}

/// A refused Photon call, kept typed so the poll loop can tell a rate limit
/// or a revoked key from a blip.
#[derive(Debug)]
pub(crate) struct ApiError {
    pub op: &'static str,
    pub status: u16,
    pub code: String,
    pub retry_after: Option<Duration>,
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "photon {} failed: HTTP {}", self.op, self.status)?;
        if !self.code.is_empty() {
            write!(f, " {}", self.code)?;
        }
        Ok(())
    }
}

impl std::error::Error for ApiError {}

/// The `code` of an RFC 9457 problem body; empty when there is none.
pub(crate) fn problem_code(body: &str) -> String {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|problem| {
            problem
                .get("code")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_default()
}

/// Photon 2.0's REST API, by hand on reqwest: eight calls do not justify a
/// generated client of several hundred thousand lines.
struct Api {
    http: reqwest::Client,
    base: String,
    key: String,
}

impl Api {
    fn new(api_base: &str, project_id: &str, key: &str) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()?;
        Ok(Self {
            http,
            base: format!("{}/projects/{project_id}", api_base.trim_end_matches('/')),
            key: key.to_string(),
        })
    }

    /// Send a request, retrying what Photon says is worth retrying. `build`
    /// makes the request afresh each time, so a retry repeats it exactly,
    /// idempotency key included.
    async fn execute(
        &self,
        op: &'static str,
        attempts: usize,
        also_retry: &[u16],
        build: impl Fn() -> reqwest::RequestBuilder,
    ) -> Result<reqwest::Response> {
        let mut attempt = 0;
        loop {
            attempt += 1;
            let backoff = Duration::from_millis(500 * (1 << attempt.min(5)));
            match build().bearer_auth(&self.key).send().await {
                Ok(response) if response.status().is_success() => return Ok(response),
                Ok(response) => {
                    let status = response.status().as_u16();
                    let wait = retry_after(
                        response
                            .headers()
                            .get(reqwest::header::RETRY_AFTER)
                            .and_then(|value| value.to_str().ok()),
                    );
                    let code = problem_code(&response.text().await.unwrap_or_default());
                    if attempt < attempts && (retryable(status) || also_retry.contains(&status)) {
                        tokio::time::sleep(wait.unwrap_or(backoff)).await;
                        continue;
                    }
                    return Err(ApiError {
                        op,
                        status,
                        code,
                        retry_after: wait,
                    }
                    .into());
                }
                Err(err) if attempt < attempts && (err.is_timeout() || err.is_connect()) => {
                    tokio::time::sleep(backoff).await;
                }
                Err(err) => return Err(anyhow!(err).context(format!("photon {op}"))),
            }
        }
    }

    async fn list_messages(&self, query: &[(&str, &str)]) -> Result<Value> {
        let url = format!("{}/messages", self.base);
        let response = self
            .execute("listMessages", 1, &[], || self.http.get(&url).query(query))
            .await?;
        Ok(response.json().await?)
    }

    async fn send(
        &self,
        key: &str,
        space: &str,
        sender: &str,
        parent: Option<&str>,
        content: Value,
    ) -> Result<()> {
        let url = format!("{}/messages", self.base);
        let mut body = json!({ "spaceId": space, "senderId": sender, "content": content });
        if let Some(parent) = parent {
            body["parentMessageId"] = Value::String(parent.to_string());
        }
        self.execute("sendMessage", SEND_ATTEMPTS, &[], || {
            self.http
                .post(&url)
                .header("idempotency-key", key)
                .json(&body)
        })
        .await?;
        Ok(())
    }

    async fn typing(&self, space: &str, sender: &str, typing: bool) -> Result<()> {
        let url = format!("{}/spaces/{space}/typing", self.base);
        let body = json!({ "senderId": sender, "typing": typing });
        self.execute("setSpaceTyping", 1, &[], || {
            self.http.post(&url).json(&body)
        })
        .await?;
        Ok(())
    }

    /// An attachment's bytes and media type. A fresh one can take a few
    /// seconds to be ready, which Photon reports as a 409 worth retrying.
    async fn download(&self, attachment: &str) -> Result<(Vec<u8>, String)> {
        let url = format!("{}/attachments/{attachment}/content", self.base);
        let response = self
            .execute("downloadAttachment", SEND_ATTEMPTS, &[409], || {
                self.http.get(&url)
            })
            .await?;
        if response
            .content_length()
            .is_some_and(|len| len > ATTACHMENT_LIMIT as u64)
        {
            bail!("the file is over {} MB", ATTACHMENT_LIMIT / (1024 * 1024));
        }
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("application/octet-stream")
            .to_string();
        let bytes = response.bytes().await?;
        ensure!(
            bytes.len() <= ATTACHMENT_LIMIT,
            "the file is over {} MB",
            ATTACHMENT_LIMIT / (1024 * 1024)
        );
        Ok((bytes.to_vec(), content_type))
    }

    async fn upload(&self, key: &str, path: &Path, content_type: &str) -> Result<String> {
        let bytes = tokio::fs::read(path)
            .await
            .with_context(|| format!("reading {}", path.display()))?;
        ensure!(
            bytes.len() <= ATTACHMENT_LIMIT,
            "{} is too large to send",
            path.display()
        );
        let filename = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("screenshot.png");
        let metadata = json!({
            "filename": filename,
            "contentType": content_type,
            "sizeBytes": bytes.len(),
        });
        let boundary = format!("aster-{}", Ulid::new());
        let body = related_body(&boundary, &metadata, content_type, &bytes);
        let url = format!("{}/attachments", self.base);
        let response = self
            .execute("uploadAttachment", SEND_ATTEMPTS, &[], || {
                self.http
                    .post(&url)
                    .header("idempotency-key", key)
                    .header(
                        reqwest::header::CONTENT_TYPE,
                        format!("multipart/related; boundary={boundary}"),
                    )
                    .body(body.clone())
            })
            .await?;
        let attachment: Value = response.json().await?;
        attachment
            .get("id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .context("uploadAttachment returned no id")
    }
}

enum Pending {
    Approval(oneshot::Sender<Answer>),
    Question {
        options: Vec<String>,
        respond: oneshot::Sender<Option<String>>,
    },
}

#[derive(Default)]
struct ChatState {
    /// When this chat last spoke, so a wake-up knows where to land.
    touched: Option<Instant>,
    /// The agent's user in this Space; every send names it as the sender.
    agent_user: Option<String>,
    /// The newest message seen here, so a reply only quotes when the chat has
    /// moved past what it answers.
    last_seen: Option<String>,
    pending: Option<Pending>,
    running: Option<AbortHandle>,
    agent: Option<Arc<Agent>>,
    session: Option<String>,
    stopped: bool,
    /// Messages that arrived too late to join the running turn; they run as
    /// one turn as soon as it ends.
    queued: Vec<(Option<String>, String)>,
    mode: Option<String>,
    photos: Option<String>,
    loaded: bool,
}

/// What a new message does in a chat, given the turn in flight.
enum Admission {
    Start {
        mode: Option<String>,
        agent: Option<Arc<Agent>>,
        session: Option<String>,
    },
    /// The turn was told to stop and is still winding down, so the message
    /// waits and runs after it.
    Queued,
    Busy,
}

impl ChatState {
    fn admit(&mut self, reply_to: Option<&str>, prompt: &str) -> Admission {
        if self.running.is_none() {
            self.stopped = false;
            return Admission::Start {
                mode: self.mode.clone(),
                agent: self.agent.clone().filter(|agent| agent.is_alive()),
                session: self.session.clone(),
            };
        }
        if self.stopped && !is_stop_request(prompt) {
            self.queued
                .push((reply_to.map(str::to_string), prompt.to_string()));
            return Admission::Queued;
        }
        Admission::Busy
    }

    /// Abort the running turn and drop what was waiting behind it, saying how
    /// many messages that was; None when nothing runs. The turn stays
    /// `running` until its finish has run, so a message sent meanwhile cannot
    /// start a second turn that the old finish would then clobber.
    fn stop(&mut self) -> Option<usize> {
        let running = self.running.as_ref()?;
        // Set before aborting, so the driver reads it as a deliberate stop
        // rather than reporting the abort as a failure.
        self.stopped = true;
        running.abort();
        Some(std::mem::take(&mut self.queued).len())
    }
}

/// Everything the bridge's tasks share.
struct Bridge {
    api: Api,
    cfg: SpectrumConfig,
    skills: Skills,
    chats: Mutex<HashMap<ChatKey, ChatState>>,
    cursor: Mutex<Cursor>,
    cursor_path: Option<PathBuf>,
    /// Handles already told how to get in, so a stranger hears it once per
    /// run rather than once per message.
    told: Mutex<HashSet<String>>,
    /// Each chat's messages waiting on the one being handled there, so a slow
    /// download in one chat never holds up another.
    workers: Mutex<HashMap<ChatKey, mpsc::UnboundedSender<Inbound>>>,
}

impl Bridge {
    fn chat<T>(&self, key: &ChatKey, act: impl FnOnce(&mut ChatState) -> T) -> T {
        let mut chats = self.chats.lock().expect("chats lock");
        let state = chats.entry(key.clone()).or_default();
        if !state.loaded {
            let saved = load_settings(key);
            let field = |name: &str| saved.get(name).and_then(Value::as_str).map(str::to_string);
            state.mode = field("mode");
            state.session = field("session");
            state.photos = field("photos");
            state.loaded = true;
        }
        act(state)
    }

    fn save_chat(&self, key: &ChatKey) {
        let saved = self.chat(
            key,
            |state| json!({ "mode": state.mode, "session": state.session, "photos": state.photos }),
        );
        if let Some(path) = settings_path(key) {
            write_json(&path, &saved);
        }
    }

    fn save_cursor(&self) {
        let Some(path) = &self.cursor_path else {
            return;
        };
        let saved = self.cursor.lock().expect("cursor lock").to_json();
        write_json(path, &saved);
    }

    /// The chat that spoke most recently, or the one saved from before a
    /// restart, with the agent user to send as.
    fn latest_chat(&self) -> Option<LastChat> {
        let live = {
            let chats = self.chats.lock().expect("chats lock");
            chats
                .iter()
                .filter_map(|(key, state)| {
                    Some((state.touched?, key.clone(), state.agent_user.clone()?))
                })
                .max_by_key(|(touched, ..)| *touched)
                .map(|(_, chat, agent)| LastChat { chat, agent })
        };
        live.or_else(|| self.cursor.lock().expect("cursor lock").last.clone())
    }

    fn outbox(self: &Arc<Self>, chat: &ChatKey, reply_to: Option<&str>, label: &str) -> Outbox {
        let agent = self.chat(chat, |state| state.agent_user.clone());
        let base = match reply_to {
            Some(id) => format!("aster:{id}:{label}"),
            None => format!("aster:{}:{label}", Ulid::new()),
        };
        Outbox {
            bridge: Arc::clone(self),
            chat: chat.clone(),
            agent,
            base,
            seq: AtomicU32::new(0),
        }
    }
}

fn settings_path(key: &ChatKey) -> Option<PathBuf> {
    let home = env::var("HOME").ok()?;
    Some(
        PathBuf::from(home)
            .join(".aster/remote")
            .join(format!("spectrum-{}.json", key.slug())),
    )
}

fn load_settings(key: &ChatKey) -> Value {
    settings_path(key)
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or(Value::Null)
}

fn cursor_path(project: &str) -> Option<PathBuf> {
    let home = env::var("HOME").ok()?;
    let key = ChatKey {
        platform: "cursor".into(),
        space: project.into(),
    };
    Some(
        PathBuf::from(home)
            .join(".aster/remote")
            .join(format!("spectrum-{}.json", key.slug())),
    )
}

/// Write through a temporary file and rename, so a crash mid-write leaves the
/// old state rather than half a file.
fn write_json(path: &Path, value: &Value) {
    if let Some(parent) = path.parent()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        tracing::warn!("could not create {}: {e}", parent.display());
        return;
    }
    let partial = path.with_extension("json.tmp");
    let written =
        std::fs::write(&partial, value.to_string()).and_then(|()| std::fs::rename(&partial, path));
    if let Err(e) = written {
        tracing::warn!("could not save {}: {e}", path.display());
    }
}

/// Sends for one purpose in one chat. Each send's idempotency key is the
/// message it answers, the purpose, and its place in the sequence, so a retry,
/// or the same message handled again after a crash, cannot post twice.
struct Outbox {
    bridge: Arc<Bridge>,
    chat: ChatKey,
    agent: Option<String>,
    base: String,
    seq: AtomicU32,
}

impl Outbox {
    fn next_key(&self) -> String {
        format!("{}:{}", self.base, self.seq.fetch_add(1, Ordering::Relaxed))
    }

    async fn text(&self, text: &str) {
        self.reply(text, None).await;
    }

    /// The text in as many messages as it takes; only the first quotes.
    async fn reply(&self, text: &str, parent: Option<&str>) {
        self.send_chunks(plain_chunks(text, CHUNK_LIMIT), parent)
            .await;
    }

    /// The text with no markdown cleanup, and whether all of it went out.
    async fn verbatim(&self, text: &str) -> bool {
        self.send_chunks(verbatim_chunks(text, CHUNK_LIMIT), None)
            .await
    }

    async fn send_chunks(&self, chunks: Vec<String>, parent: Option<&str>) -> bool {
        let Some(agent) = &self.agent else {
            tracing::warn!(
                "no agent user known for {}; reply dropped",
                self.chat.slug()
            );
            return false;
        };
        for (n, chunk) in chunks.into_iter().enumerate() {
            let content = json!({ "type": "text", "format": "plain", "text": chunk });
            let quoted = parent.filter(|_| n == 0);
            if let Err(e) = self
                .bridge
                .api
                .send(&self.next_key(), &self.chat.space, agent, quoted, content)
                .await
            {
                tracing::warn!("send to {} failed: {e:#}", self.chat.slug());
                eprintln!("[{}] error: a reply did not send: {e:#}", self.chat.slug());
                return false;
            }
        }
        true
    }

    /// A screenshot as an attachment message. Only images under the temp
    /// directory or the workspace go out, so a path in a tool's output can
    /// never turn into a way to send some other file off the phone.
    async fn photo(&self, path: &str) {
        let Some(agent) = &self.agent else {
            return;
        };
        let path = Path::new(path);
        let Some(content_type) = image_type(path) else {
            return;
        };
        if !is_sendable(path, &self.bridge.cfg.repo_root) {
            tracing::warn!(
                "not sending {}: outside the temp dir and workspace",
                path.display()
            );
            return;
        }
        let key = self.next_key();
        let attachment = match self
            .bridge
            .api
            .upload(&format!("{key}:upload"), path, content_type)
            .await
        {
            Ok(id) => id,
            Err(e) => {
                tracing::warn!("could not upload {}: {e:#}", path.display());
                return;
            }
        };
        let content = json!({ "type": "attachment", "attachmentId": attachment });
        if let Err(e) = self
            .bridge
            .api
            .send(&key, &self.chat.space, agent, None, content)
            .await
        {
            tracing::warn!("could not send {}: {e:#}", path.display());
        }
    }
}

fn is_sendable(path: &Path, repo_root: &Path) -> bool {
    let Ok(path) = path.canonicalize() else {
        return false;
    };
    [env::temp_dir(), repo_root.to_path_buf()]
        .iter()
        .filter_map(|root| root.canonicalize().ok())
        .any(|root| path.starts_with(root))
}

pub async fn run_spectrum(cfg: SpectrumConfig) -> Result<()> {
    ensure!(
        !cfg.api_key.trim().is_empty(),
        "the Photon API key is empty"
    );
    ensure!(
        cfg.project_id.starts_with("pho_prj_")
            && cfg
                .project_id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_'),
        "the Photon project id should look like pho_prj_…"
    );
    let api = Api::new(&cfg.api_base, &cfg.project_id, &cfg.api_key)?;
    let cursor_path = cursor_path(&cfg.project_id);
    let saved = cursor_path
        .as_deref()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
        .and_then(|value| Cursor::from_json(&value));
    let newest = connect(&api).await?;
    // A first run starts at the newest message already in the inbox, by
    // Photon's clock rather than the phone's, so nothing from before the
    // bridge existed is answered and nothing after it is missed.
    let cursor = saved.unwrap_or_else(|| {
        let mut cursor = Cursor::default();
        for (id, created) in newest {
            cursor.admit(&id, created);
        }
        cursor
    });

    let platforms = if cfg.platforms.is_empty() {
        SPECTRUM_PLATFORMS.join(", ")
    } else {
        cfg.platforms.join(", ")
    };
    eprintln!(
        "aster remote: connected as {} on {platforms}, repo {}, mode {}",
        cfg.project_id,
        cfg.repo_root.display(),
        cfg.mode
    );
    if cfg.allowed_senders.is_empty() {
        eprintln!(
            "aster remote: no senders allowed yet; message the line once and restart with --sender <your handle>"
        );
    }

    let skills: Skills = Arc::new(discover_skill_commands(&cfg.repo_root));
    let mut cfg = cfg;
    if cfg.platforms.is_empty() {
        cfg.platforms = SPECTRUM_PLATFORMS.iter().map(|p| p.to_string()).collect();
    }
    let bridge = Arc::new(Bridge {
        api,
        cfg,
        skills,
        chats: Mutex::new(HashMap::new()),
        cursor: Mutex::new(cursor),
        cursor_path,
        told: Mutex::new(HashSet::new()),
        workers: Mutex::new(HashMap::new()),
    });
    bridge.save_cursor();
    tokio::spawn(watch_wakeups(Arc::clone(&bridge)));
    tokio::spawn(sweep_staged_files());
    poll_loop(bridge).await
}

/// Check the key against the inbox, returning the newest messages in it. A
/// refused key is fatal; anything else is the network, which a phone loses
/// and regains, so it is waited out.
async fn connect(api: &Api) -> Result<Vec<(String, i64)>> {
    let query = [
        ("direction", "received"),
        ("order", "desc"),
        ("pageSize", PAGE_SIZE),
    ];
    let mut wait = Duration::from_secs(2);
    loop {
        match api.list_messages(&query).await {
            Ok(page) => return Ok(page_ids(&page)),
            Err(e) => {
                if let Some(refused) = e.downcast_ref::<ApiError>()
                    && matches!(refused.status, 401 | 403 | 404)
                {
                    return Err(e).context(
                        "connecting to Photon; check the API key, its project:read and project:write permissions, and the project id",
                    );
                }
                tracing::warn!("photon is not reachable yet: {e:#}");
                tokio::time::sleep(wait).await;
                wait = (wait * 2).min(RETRY_AFTER_CAP);
            }
        }
    }
}

fn page_ids(page: &Value) -> Vec<(String, i64)> {
    page.get("items")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let id = item.get("id")?.as_str()?.to_string();
                    let created = parse_time(item.get("createdAt")?.as_str()?)?;
                    Some((id, created))
                })
                .collect()
        })
        .unwrap_or_default()
}

async fn poll_loop(bridge: Arc<Bridge>) -> Result<()> {
    let mut wait = POLL_FAST;
    loop {
        tokio::time::sleep(wait).await;
        wait = match poll_once(&bridge).await {
            Ok(true) => POLL_FAST,
            Ok(false) => (wait + Duration::from_secs(1)).min(POLL_SLOW),
            Err(e) => match e.downcast_ref::<ApiError>() {
                Some(refused) if matches!(refused.status, 401 | 403) => {
                    return Err(e).context("Photon stopped accepting the API key");
                }
                Some(refused) if refused.status == 429 => refused
                    .retry_after
                    .unwrap_or(Duration::from_secs(10))
                    .max(POLL_SLOW),
                _ => {
                    tracing::warn!("listMessages failed: {e:#}");
                    (wait * 2).clamp(POLL_SLOW, RETRY_AFTER_CAP)
                }
            },
        };
    }
}

/// Read everything since the cursor, answer what is new, and say whether
/// there was any.
async fn poll_once(bridge: &Arc<Bridge>) -> Result<bool> {
    let since = bridge.cursor.lock().expect("cursor lock").query_since();
    let platform = match bridge.cfg.platforms.as_slice() {
        [only] => Some(only.clone()),
        _ => None,
    };
    let mut token: Option<String> = None;
    let mut found = false;
    for _ in 0..MAX_PAGES {
        let mut query = vec![
            ("createdSince", since.as_str()),
            ("direction", "received"),
            ("order", "asc"),
            ("pageSize", PAGE_SIZE),
        ];
        if let Some(platform) = &platform {
            query.push(("platform", platform.as_str()));
        }
        if let Some(token) = &token {
            query.push(("pageToken", token.as_str()));
        }
        let page = bridge.api.list_messages(&query).await?;
        let items = page
            .get("items")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        for item in &items {
            let (Some(id), Some(created)) = (
                item.get("id").and_then(Value::as_str),
                item.get("createdAt")
                    .and_then(Value::as_str)
                    .and_then(parse_time),
            ) else {
                continue;
            };
            let fresh = bridge
                .cursor
                .lock()
                .expect("cursor lock")
                .admit(id, created);
            if !fresh {
                continue;
            }
            found = true;
            // Saved before acting, so a restart never runs a turn twice.
            bridge.save_cursor();
            if let Some(inbound) = parse_inbound(item, &bridge.cfg.platforms) {
                dispatch(bridge, inbound);
            }
        }
        bridge.save_cursor();
        token = page
            .get("nextPageToken")
            .and_then(Value::as_str)
            .map(str::to_string);
        if token.is_none() {
            break;
        }
    }
    Ok(found)
}

/// Hand a message to its chat's worker, starting one when the chat has none.
/// A chat's messages are handled one at a time and in order, while the poll
/// moves straight on.
fn dispatch(bridge: &Arc<Bridge>, inbound: Inbound) {
    let mut workers = bridge.workers.lock().expect("workers lock");
    let inbound = match workers.get(&inbound.chat) {
        Some(worker) => match worker.send(inbound) {
            Ok(()) => return,
            Err(mpsc::error::SendError(inbound)) => inbound,
        },
        None => inbound,
    };
    let chat = inbound.chat.clone();
    let (worker, mut waiting) = mpsc::unbounded_channel();
    workers.insert(chat.clone(), worker);
    let bridge = Arc::clone(bridge);
    tokio::spawn(async move {
        let mut next = Some(inbound);
        while let Some(inbound) = next {
            handle_inbound(&bridge, inbound).await;
            // Checked under the lock `dispatch` sends under, so a message
            // either lands here or finds no worker and starts a new one.
            let mut workers = bridge.workers.lock().expect("workers lock");
            next = waiting.try_recv().ok();
            if next.is_none() {
                workers.remove(&chat);
            }
        }
    });
}

async fn handle_inbound(bridge: &Arc<Bridge>, inbound: Inbound) {
    let Inbound {
        id,
        chat,
        agent,
        sender,
        parts,
    } = inbound;
    if !is_allowed(&bridge.cfg.allowed_senders, &chat.platform, &sender) {
        let first = bridge
            .told
            .lock()
            .expect("told lock")
            .insert(normalize_handle(&sender));
        match stranger_reply(&bridge.cfg.allowed_senders, &sender) {
            Some(text) if first => {
                bridge.chat(&chat, |state| state.agent_user = Some(agent.clone()));
                bridge
                    .outbox(&chat, Some(&id), "stranger")
                    .text(&text)
                    .await;
            }
            Some(_) => {}
            None => tracing::warn!(
                "ignored a message from a sender outside the allowlist on {}",
                chat.platform
            ),
        }
        return;
    }
    bridge.chat(&chat, |state| {
        state.touched = Some(Instant::now());
        state.agent_user = Some(agent.clone());
        state.last_seen = Some(id.clone());
    });
    bridge.cursor.lock().expect("cursor lock").last = Some(LastChat {
        chat: chat.clone(),
        agent,
    });

    let mut texts = Vec::new();
    let mut files = Vec::new();
    for part in parts {
        match part {
            Part::Text(text) => texts.push(text),
            Part::Attachment(attachment) => match stage_attachment(bridge, &attachment).await {
                Ok(path) => files.push(format!("@{path}")),
                Err(e) => {
                    tracing::warn!("attachment {attachment} not staged: {e:#}");
                    bridge
                        .outbox(&chat, Some(&id), "intake")
                        .text("That file didn't reach me. Send it again?")
                        .await;
                    return;
                }
            },
        }
    }
    let text = texts.join("\n");
    let trimmed = text.trim();

    if files.is_empty()
        && let Some(command) = trimmed.strip_prefix('/')
    {
        let (name, arg) = match command.split_once(char::is_whitespace) {
            Some((name, arg)) => (name, arg.trim()),
            None => (command, ""),
        };
        handle_command(bridge, &chat, &id, &name.to_lowercase(), arg).await;
        return;
    }

    // A pending approval or question claims the next plain message. A reply
    // that is not an answer at all still settles an approval, as a no, and
    // then goes on to the agent as what the person said instead.
    let pending = bridge.chat(&chat, |state| state.pending.take());
    match pending {
        Some(Pending::Approval(respond)) if files.is_empty() => {
            let answer = parse_approval(trimmed);
            let _ = respond.send(answer.unwrap_or(Answer::Deny));
            if answer.is_some() {
                return;
            }
        }
        Some(Pending::Question { options, respond }) if files.is_empty() => {
            let _ = respond.send(resolve_choice(trimmed, &options));
            return;
        }
        Some(Pending::Approval(respond)) => {
            let _ = respond.send(Answer::Deny);
        }
        Some(Pending::Question { respond, .. }) => {
            let _ = respond.send(None);
        }
        None => {}
    }

    let prompt = files
        .into_iter()
        .chain((!trimmed.is_empty()).then(|| trimmed.to_string()))
        .collect::<Vec<_>>()
        .join(" ");
    start_turn(bridge, &chat, Some(id), prompt);
}

/// Download an attachment to the temp dir, so the prompt can mention it and
/// the agent's `@path` attachment picks it up.
async fn stage_attachment(bridge: &Bridge, attachment: &str) -> Result<String> {
    let (bytes, content_type) = bridge.api.download(attachment).await?;
    let path = env::temp_dir().join("aster-pasted").join(format!(
        "{STAGED_PREFIX}{}.{}",
        Ulid::new(),
        extension_for(&content_type)
    ));
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(&path, bytes).await?;
    Ok(path.display().to_string())
}

/// Deletes staged files older than a day, so one that was never picked up by
/// a turn does not sit in temp storage forever.
async fn sweep_staged_files() {
    let dir = env::temp_dir().join("aster-pasted");
    loop {
        tokio::time::sleep(STAGED_SWEEP).await;
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let cutoff = SystemTime::now() - STAGED_TTL;
        for entry in entries.flatten() {
            let path = entry.path();
            let is_ours = path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(STAGED_PREFIX));
            let expired = entry
                .metadata()
                .and_then(|meta| meta.modified())
                .map(|modified| modified < cutoff)
                .unwrap_or(false);
            if is_ours && expired {
                let _ = tokio::fs::remove_file(&path).await;
            }
        }
    }
}

async fn handle_command(bridge: &Arc<Bridge>, chat: &ChatKey, id: &str, name: &str, arg: &str) {
    let outbox = bridge.outbox(chat, Some(id), "cmd");
    match name {
        "start" | "help" => outbox.text(&help(&bridge.cfg)).await,
        "new" | "clear" => {
            let agent = bridge.chat(chat, |state| {
                state.session = None;
                state.agent.clone()
            });
            if let Some(agent) = agent {
                agent.reset_session();
            }
            bridge.save_chat(chat);
            outbox.text("Started a fresh conversation.").await;
        }
        "stop" => stop_turn(bridge, chat, &outbox).await,
        "mode" => {
            let reply = set_choice(bridge, chat, arg, MODES, |state| &mut state.mode)
                .unwrap_or_else(|| {
                    let current = bridge
                        .chat(chat, |state| state.mode.clone())
                        .unwrap_or_else(|| bridge.cfg.mode.clone());
                    format!(
                        "Mode is {current}. Reply /mode with one of: {}.",
                        MODES.join(", ")
                    )
                });
            outbox.text(&reply).await;
        }
        "photos" => {
            let reply = set_choice(bridge, chat, arg, PHOTOS, |state| &mut state.photos)
                .unwrap_or_else(|| {
                    let current = photo_policy(bridge, chat);
                    format!(
                        "Screenshots are {current}. Reply /photos end to get the last one with each reply, on for every one, or off for none."
                    )
                });
            outbox.text(&reply).await;
        }
        "status" => {
            let (mode, busy) =
                bridge.chat(chat, |state| (state.mode.clone(), state.running.is_some()));
            let text = format!(
                "Workspace: {}\nMode: {}\nScreenshots: {}\nState: {}",
                workspace_name(&bridge.cfg.repo_root),
                mode.unwrap_or_else(|| bridge.cfg.mode.clone()),
                photo_policy(bridge, chat),
                if busy { "working" } else { "idle" }
            );
            outbox.text(&text).await;
        }
        "skills" => {
            let mut names: Vec<&String> = bridge.skills.keys().collect();
            names.sort();
            let text = if names.is_empty() {
                "No skills are installed.".to_string()
            } else {
                let names: Vec<String> = names.iter().map(|name| format!("/{name}")).collect();
                format!("Installed skills: {}", names.join(", "))
            };
            outbox.text(&text).await;
        }
        other => match bridge.skills.get(other) {
            Some(skill) => start_turn(bridge, chat, Some(id.to_string()), skill_prompt(skill, arg)),
            None => {
                outbox
                    .text("Unknown command. /help lists what I know.")
                    .await
            }
        },
    }
}

/// Set one of a chat's saved choices. None when no value was given, so the
/// caller can say what it is now instead.
fn set_choice(
    bridge: &Bridge,
    chat: &ChatKey,
    arg: &str,
    allowed: &[&str],
    slot: impl FnOnce(&mut ChatState) -> &mut Option<String>,
) -> Option<String> {
    let arg = arg.trim().to_lowercase();
    if arg.is_empty() {
        return None;
    }
    if !allowed.contains(&arg.as_str()) {
        return Some(format!("Expected one of: {}.", allowed.join(", ")));
    }
    bridge.chat(chat, |state| *slot(state) = Some(arg.clone()));
    bridge.save_chat(chat);
    Some(format!("Set to {arg}."))
}

fn photo_policy(bridge: &Bridge, chat: &ChatKey) -> String {
    bridge
        .chat(chat, |state| state.photos.clone())
        .unwrap_or_else(|| PHOTOS[0].to_string())
}

fn help(cfg: &SpectrumConfig) -> String {
    format!(
        "Aster remote control over chat.\n\
         Send a message to run the agent on {}.\n\
         Approvals arrive as questions; answer yes, always, or no.\n\n\
         /new starts a fresh conversation\n\
         /stop cancels the running task\n\
         /mode sets how the agent acts (plan, manual, auto, edit, yolo)\n\
         /photos sets which screenshots come back (end, on, off)\n\
         /status shows the mode and workspace\n\
         /skills lists installed skills\n\
         /help shows this message",
        workspace_name(&cfg.repo_root)
    )
}

fn start_turn(bridge: &Arc<Bridge>, chat: &ChatKey, reply_to: Option<String>, prompt: String) {
    let (mode, agent, session) =
        match bridge.chat(chat, |state| state.admit(reply_to.as_deref(), &prompt)) {
            Admission::Start {
                mode,
                agent,
                session,
            } => (mode, agent, session),
            Admission::Queued => return,
            Admission::Busy => {
                // "Stop" is an order to the bridge, not a note for the agent to weigh
                // against the job it is in the middle of. Everything else joins the
                // turn rather than starting a second conversation about the same work.
                if is_stop_request(&prompt) {
                    let bridge = Arc::clone(bridge);
                    let chat = chat.clone();
                    tokio::spawn(async move {
                        let outbox = bridge.outbox(&chat, reply_to.as_deref(), "cmd");
                        stop_turn(&bridge, &chat, &outbox).await;
                    });
                    return;
                }
                steer_running_turn(bridge, chat, reply_to, prompt);
                return;
            }
        };

    let turn = Turn {
        bin: bridge.cfg.bin.clone(),
        repo_root: bridge.cfg.repo_root.clone(),
        session: format!("spectrum-{}", chat.slug()),
        mode: mode.unwrap_or_else(|| bridge.cfg.mode.clone()),
        model: None,
        effort: None,
        extra_env: vec![],
    };
    eprintln!("[{}] user: {}", chat.slug(), console_text(&prompt, 200));
    let (events_tx, events_rx) = mpsc::channel::<TurnEvent>(8);
    let turn_bridge = Arc::clone(bridge);
    let turn_chat = chat.clone();
    let turn_task = tokio::spawn(async move {
        // One agent process per chat, kept across turns; a session saved
        // before a restart is loaded back rather than started over.
        let agent = match agent {
            Some(agent) => agent,
            None => {
                let agent = Agent::spawn(&turn).await?;
                turn_bridge.chat(&turn_chat, |state| state.agent = Some(Arc::clone(&agent)));
                agent
            }
        };
        let session_id = agent
            .ensure_session(&turn.repo_root, session.as_deref())
            .await?;
        if session.as_deref() != Some(&session_id) {
            turn_bridge.chat(&turn_chat, |state| state.session = Some(session_id.clone()));
            turn_bridge.save_chat(&turn_chat);
        }
        agent.configure(&session_id, &turn).await;
        // The standing rules go in with the first message of a session rather
        // than every one, so the recorded conversation stays conversation.
        let text = if agent.primed() {
            prompt
        } else {
            format!("{SPECTRUM_SYSTEM}\n\n{prompt}")
        };
        agent.prompt(&session_id, &text, &events_tx).await
    });
    bridge.chat(chat, |state| state.running = Some(turn_task.abort_handle()));

    let bridge = Arc::clone(bridge);
    let chat = chat.clone();
    tokio::spawn(async move {
        let outbox = bridge.outbox(&chat, reply_to.as_deref(), "turn");
        let result = drive_turn(&bridge, &chat, &outbox, events_rx, turn_task).await;
        finish_turn(&bridge, &chat, &outbox, reply_to.as_deref(), result).await;
        run_queued(&bridge, &chat);
    });
}

/// A message sent mid-turn goes to the turn in flight, so the agent hears it
/// while it works instead of picking it up as a second, contextless task. One
/// that arrives as the turn is ending waits and runs next.
fn steer_running_turn(
    bridge: &Arc<Bridge>,
    chat: &ChatKey,
    reply_to: Option<String>,
    prompt: String,
) {
    let agent = bridge.chat(chat, |state| {
        state.agent.clone().filter(|agent| agent.is_alive())
    });
    let bridge = Arc::clone(bridge);
    let chat = chat.clone();
    tokio::spawn(async move {
        let joined = match agent {
            Some(agent) => agent
                .steer(&format!("{ASIDE_NOTE}\n\n{prompt}"))
                .await
                .unwrap_or(false),
            None => false,
        };
        eprintln!(
            "[{}] {} {}",
            chat.slug(),
            if joined { "mid-turn" } else { "queued" },
            console_text(&prompt, 200)
        );
        if joined {
            return;
        }
        let orphaned = bridge.chat(&chat, |state| {
            state.queued.push((reply_to, prompt));
            state.running.is_none()
        });
        // The turn ended while this was in flight, so its finish already ran
        // whatever was waiting and nothing else will pick this up.
        if orphaned {
            run_queued(&bridge, &chat);
        }
    });
}

/// Whatever came in too late to join the last turn runs now, as one turn.
fn run_queued(bridge: &Arc<Bridge>, chat: &ChatKey) {
    let queued = bridge.chat(chat, |state| std::mem::take(&mut state.queued));
    if let Some((reply_to, prompt)) = merge_queued(queued) {
        start_turn(bridge, chat, reply_to, prompt);
    }
}

/// The waiting messages as one prompt, answered as a reply to the first of
/// them. None when nothing is waiting.
pub(crate) fn merge_queued(
    queued: Vec<(Option<String>, String)>,
) -> Option<(Option<String>, String)> {
    let reply_to = queued.first()?.0.clone();
    let prompt = queued
        .into_iter()
        .map(|(_, text)| text)
        .collect::<Vec<_>>()
        .join("\n\n");
    Some((reply_to, prompt))
}

async fn stop_turn(bridge: &Bridge, chat: &ChatKey, outbox: &Outbox) {
    let (queued, agent) = bridge.chat(chat, |state| (state.stop(), state.agent.clone()));
    let Some(queued) = queued else {
        outbox.text("Nothing to stop.").await;
        return;
    };
    // The agent process outlives the turn, so it has to be told as well, or
    // it keeps working on a message nobody is reading.
    if let Some(agent) = agent {
        agent.cancel().await;
    }
    let text = match queued {
        0 => "Stopped.".to_string(),
        1 => "Stopped, and dropped the message waiting behind it.".to_string(),
        n => format!("Stopped, and dropped the {n} messages waiting behind it."),
    };
    outbox.text(&text).await;
}

async fn drive_turn(
    bridge: &Arc<Bridge>,
    chat: &ChatKey,
    outbox: &Outbox,
    mut events: mpsc::Receiver<TurnEvent>,
    turn_task: tokio::task::JoinHandle<Result<TurnOutcome>>,
) -> Result<TurnOutcome> {
    let typing = outbox.agent.clone().map(|agent| {
        let bridge = Arc::clone(bridge);
        let chat = chat.clone();
        tokio::spawn(async move {
            loop {
                let waiting = bridge.chat(&chat, |state| state.pending.is_some());
                // A platform without typing says so once; asking again every
                // few seconds would only spend the rate limit.
                if !waiting && bridge.api.typing(&chat.space, &agent, true).await.is_err() {
                    break;
                }
                tokio::time::sleep(TYPING_EVERY).await;
            }
        })
    });

    let policy = photo_policy(bridge, chat);
    let mut held_shot: Option<String> = None;
    while let Some(event) = events.recv().await {
        match event {
            TurnEvent::ToolCall {
                name, arguments, ..
            } => {
                eprintln!("[{}] -> {}", chat.slug(), console_tool(&name, &arguments));
            }
            TurnEvent::ToolResult { error, output, .. } => {
                let shots = shot_paths(&output);
                match policy.as_str() {
                    "on" => {
                        for path in shots {
                            outbox.photo(&path).await;
                        }
                    }
                    "off" => {}
                    _ => {
                        if let Some(path) = shots.into_iter().next_back() {
                            held_shot = Some(path);
                        }
                    }
                }
                if error {
                    eprintln!("[{}]   tool failed", chat.slug());
                }
            }
            TurnEvent::Text { .. } | TurnEvent::Thought { .. } => {}
            TurnEvent::ApprovalRequest {
                preview,
                scope,
                respond,
            } => {
                eprintln!(
                    "[{}] approval needed: {}",
                    chat.slug(),
                    console_text(&preview, 120)
                );
                // Pending before the send, so an answer that beats a slow or
                // retried send still lands.
                bridge.chat(chat, |state| {
                    state.pending = Some(Pending::Approval(respond))
                });
                if !outbox
                    .verbatim(&approval_text(&preview, scope.as_deref()))
                    .await
                {
                    withdraw_pending(bridge, chat);
                }
            }
            TurnEvent::Question {
                header,
                question,
                options,
                respond,
            } => {
                eprintln!("[{}] ? {}", chat.slug(), console_text(&question, 120));
                let text = question_text(&header, &question, &options);
                bridge.chat(chat, |state| {
                    state.pending = Some(Pending::Question { options, respond })
                });
                if !outbox.verbatim(&text).await {
                    withdraw_pending(bridge, chat);
                }
            }
        }
    }
    let result = turn_task
        .await
        .unwrap_or_else(|e| Err(anyhow!("turn cancelled: {e}")));
    if let Some(typing) = typing {
        typing.abort();
        if let Some(agent) = &outbox.agent {
            let _ = bridge.api.typing(&chat.space, agent, false).await;
        }
    }
    // The last screenshot is the evidence the reply leans on, so it goes out
    // however the turn ended.
    if let Some(path) = held_shot {
        outbox.photo(&path).await;
    }
    match &result {
        Ok(outcome) => eprintln!(
            "[{}] done: {}",
            chat.slug(),
            console_text(outcome.reply.trim(), 200)
        ),
        Err(e) => eprintln!("[{}] turn failed: {e:#}", chat.slug()),
    }
    result
}

/// A prompt that never reached the person is answered as a no, so the turn
/// does not wait on it and their next message is not taken as the answer.
fn withdraw_pending(bridge: &Bridge, chat: &ChatKey) {
    match bridge.chat(chat, |state| state.pending.take()) {
        Some(Pending::Approval(respond)) => {
            let _ = respond.send(Answer::Deny);
        }
        Some(Pending::Question { respond, .. }) => {
            let _ = respond.send(None);
        }
        None => {}
    }
}

async fn finish_turn(
    bridge: &Bridge,
    chat: &ChatKey,
    outbox: &Outbox,
    reply_to: Option<&str>,
    result: Result<TurnOutcome>,
) {
    let (stopped, latest) = bridge.chat(chat, |state| {
        state.running = None;
        state.pending = None;
        (std::mem::take(&mut state.stopped), state.last_seen.clone())
    });
    // In a two-person chat an answer to the last thing said needs no quote;
    // one that lands after the person has moved on does.
    let quote = reply_to.filter(|id| latest.as_deref() != Some(*id));
    match result {
        Ok(outcome) => outbox.reply(&outcome.reply, quote).await,
        // /stop already confirmed; reporting the abort as a failure on top of
        // it would be a second, noisier message.
        Err(_) if stopped => {}
        Err(e) => {
            outbox
                .reply(
                    &format!("I couldn't finish that. Send it again to retry.\n\n{e:#}"),
                    quote,
                )
                .await
        }
    }
}

/// `<data home>/aster/wakeups/<id>.json`, written by the phone; a reminder
/// becomes one turn in the chat that last spoke, a notice is only posted.
fn wake_dir() -> Option<PathBuf> {
    let root = match env::var_os("XDG_DATA_HOME").filter(|dir| !dir.is_empty()) {
        Some(dir) => PathBuf::from(dir),
        None => PathBuf::from(env::var_os("HOME")?).join(".local/share"),
    };
    Some(root.join("aster").join("wakeups"))
}

async fn watch_wakeups(bridge: Arc<Bridge>) {
    let Some(dir) = wake_dir() else {
        return;
    };
    loop {
        tokio::time::sleep(WAKE_POLL).await;
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut files: Vec<PathBuf> = entries
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
            .collect();
        files.sort();
        for path in files {
            let wakeup = std::fs::read_to_string(&path)
                .ok()
                .and_then(|raw| parse_wakeup(&raw));
            let _ = std::fs::remove_file(&path);
            let Some(wakeup) = wakeup else {
                continue;
            };
            let Some(last) = bridge.latest_chat() else {
                tracing::warn!("wake-up with no chat to land in: {wakeup:?}");
                continue;
            };
            bridge.chat(&last.chat, |state| {
                state.agent_user.get_or_insert(last.agent.clone());
            });
            let outbox = bridge.outbox(&last.chat, None, "wake");
            match wakeup {
                Wakeup::Notice(text) => outbox.text(&text).await,
                Wakeup::Reminder(text) => {
                    // What lands in the chat is the errand in a line, never
                    // the whole instruction the scheduler wrote to itself.
                    outbox.text(&format!("⏰ {}", errand(&text))).await;
                    let prompt = format!(
                        "⏰ The reminder you set has fired: {text} Pick this up now and report back when it is done."
                    );
                    start_turn(&bridge, &last.chat, None, prompt);
                }
            }
        }
    }
}

/// A reminder in a line: its first sentence, short enough to read at a glance.
fn errand(text: &str) -> String {
    let first = text
        .split_once(". ")
        .map(|(head, _)| head)
        .unwrap_or(text)
        .trim();
    truncate(first, ERRAND_CHARS)
}

#[cfg(test)]
#[path = "tests/spectrum_test.rs"]
mod tests;
