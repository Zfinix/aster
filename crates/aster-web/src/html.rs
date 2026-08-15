//! The HTML handling both tools need: attribute reads, tag stripping, entity
//! and percent decoding. Deliberately small; a real parser is not worth the
//! dependency for two endpoints whose markup we already know.

/// Read `name="value"` out of an opening tag. Single and double quotes both
/// appear in DuckDuckGo's markup.
pub fn attr(tag: &str, name: &str) -> Option<String> {
    let at = tag.find(&format!("{name}="))? + name.len() + 1;
    let quote = tag[at..].chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let start = at + quote.len_utf8();
    let end = start + tag[start..].find(quote)?;
    Some(tag[start..end].to_string())
}

/// Strip tags, decode entities, and collapse runs of whitespace into one space.
pub fn to_text(fragment: &str) -> String {
    decode_entities(&strip_tags(fragment))
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Page text: like [`to_text`] but keeping paragraph breaks, and dropping the
/// contents of `script` and `style` rather than reading them as prose.
pub fn to_document(fragment: &str) -> String {
    let mut out = String::with_capacity(fragment.len());
    let mut rest = fragment;
    while let Some(open) = rest.find('<') {
        out.push_str(&rest[..open]);
        let tail = &rest[open..];
        let Some(end) = tail.find('>') else { break };
        let tag = &tail[1..end];
        let name = tag
            .trim_start_matches('/')
            .split(|c: char| c.is_whitespace() || c == '>')
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();

        if matches!(name.as_str(), "script" | "style" | "noscript") && !tag.starts_with('/') {
            let close = format!("</{name}");
            rest = match tail[end..].find(&close) {
                Some(at) => &tail[end + at..],
                None => "",
            };
            continue;
        }
        if BREAKING.contains(&name.as_str()) {
            out.push('\n');
        }
        rest = &tail[end + 1..];
    }
    out.push_str(rest);

    let text = decode_entities(&out);
    let mut lines: Vec<&str> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() && lines.last().is_some_and(|l: &&str| l.is_empty()) {
            continue;
        }
        lines.push(line);
    }
    lines.join("\n").trim().to_string()
}

const BREAKING: &[&str] = &[
    "p",
    "br",
    "div",
    "li",
    "tr",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "section",
    "article",
    "header",
    "footer",
    "blockquote",
    "pre",
];

fn strip_tags(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if in_tag => {}
            _ => out.push(c),
        }
    }
    out
}

pub fn decode_entities(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&#x27;", "'")
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
}

pub fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => match decode_pair(bytes, i) {
                Some(byte) => {
                    out.push(byte);
                    i += 3;
                }
                None => {
                    out.push(b'%');
                    i += 1;
                }
            },
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            byte => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn decode_pair(bytes: &[u8], at: usize) -> Option<u8> {
    let hi = hex(*bytes.get(at + 1)?)?;
    let lo = hex(*bytes.get(at + 2)?)?;
    Some(hi << 4 | lo)
}

fn hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
#[path = "tests/html_test.rs"]
mod tests;
