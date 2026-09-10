//! Renders agent markdown as Telegram HTML, chunked under the message limit.

/// Convert markdown to Telegram-safe HTML chunks of at most `limit` chars. Code
/// fences become `<pre><code>`, inline code `<code>`, `**bold**` `<b>`, headers
/// bold lines, `>` quotes `<blockquote>`, and `[text](url)` links; chunks never
/// split inside a tag pair.
pub fn to_html_chunks(markdown: &str, limit: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();
    for block in parse_blocks(markdown) {
        for piece in block.render(limit) {
            if current.len() + piece.len() + 1 > limit && !current.is_empty() {
                chunks.push(std::mem::take(&mut current));
            }
            if !current.is_empty() {
                current.push('\n');
            }
            current.push_str(&piece);
        }
    }
    if !current.trim().is_empty() {
        chunks.push(current);
    }
    chunks
}

/// Escape the three characters Telegram HTML reserves.
pub fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

enum Block {
    Code { lang: String, body: String },
    Quote(String),
    Text(String),
}

impl Block {
    fn render(&self, limit: usize) -> Vec<String> {
        match self {
            Block::Code { lang, body } => code_chunks(&escape(body), lang, limit),
            Block::Quote(text) => {
                let inner = text.lines().map(render_line).collect::<Vec<_>>().join("\n");
                let budget = limit.saturating_sub(25).max(64);
                split_plain(&inner, budget)
                    .into_iter()
                    .map(|part| format!("<blockquote>{}</blockquote>", part.trim_end_matches('\n')))
                    .collect()
            }
            Block::Text(text) => {
                let html: Vec<String> = tighten(text).lines().map(render_line).collect();
                split_plain(&html.join("\n"), limit)
            }
        }
    }
}

fn code_chunks(code: &str, lang: &str, limit: usize) -> Vec<String> {
    // Telegram syntax-highlights a fence whose opening tag carries
    // `class="language-x"`; an unknown or absent language stays plain.
    let (open, close) = match safe_lang(lang) {
        Some(lang) => (
            format!("<pre><code class=\"language-{lang}\">"),
            "</code></pre>",
        ),
        None => ("<pre>".to_string(), "</pre>"),
    };
    let budget = limit.saturating_sub(open.len() + close.len()).max(64);
    split_plain(code, budget)
        .into_iter()
        .map(|part| format!("{open}{}{close}", part.trim_end_matches('\n')))
        .collect()
}

fn safe_lang(lang: &str) -> Option<&str> {
    let lang = lang.trim();
    (!lang.is_empty()
        && lang.len() <= 20
        && lang
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '_' | '#')))
    .then_some(lang)
}

/// Collapse runs of blank lines so paragraphs stop gluing inside one chunk.
fn tighten(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut blanks = 0;
    for line in text.lines() {
        if line.trim().is_empty() {
            blanks += 1;
            if blanks > 1 {
                continue;
            }
        } else {
            blanks = 0;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

fn parse_blocks(markdown: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut text = String::new();
    let mut quote = String::new();
    let mut code = None::<(String, String)>;
    for line in markdown.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            flush_quote(&mut quote, &mut blocks);
            match code.take() {
                Some((lang, body)) => blocks.push(Block::Code { lang, body }),
                None => {
                    flush_text(&mut text, &mut blocks);
                    let lang = trimmed.trim_start_matches('`').trim().to_string();
                    code = Some((lang, String::new()));
                }
            }
            continue;
        }
        if let Some((_, body)) = &mut code {
            body.push_str(line);
            body.push('\n');
            continue;
        }
        // Consecutive `>` lines become one blockquote, the way the chat shows it.
        if let Some(rest) = trimmed.strip_prefix('>') {
            flush_text(&mut text, &mut blocks);
            quote.push_str(rest.strip_prefix(' ').unwrap_or(rest));
            quote.push('\n');
            continue;
        }
        flush_quote(&mut quote, &mut blocks);
        text.push_str(line);
        text.push('\n');
    }
    flush_quote(&mut quote, &mut blocks);
    // An unclosed fence still renders as code rather than vanishing.
    if let Some((lang, body)) = code {
        blocks.push(Block::Code { lang, body });
    }
    flush_text(&mut text, &mut blocks);
    blocks
}

fn flush_text(text: &mut String, blocks: &mut Vec<Block>) {
    if !text.trim().is_empty() {
        blocks.push(Block::Text(std::mem::take(text)));
    }
    text.clear();
}

fn flush_quote(quote: &mut String, blocks: &mut Vec<Block>) {
    if !quote.trim().is_empty() {
        blocks.push(Block::Quote(std::mem::take(quote)));
    }
    quote.clear();
}

fn render_line(line: &str) -> String {
    let trimmed = line.trim_start();
    if let Some(header) = trimmed.strip_prefix('#') {
        let title = header.trim_start_matches('#').trim();
        return format!("<b>{}</b>", inline(title));
    }
    if let Some(item) = trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
    {
        let indent = &line[..line.len() - trimmed.len()];
        return format!("{indent}•  {}", inline(item));
    }
    inline(line)
}

fn inline(text: &str) -> String {
    let parts: Vec<&str> = text.split('`').collect();
    if parts.len().is_multiple_of(2) {
        // Unbalanced backticks: treat the line as plain text.
        return bold(&linkify(&escape(text)));
    }
    parts
        .iter()
        .enumerate()
        .map(|(i, part)| {
            if i % 2 == 1 {
                format!("<code>{}</code>", escape(part))
            } else {
                bold(&linkify(&escape(part)))
            }
        })
        .collect()
}

/// Turn `[label](url)` into a real link. Runs after escaping, so the url is
/// already attribute-safe apart from quotes, which we refuse rather than guess.
fn linkify(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('[') {
        let Some(close) = rest[start..].find("](") else {
            break;
        };
        let close = start + close;
        let Some(end) = rest[close..].find(')') else {
            break;
        };
        let end = close + end;
        let label = &rest[start + 1..close];
        let url = &rest[close + 2..end];
        if label.is_empty() || !is_safe_url(url) {
            out.push_str(&rest[..start + 1]);
            rest = &rest[start + 1..];
            continue;
        }
        out.push_str(&rest[..start]);
        out.push_str(&format!("<a href=\"{url}\">{label}</a>"));
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    out
}

fn is_safe_url(url: &str) -> bool {
    (url.starts_with("https://") || url.starts_with("http://"))
        && !url.contains('"')
        && !url.contains('<')
}

fn bold(text: &str) -> String {
    let parts: Vec<&str> = text.split("**").collect();
    if parts.len().is_multiple_of(2) {
        return text.to_string();
    }
    parts
        .iter()
        .enumerate()
        .map(|(i, part)| {
            if i % 2 == 1 {
                format!("<b>{part}</b>")
            } else {
                (*part).to_string()
            }
        })
        .collect()
}

fn split_plain(text: &str, limit: usize) -> Vec<String> {
    let limit = limit.max(64);
    let mut pieces = Vec::new();
    let mut current = String::new();
    for line in text.split_inclusive('\n') {
        if current.len() + line.len() > limit && !current.is_empty() {
            pieces.push(std::mem::take(&mut current));
        }
        let mut rest = line;
        while rest.len() > limit {
            let mut cut = limit;
            while !rest.is_char_boundary(cut) {
                cut -= 1;
            }
            pieces.push(rest[..cut].to_string());
            rest = &rest[cut..];
        }
        current.push_str(rest);
    }
    if !current.is_empty() {
        pieces.push(current);
    }
    pieces
}

#[cfg(test)]
#[path = "tests/markdown_test.rs"]
mod tests;
