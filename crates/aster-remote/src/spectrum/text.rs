//! Text in and out of a chat that shows no markdown: replies cut to message
//! size, approvals and questions as plain text, and the answers to them.

use crate::bridge::Answer;
use crate::channel::{ApprovalView, pack_lines};

/// A reply as plain-text messages of at most `limit` bytes. The chat apps on
/// this bridge show markdown as typed, so the markers that only make sense
/// rendered go, and a line longer than a whole message is cut, at a space
/// where there is one, rather than sent over the limit.
pub(crate) fn plain_chunks(text: &str, limit: usize) -> Vec<String> {
    pack_lines(text.lines().filter_map(plain_line), limit)
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

/// An approval as a text message. A yes allows the whole request, so the
/// preview goes in whole and is split across as many messages as it takes
/// rather than cut.
pub(crate) fn approval_text(preview: &str, scope: Option<&str>) -> String {
    let mut text = format!(
        "Needs your go-ahead:\n{}",
        ApprovalView::new(preview).text()
    );
    if let Some(scope) = scope.filter(|scope| !scope.trim().is_empty()) {
        text.push_str(&format!("\nScope: {scope}"));
    }
    text
}

/// The question an approval ends on, sent by itself once all of
/// [`approval_text`] is out.
pub(crate) const APPROVAL_ASK: &str = "Reply yes, always, or no.";

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
