//! Breaking a command into the lines a rule should actually see. `run_command` has
//! no shell, so the agent chains through `bash -lc "one && two"`; matching only the
//! outer binary would make every command look like `bash`.

use std::path::Path;

const SHELLS: &[&str] = &["sh", "bash", "zsh", "dash", "ksh", "ash", "fish"];

/// How far a shell inside a shell is followed. Three is past anything a model
/// writes on purpose and keeps a hostile nesting from looping.
const MAX_DEPTH: usize = 3;

/// The command lines a rule is matched against: the invocation itself, plus
/// every segment of a shell script it carries.
pub fn segments(binary: &str, args: &[&str]) -> Vec<String> {
    let tokens: Vec<String> = std::iter::once(binary.to_string())
        .chain(args.iter().map(|a| a.to_string()))
        .collect();
    let mut out = Vec::new();
    collect(&tokens, 0, &mut out);
    out
}

fn collect(tokens: &[String], depth: usize, out: &mut Vec<String>) {
    let tokens = without_env_prefix(tokens);
    if tokens.is_empty() {
        return;
    }
    let line = tokens.join(" ");
    if !out.contains(&line) {
        out.push(line);
    }
    if depth >= MAX_DEPTH {
        return;
    }
    let Some(script) = script_argument(tokens) else {
        return;
    };
    for part in split_operators(script) {
        let inner = tokenize(&part);
        if !inner.is_empty() {
            collect(&inner, depth + 1, out);
        }
    }
}

/// `FOO=1 sudo rm` is a `sudo` call. Leading assignments are dropped so a rule
/// naming the command still matches.
fn without_env_prefix(tokens: &[String]) -> &[String] {
    let skip = tokens
        .iter()
        .take_while(|t| {
            t.split_once('=').is_some_and(|(name, _)| {
                !name.is_empty()
                    && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                    && !name.starts_with(|c: char| c.is_ascii_digit())
            })
        })
        .count();
    &tokens[skip..]
}

/// The script a shell was handed, as in the `-c` of `bash -lc "…"`.
fn script_argument(tokens: &[String]) -> Option<&str> {
    if !SHELLS.contains(&command_name(tokens.first()?).as_str()) {
        return None;
    }
    tokens.iter().enumerate().skip(1).find_map(|(i, token)| {
        let flag = token.strip_prefix('-').filter(|f| !f.starts_with('-'))?;
        flag.contains('c')
            .then(|| tokens.get(i + 1))?
            .map(String::as_str)
    })
}

/// Split on the operators that separate one command from the next, leaving
/// quoted text alone.
fn split_operators(script: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut chars = script.chars().peekable();

    while let Some(c) = chars.next() {
        match quote {
            Some(q) => {
                if c == q {
                    quote = None;
                }
                current.push(c);
            }
            None => match c {
                '\'' | '"' => {
                    quote = Some(c);
                    current.push(c);
                }
                '\\' => {
                    current.push(c);
                    if let Some(next) = chars.next() {
                        current.push(next);
                    }
                }
                ';' | '\n' | '|' | '&' => {
                    // `&&` and `||` are two characters; the second is eaten
                    // here so it does not open an empty segment.
                    if chars.peek() == Some(&c) {
                        chars.next();
                    }
                    parts.push(std::mem::take(&mut current));
                }
                _ => current.push(c),
            },
        }
    }
    parts.push(current);
    parts.retain(|p| !p.trim().is_empty());
    parts
}

/// Split a line into words, honouring quotes and dropping them from the result
/// the way a shell would.
fn tokenize(line: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut started = false;
    let mut chars = line.chars();

    while let Some(c) = chars.next() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => current.push(c),
            None => match c {
                '\'' | '"' => {
                    quote = Some(c);
                    started = true;
                }
                '\\' => {
                    if let Some(next) = chars.next() {
                        current.push(next);
                        started = true;
                    }
                }
                c if c.is_whitespace() => {
                    if started || !current.is_empty() {
                        tokens.push(std::mem::take(&mut current));
                        started = false;
                    }
                }
                _ => {
                    current.push(c);
                    started = true;
                }
            },
        }
    }
    if started || !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

/// The bare program name: no directory, no `.exe`.
fn command_name(binary: &str) -> String {
    let name = Path::new(binary)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| binary.to_string());
    name.strip_suffix(".exe").unwrap_or(&name).to_string()
}

#[cfg(test)]
#[path = "tests/shell_test.rs"]
mod tests;
