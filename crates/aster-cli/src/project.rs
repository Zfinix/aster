//! A skim of the repository, read once at session start: its name, what it says it
//! does, its languages, where its packages and docs sit. Short enough for every
//! request, specific enough that the first turn talks about this project.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use ignore::WalkBuilder;
use serde_json::Value;

/// Package names listed per ecosystem before the rest become a count.
const MAX_PACKAGE_NAMES: usize = 6;
/// Ecosystems reported, largest first. Past this it is a build artifact or a
/// stray config, not what the repo is written in.
const MAX_STACKS: usize = 4;
/// A packageless language earns its line at this fraction of the files counted.
const MIN_STACK_SHARE: usize = 20;
/// Top-level directories listed before the rest become a count.
const MAX_TOP_LEVEL: usize = 16;
const MAX_DOCS: usize = 8;
/// Characters of description kept. Long enough for the opening claim, short
/// enough that it does not crowd the instructions that follow it.
const MAX_ABOUT_CHARS: usize = 320;
/// How deep to look for manifests. Deeper than this is a package's own vendored
/// tree more often than a workspace member.
const MAX_MANIFEST_DEPTH: usize = 3;
/// Entries the skim will walk. A monorepo of a million files must not hold the
/// UI up; counts past this are reported as approximate.
const MAX_WALK_ENTRIES: usize = 20_000;

const MANIFESTS: &[(&str, &str)] = &[
    ("Cargo.toml", "Rust"),
    ("package.json", "JavaScript/TypeScript"),
    ("deno.json", "JavaScript/TypeScript"),
    ("pyproject.toml", "Python"),
    ("setup.py", "Python"),
    ("go.mod", "Go"),
    ("pubspec.yaml", "Dart"),
    ("Gemfile", "Ruby"),
    ("composer.json", "PHP"),
    ("mix.exs", "Elixir"),
    ("Package.swift", "Swift"),
    ("pom.xml", "Java/Kotlin"),
    ("build.gradle", "Java/Kotlin"),
    ("build.gradle.kts", "Java/Kotlin"),
    ("CMakeLists.txt", "C/C++"),
];

/// Source extensions and the ecosystem each belongs to. The labels are the ones
/// [`MANIFESTS`] uses, so a count and its packages land on one line.
const EXTENSIONS: &[(&str, &str)] = &[
    ("rs", "Rust"),
    ("ts", "JavaScript/TypeScript"),
    ("tsx", "JavaScript/TypeScript"),
    ("js", "JavaScript/TypeScript"),
    ("jsx", "JavaScript/TypeScript"),
    ("mjs", "JavaScript/TypeScript"),
    ("svelte", "JavaScript/TypeScript"),
    ("vue", "JavaScript/TypeScript"),
    ("py", "Python"),
    ("go", "Go"),
    ("dart", "Dart"),
    ("rb", "Ruby"),
    ("php", "PHP"),
    ("ex", "Elixir"),
    ("exs", "Elixir"),
    ("swift", "Swift"),
    ("java", "Java/Kotlin"),
    ("kt", "Java/Kotlin"),
    ("c", "C/C++"),
    ("cc", "C/C++"),
    ("cpp", "C/C++"),
    ("h", "C/C++"),
    ("hpp", "C/C++"),
    ("sh", "Shell"),
    ("sql", "SQL"),
];

/// Dependency and build directories. A repo that does not ignore them still
/// should not have them read back as its own layout.
const NOT_LAYOUT: &[&str] = &[
    "node_modules",
    "target",
    "dist",
    "build",
    "out",
    "vendor",
    "venv",
    "coverage",
    "Pods",
];

/// Manifests that carry a description, and the dotted key holding it. Tried in
/// order when there is no README, or when the README is all decoration.
const DESCRIBED_BY: &[(&str, &str)] = &[
    ("Cargo.toml", "package.description"),
    ("Cargo.toml", "workspace.package.description"),
    ("package.json", "description"),
    ("pyproject.toml", "project.description"),
    ("pyproject.toml", "tool.poetry.description"),
    ("composer.json", "description"),
    ("pubspec.yaml", "description"),
];

const DOC_DIRS: &[&str] = &["docs", "doc", "documentation"];
const DOC_EXTENSIONS: &[&str] = &["md", "mdx", "rst"];

/// A `## Project` section for the system prompt, or `None` when the directory
/// says nothing about itself.
pub(crate) fn snapshot(repo_root: &Path) -> Option<String> {
    let skim = skim(repo_root);
    let mut note = String::from("## Project\n");
    if let Some(name) = repo_root.file_name().and_then(|n| n.to_str()) {
        note.push_str(&format!("- Name: {name}\n"));
    }
    if let Some(about) = about(repo_root) {
        note.push_str(&format!("- About: {about}\n"));
    }
    for (stack, body) in skim.stacks() {
        note.push_str(&format!("- {stack}: {body}\n"));
    }
    if !skim.top_level.is_empty() {
        note.push_str(&format!(
            "- Top level: {}\n",
            listed(&skim.top_level, MAX_TOP_LEVEL)
        ));
    }
    if let Some((dir, pages)) = docs(repo_root) {
        note.push_str(&format!("- Docs in {dir}/: {}\n", listed(&pages, MAX_DOCS)));
    }
    (note.lines().count() > 1).then_some(note)
}

/// One pass over the tree: how many files each ecosystem has, which directories
/// declare it, and what sits at the top level.
#[derive(Default)]
struct Skim {
    files: BTreeMap<&'static str, usize>,
    packages: BTreeMap<&'static str, Vec<String>>,
    top_level: Vec<String>,
    /// Set when the walk hit its cap, so the counts read as "600+".
    partial: bool,
}

impl Skim {
    /// One line per ecosystem, biggest first: its size, then where its packages
    /// live. A language with no package of its own has to hold a real share of
    /// the tree to earn a line; two stray shell scripts do not describe a repo.
    fn stacks(&self) -> Vec<(&'static str, String)> {
        let total: usize = self.files.values().sum();
        let mut stacks: Vec<&&str> = self
            .files
            .keys()
            .chain(self.packages.keys())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        stacks
            .sort_by_key(|stack| std::cmp::Reverse(self.files.get(**stack).copied().unwrap_or(0)));
        stacks
            .into_iter()
            .filter(|stack| {
                self.packages.contains_key(**stack)
                    || self.files.get(**stack).copied().unwrap_or(0) * MIN_STACK_SHARE >= total
            })
            .take(MAX_STACKS)
            .filter_map(|stack| {
                let mut parts = Vec::new();
                if let Some(count) = self.files.get(*stack) {
                    let more = if self.partial { "+" } else { "" };
                    let plural = if *count == 1 { "" } else { "s" };
                    parts.push(format!("{count}{more} file{plural}"));
                }
                if let Some(packages) = self.packages.get(*stack).and_then(|dirs| packaged(dirs)) {
                    parts.push(packages);
                }
                (!parts.is_empty()).then(|| (*stack, parts.join(", ")))
            })
            .collect()
    }
}

fn skim(repo_root: &Path) -> Skim {
    let mut skim = Skim::default();
    for (seen, entry) in walk(repo_root).flatten().enumerate() {
        if seen >= MAX_WALK_ENTRIES {
            skim.partial = true;
            break;
        }
        let Some(name) = entry.file_name().to_str() else {
            continue;
        };
        if entry.file_type().is_some_and(|t| t.is_dir()) {
            if entry.depth() == 1
                && let Some(dir) = relative(repo_root, entry.path())
            {
                skim.top_level.push(dir);
            }
            continue;
        }
        if entry.depth() <= MAX_MANIFEST_DEPTH
            && let Some((_, stack)) = MANIFESTS.iter().find(|(manifest, _)| *manifest == name)
            && let Some(dir) = entry.path().parent().and_then(|p| relative(repo_root, p))
        {
            let dirs = skim.packages.entry(stack).or_default();
            if !dirs.contains(&dir) {
                dirs.push(dir);
            }
        }
        if let Some(extension) = entry.path().extension().and_then(|e| e.to_str())
            && let Some((_, stack)) = EXTENSIONS.iter().find(|(known, _)| *known == extension)
        {
            *skim.files.entry(stack).or_default() += 1;
        }
    }
    skim.top_level.sort();
    for dirs in skim.packages.values_mut() {
        dirs.sort();
    }
    skim
}

/// Where an ecosystem's packages live. A workspace collapses to the directory
/// holding most of it and the member names, which is how someone would describe
/// it out loud; whatever sits outside that directory is named after it.
fn packaged(dirs: &[String]) -> Option<String> {
    let members: Vec<&str> = dirs
        .iter()
        .filter(|dir| *dir != ".")
        .map(String::as_str)
        .collect();
    if members.is_empty() {
        return None;
    }
    let mut by_parent: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for member in &members {
        by_parent
            .entry(member.rsplit_once('/').map_or("", |(parent, _)| parent))
            .or_default()
            .push(member);
    }
    let (parent, group) = by_parent.iter().max_by_key(|(_, group)| group.len())?;
    if parent.is_empty() || group.len() < 3 || group.len() * 2 <= members.len() {
        let all: Vec<String> = members.iter().map(|dir| dir.to_string()).collect();
        return Some(format!("packages in {}", listed(&all, MAX_PACKAGE_NAMES)));
    }
    let names: Vec<String> = group
        .iter()
        .map(|dir| dir.rsplit('/').next().unwrap_or(dir).to_string())
        .collect();
    let mut body = format!(
        "{} packages under {parent}/ ({})",
        group.len(),
        listed(&names, MAX_PACKAGE_NAMES)
    );
    let strays: Vec<String> = members
        .iter()
        .filter(|dir| !group.contains(dir))
        .map(|dir| dir.to_string())
        .collect();
    if !strays.is_empty() {
        body.push_str(&format!(", plus {}", listed(&strays, MAX_PACKAGE_NAMES)));
    }
    Some(body)
}

/// The documentation directory and its pages: the fastest read of what a
/// project explains about itself, and where a turn should look next.
fn docs(repo_root: &Path) -> Option<(String, Vec<String>)> {
    let dir = DOC_DIRS.iter().find(|dir| repo_root.join(dir).is_dir())?;
    let mut pages: Vec<String> = fs::read_dir(repo_root.join(dir))
        .ok()?
        .flatten()
        .filter(|entry| {
            entry
                .path()
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| DOC_EXTENSIONS.contains(&e))
        })
        .filter_map(|entry| entry.file_name().to_str().map(str::to_string))
        .collect();
    pages.sort();
    (!pages.is_empty()).then(|| (dir.to_string(), pages))
}

/// What the project says it does. Not every repo has a README, and not every
/// README opens with prose, so the manifests answer when it cannot.
fn about(repo_root: &Path) -> Option<String> {
    readme_about(repo_root).or_else(|| manifest_about(repo_root))
}

/// The README's opening prose. Headings, badges, and raw HTML are skipped: they
/// are decoration, and the first real sentence is what says what this is.
fn readme_about(repo_root: &Path) -> Option<String> {
    let body = readme(repo_root)?;
    let mut about = String::new();
    for line in body.lines().map(str::trim) {
        match prose(line) {
            true => {
                if !about.is_empty() {
                    about.push(' ');
                }
                about.push_str(line);
            }
            false if about.is_empty() => continue,
            false => break,
        }
        if about.chars().count() >= MAX_ABOUT_CHARS {
            break;
        }
    }
    (!about.is_empty()).then(|| truncate(&about, MAX_ABOUT_CHARS))
}

/// The README, however this repo spells it: the name is capitalized every way
/// there is, and the extension is sometimes absent.
fn readme(repo_root: &Path) -> Option<String> {
    let mut names: Vec<_> = fs::read_dir(repo_root)
        .ok()?
        .flatten()
        .map(|entry| entry.file_name())
        .filter(|name| {
            name.to_str()
                .is_some_and(|name| name.to_ascii_lowercase().starts_with("readme"))
        })
        .collect();
    names.sort();
    names
        .iter()
        .find_map(|name| fs::read_to_string(repo_root.join(name)).ok())
}

/// The description field of whichever root manifest declares one.
fn manifest_about(repo_root: &Path) -> Option<String> {
    DESCRIBED_BY.iter().find_map(|(file, key)| {
        let raw = fs::read_to_string(repo_root.join(file)).ok()?;
        let manifest: Value = match Path::new(file).extension()?.to_str()? {
            "toml" => toml::from_str(&raw).ok()?,
            "yaml" => serde_yaml::from_str(&raw).ok()?,
            _ => serde_json::from_str(&raw).ok()?,
        };
        let described = key
            .split('.')
            .try_fold(&manifest, |node, key| node.get(key))?;
        described
            .as_str()
            .map(|about| truncate(about.trim(), MAX_ABOUT_CHARS))
            .filter(|about| !about.is_empty())
    })
}

/// Whether a README line is prose rather than a heading, badge, table, list,
/// quote, code fence, or embedded HTML.
fn prose(line: &str) -> bool {
    !line.is_empty()
        && !line.starts_with(['#', '<', '>', '|', '-', '*', '='])
        && !line.starts_with("![")
        && !line.starts_with("[!")
        && !line.starts_with("```")
}

/// A walk that honors `.gitignore` whether or not this is a git repository, so
/// a plain folder does not report `target/` or `node_modules/` as its layout.
fn walk(root: &Path) -> ignore::Walk {
    WalkBuilder::new(root)
        .require_git(false)
        .filter_entry(|entry| {
            entry.depth() == 0
                || !entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| NOT_LAYOUT.contains(&name))
        })
        .build()
}

/// `path` relative to the root, with the root itself as `.`.
fn relative(repo_root: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(repo_root).ok()?.to_str()?;
    Some(match rel.is_empty() {
        true => ".".to_string(),
        false => rel.to_string(),
    })
}

/// `a, b, c, and 4 more`, so a wide monorepo cannot flood the prompt.
fn listed(items: &[String], cap: usize) -> String {
    let shown = items
        .iter()
        .take(cap)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    match items.len() > cap {
        true => format!("{shown}, and {} more", items.len() - cap),
        false => shown,
    }
}

fn truncate(text: &str, cap: usize) -> String {
    match text.chars().count() > cap {
        true => format!("{}…", text.chars().take(cap).collect::<String>().trim_end()),
        false => text.to_string(),
    }
}

#[cfg(test)]
#[path = "tests/project_test.rs"]
mod tests;
