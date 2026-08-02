//! Renders agent markdown as Telegram HTML, chunked under the message limit.

/// Convert markdown to Telegram-safe HTML chunks of at most `limit` chars.
///
/// Code fences become `<pre>`, inline code `<code>`, `**bold**` `<b>`, and
/// headers bold lines; chunks never split inside a tag pair.
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
mod tests {
    use super::*;

    #[test]
    fn render_code_fence_becomes_pre() {
        let chunks = to_html_chunks("before\n```rust\nlet x = 1;\n```\nafter", 4000);
        let html = chunks.join("\n");
        assert!(html.contains("<pre>let x = 1;</pre>"));
        assert!(html.contains("before"));
        assert!(html.contains("after"));
    }

    #[test]
    fn render_escapes_html_in_code() {
        let chunks = to_html_chunks("```\nVec<String>\n```", 4000);
        assert!(chunks[0].contains("Vec&lt;String&gt;"));
    }

    #[test]
    fn render_inline_code_and_bold() {
        let chunks = to_html_chunks("run `cargo test` for **all** crates", 4000);
        assert_eq!(
            chunks[0],
            "run <code>cargo test</code> for <b>all</b> crates"
        );
    }

    #[test]
    fn render_header_and_bullets() {
        let chunks = to_html_chunks("## Plan\n- first\n- second", 4000);
        assert_eq!(chunks[0], "<b>Plan</b>\n•  first\n•  second");
    }

    #[test]
    fn render_unbalanced_backticks_stay_plain() {
        let chunks = to_html_chunks("odd ` tick", 4000);
        assert_eq!(chunks[0], "odd ` tick");
    }

    #[test]
    fn chunking_splits_long_code_into_multiple_pre() {
        let code = format!("```\n{}\n```", "x".repeat(9000));
        let chunks = to_html_chunks(&code, 4000);
        assert!(chunks.len() >= 3);
        for chunk in &chunks {
            assert!(chunk.len() <= 4000);
            assert_eq!(
                chunk.matches("<pre>").count(),
                chunk.matches("</pre>").count()
            );
        }
    }

    #[test]
    fn chunking_never_exceeds_limit() {
        let text = format!("{}\n```\n{}\n```", "word ".repeat(2000), "y".repeat(5000));
        for chunk in to_html_chunks(&text, 4000) {
            assert!(chunk.len() <= 4000, "chunk was {}", chunk.len());
        }
    }
}
