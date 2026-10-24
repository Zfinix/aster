//! What comes in from Photon's inbox: the messages worth answering and who is
//! allowed to send them.

use serde_json::Value;

use super::ChatKey;

/// The platforms an allowlist entry can be scoped to.
const SCOPES: &[&str] = &[
    "imessage",
    "whatsapp",
    "whatsapp_business",
    "telegram",
    "sms",
    "email",
];

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
            Some((scope, wanted)) if SCOPES.iter().any(|p| p.eq_ignore_ascii_case(scope)) => {
                (Some(scope), wanted)
            }
            _ => (None, entry.as_str()),
        };
        scope.is_none_or(|scope| scope.eq_ignore_ascii_case(platform))
            && normalize_handle(wanted) == handle
    })
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
