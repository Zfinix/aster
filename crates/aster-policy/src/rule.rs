//! One rule language for every gated tool: `Edit(glob)`, `Read(glob)`,
//! `Bash(pattern)`. Rules sort into `allow`, `ask`, and `deny` and are the only
//! matcher the policy consults.

use anyhow::{Result, bail};
use globset::{Glob, GlobMatcher};

use crate::action::Action;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Subject {
    Edit,
    Read,
    Bash,
}

impl Subject {
    fn parse(name: &str) -> Option<Subject> {
        match name {
            "Edit" => Some(Subject::Edit),
            "Read" => Some(Subject::Read),
            "Bash" => Some(Subject::Bash),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
enum Match {
    Any,
    Path(GlobMatcher),
    CommandPrefix(String),
    CommandExact(String),
}

#[derive(Debug, Clone)]
pub struct Rule {
    subject: Subject,
    matcher: Match,
    source: String,
}

impl Rule {
    /// Parse `Tool(specifier)`, or a bare `Tool` covering everything it gates.
    pub fn parse(text: &str) -> Result<Rule> {
        let text = text.trim();
        let (name, specifier) = match text.split_once('(') {
            None => (text, None),
            Some((name, rest)) => match rest.strip_suffix(')') {
                None => bail!("rule `{text}` is missing its closing `)`"),
                Some(spec) => (name.trim_end(), Some(spec)),
            },
        };
        let Some(subject) = Subject::parse(name) else {
            bail!("rule `{text}` names `{name}`, which is not one of Edit, Read, or Bash")
        };
        let matcher = match (subject, specifier) {
            (_, None) | (_, Some("*")) => Match::Any,
            (Subject::Edit | Subject::Read, Some(spec)) => {
                let glob = Glob::new(spec)
                    .map_err(|e| anyhow::anyhow!("rule `{text}` has an invalid glob: {e}"))?;
                Match::Path(glob.compile_matcher())
            }
            (Subject::Bash, Some(spec)) => match spec.strip_suffix(":*") {
                Some(prefix) => Match::CommandPrefix(prefix.trim_end().to_string()),
                None => Match::CommandExact(spec.trim().to_string()),
            },
        };
        Ok(Rule {
            subject,
            matcher,
            source: text.to_string(),
        })
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    /// Whether this rule covers `action`. A command matches when any of its
    /// segments does, so a rule still applies to work hidden inside `bash -lc`.
    pub fn matches(&self, action: &Action) -> bool {
        match (self.subject, action) {
            (Subject::Edit, Action::Edit { path }) | (Subject::Read, Action::Read { path }) => {
                match &self.matcher {
                    Match::Any => true,
                    Match::Path(glob) => glob.is_match(path),
                    _ => false,
                }
            }
            (Subject::Bash, Action::Exec { binary, args }) => {
                let segments = crate::shell::segments(binary, args);
                segments.iter().any(|line| self.matches_line(line))
            }
            _ => false,
        }
    }

    fn matches_line(&self, line: &str) -> bool {
        match &self.matcher {
            Match::Any => true,
            Match::CommandExact(text) => line == text,
            // A prefix must end at a word boundary, so `Bash(rm:*)` does not
            // also match `rmdir`.
            Match::CommandPrefix(prefix) => match line.strip_prefix(prefix.as_str()) {
                Some("") => true,
                Some(rest) => rest.starts_with(char::is_whitespace),
                None => false,
            },
            Match::Path(_) => false,
        }
    }
}

/// Parse a whole bucket, naming the offending entry if one fails.
pub fn parse_all(rules: &[String]) -> Result<Vec<Rule>> {
    rules.iter().map(|r| Rule::parse(r)).collect()
}

/// The first rule in the list that covers `action`.
pub fn first_match<'a>(rules: &'a [Rule], action: &Action) -> Option<&'a Rule> {
    rules.iter().find(|rule| rule.matches(action))
}

#[cfg(test)]
#[path = "tests/rule_test.rs"]
mod tests;
