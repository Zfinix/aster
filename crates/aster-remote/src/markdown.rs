//! Renders agent markdown as Telegram HTML, chunked under the message limit.

/// Convert markdown to Telegram-safe HTML chunks of at most `limit` chars. Code
/// fences become `<pre>`, inline code `<code>`, `**bold**` `<b>`, and headers bold
/// lines; chunks never split inside a tag pair.
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
    Code(String),
    Text(String),
}

impl Block {
    /// Render to one or more HTML strings, each at most `limit` chars.
    fn render(&self, limit: usize) -> Vec<String> {
        match self {
            Block::Code(code) => split_plain(&escape(code), limit.saturating_sub(11))
                .into_iter()
                .map(|part| format!("<pre>{}</pre>", part.trim_end_matches('\n')))
                .collect(),
            Block::Text(text) => {
                let html: Vec<String> = text.lines().map(render_line).collect();
                split_plain(&html.join("\n"), limit)
            }
        }
    }
}

fn parse_blocks(markdown: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut text = String::new();
    let mut code = None::<String>;
    for line in markdown.lines() {
        if line.trim_start().starts_with("```") {
            match code.take() {
                Some(done) => blocks.push(Block::Code(done)),
                None => {
                    if !text.trim().is_empty() {
                        blocks.push(Block::Text(std::mem::take(&mut text)));
                    }
                    text.clear();
                    code = Some(String::new());
                }
            }
            continue;
        }
        match &mut code {
            Some(code) => {
                code.push_str(line);
                code.push('\n');
            }
            None => {
                text.push_str(line);
                text.push('\n');
            }
        }
    }
    // An unclosed fence still renders as code rather than vanishing.
    if let Some(rest) = code {
        blocks.push(Block::Code(rest));
    }
    if !text.trim().is_empty() {
        blocks.push(Block::Text(text));
    }
    blocks
}

/// One markdown line to HTML: headers, bullets, inline code, and bold.
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

/// Inline code spans first (their content stays literal), then bold.
fn inline(text: &str) -> String {
    let parts: Vec<&str> = text.split('`').collect();
    if parts.len().is_multiple_of(2) {
        // Unbalanced backticks: treat the line as plain text.
        return bold(&escape(text));
    }
    parts
        .iter()
        .enumerate()
        .map(|(i, part)| {
            if i % 2 == 1 {
                format!("<code>{}</code>", escape(part))
            } else {
                bold(&escape(part))
            }
        })
        .collect()
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

/// Split HTML-free-of-unclosed-tags text into pieces of at most `limit`,
/// preferring newline boundaries and never splitting inside a char.
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
