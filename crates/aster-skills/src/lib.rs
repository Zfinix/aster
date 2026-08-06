#![forbid(unsafe_code)]
//! Filesystem-based agent skills. A skill is a directory holding a `SKILL.md`:
//! YAML frontmatter (`name`, `description`) then a markdown instruction body.
//! Discovery loads only the frontmatter; the body is read on demand.
//!
//! Layout mirrors Claude Code: a project root (`.aster/skills`) overrides a
//! user-global root (`<config>/aster/skills`) on name collision.

pub mod agents;

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

/// The instruction file every skill directory must contain.
pub const SKILL_FILE: &str = "SKILL.md";

/// Spec limits on the frontmatter fields.
const MAX_NAME_LEN: usize = 64;
const MAX_DESCRIPTION_LEN: usize = 1024;

/// One discovered skill: its metadata plus the path to its `SKILL.md`.
#[derive(Debug, Clone)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub path: PathBuf,
}

impl Skill {
    /// The instructions below the frontmatter, read on demand. Returns the whole
    /// file when it has no frontmatter fence.
    pub fn load_body(&self) -> Result<String> {
        let raw = fs::read_to_string(&self.path)
            .with_context(|| format!("reading skill {}", self.path.display()))?;
        Ok(strip_frontmatter(&raw).trim().to_string())
    }
}

/// Skills discovered across one or more roots, deduped by name.
#[derive(Debug, Default, Clone)]
pub struct SkillSet {
    skills: Vec<Skill>,
}

impl SkillSet {
    /// Scan each root's immediate subdirectories for a `SKILL.md`. First name
    /// wins, so pass the project root before the global one. Unreadable roots and
    /// malformed skills are skipped with a warning, not fatal.
    pub fn discover(roots: &[PathBuf]) -> Self {
        let mut skills: Vec<Skill> = Vec::new();
        for root in roots {
            for skill in scan_root(root) {
                if skills.iter().any(|s| s.name == skill.name) {
                    tracing::debug!(
                        skill = %skill.name,
                        "shadowed by a higher-precedence skill of the same name"
                    );
                    continue;
                }
                skills.push(skill);
            }
        }
        skills.sort_by(|a, b| a.name.cmp(&b.name));
        Self { skills }
    }

    pub fn is_empty(&self) -> bool {
        self.skills.is_empty()
    }

    pub fn len(&self) -> usize {
        self.skills.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Skill> {
        self.skills.iter()
    }

    pub fn get(&self, name: &str) -> Option<&Skill> {
        self.skills.iter().find(|s| s.name == name)
    }

    /// The system-prompt block listing every skill by name and description, or
    /// `None` when none are installed. The model loads a body with `read_skill`.
    pub fn render_index(&self) -> Option<String> {
        if self.skills.is_empty() {
            return None;
        }
        let mut out = String::from(
            "## Skills\n\n\
            Skills are reusable instruction sets for specific tasks. Each is listed \
            below as a name and a description of what it does and when to use it. \
            When a request matches a skill's description, call `read_skill` with its \
            name to load the full instructions before acting. Ignore skills that are \
            not relevant.\n",
        );
        for skill in &self.skills {
            out.push_str(&format!(
                "\n- **{}**: {}",
                skill.name,
                first_sentences(&skill.description, INDEX_DESCRIPTION_CHARS)
            ));
        }
        Some(out)
    }
}

/// Description budget per skill in the index. The index is re-sent on every
/// round, so a hundred installed skills otherwise cost more than the rest of
/// the system prompt combined; `read_skill` still loads the full instructions.
const INDEX_DESCRIPTION_CHARS: usize = 180;

/// Trim to whole sentences under `max`, falling back to a word boundary. Keeps
/// the trigger phrasing models match on rather than cutting mid-word.
fn first_sentences(description: &str, max: usize) -> String {
    let description = description.trim();
    if description.len() <= max {
        return description.to_string();
    }
    let window = &description[..ceil_boundary(description, max)];
    if let Some(end) = window.rfind(". ") {
        return description[..=end].trim_end().to_string();
    }
    match window.rfind(' ') {
        Some(space) => format!("{}…", &description[..space]),
        None => format!("{window}…"),
    }
}

/// The largest char boundary at or below `max`.
fn ceil_boundary(text: &str, max: usize) -> usize {
    let mut cut = max.min(text.len());
    while !text.is_char_boundary(cut) {
        cut -= 1;
    }
    cut
}

use aster_models::SKIP_DIRS;

/// How deep to walk a source tree looking for skills. Repos nest skills a few
/// levels down (`skills/<category>/<name>/SKILL.md`); this bounds the search.
const MAX_FIND_DEPTH: usize = 6;

/// Recursively find every skill under `root` (deduped by name, first found wins).
/// Unlike [`SkillSet::discover`], this walks the whole tree, not just immediate
/// children. `full_depth` keeps descending into a skill dir to find nested skills;
/// otherwise it stops at the first `SKILL.md`.
pub fn find_skills(root: &Path, full_depth: bool) -> Vec<Skill> {
    find_skills_report(root, full_depth).0
}

/// Like [`find_skills`], but also returns one message per manifest that failed to
/// load, so a caller can explain an empty result instead of reporting a bare zero.
pub fn find_skills_report(root: &Path, full_depth: bool) -> (Vec<Skill>, Vec<String>) {
    let mut found = Vec::new();
    let mut skipped = Vec::new();
    walk(root, 0, full_depth, &mut found, &mut skipped);
    found.sort_by(|a, b| a.name.cmp(&b.name));
    found.dedup_by(|a, b| a.name == b.name);
    (found, skipped)
}

fn walk(
    dir: &Path,
    depth: usize,
    full_depth: bool,
    out: &mut Vec<Skill>,
    skipped: &mut Vec<String>,
) {
    let manifest = dir.join(SKILL_FILE);
    if manifest.is_file() {
        let dir_name = dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        match load_skill(&manifest, &dir_name) {
            Ok(skill) => out.push(skill),
            Err(e) => {
                tracing::warn!(path = %manifest.display(), "skipping skill: {e:#}");
                skipped.push(format!("{}: {e:#}", manifest.display()));
            }
        }
        if !full_depth {
            return;
        }
    }
    if depth >= MAX_FIND_DEPTH {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') || SKIP_DIRS.contains(&name.as_ref()) {
            continue;
        }
        walk(&path, depth + 1, full_depth, out, skipped);
    }
}

/// Copy a skill's whole directory (SKILL.md plus any bundled resources) into
/// `dest_root/<name>`. Returns the installed path. Refuses to clobber an existing
/// install unless `overwrite` is set.
pub fn install_skill(skill: &Skill, dest_root: &Path, overwrite: bool) -> Result<PathBuf> {
    let src = skill
        .path
        .parent()
        .context("skill manifest has no parent directory")?;
    let dest = dest_root.join(&skill.name);
    if dest.exists() {
        if !overwrite {
            bail!("{} is already installed", skill.name);
        }
        fs::remove_dir_all(&dest)
            .with_context(|| format!("removing existing {}", dest.display()))?;
    }
    fs::create_dir_all(dest_root).with_context(|| format!("creating {}", dest_root.display()))?;
    copy_dir(src, &dest)?;
    Ok(dest)
}

/// Delete an installed skill directory. Returns `false` when it was not present.
pub fn remove_skill(dest_root: &Path, name: &str) -> Result<bool> {
    let dest = dest_root.join(name);
    if !dest.exists() {
        return Ok(false);
    }
    fs::remove_dir_all(&dest).with_context(|| format!("removing {}", dest.display()))?;
    Ok(true)
}

fn copy_dir(src: &Path, dest: &Path) -> Result<()> {
    fs::create_dir_all(dest).with_context(|| format!("creating {}", dest.display()))?;
    for entry in fs::read_dir(src).with_context(|| format!("reading {}", src.display()))? {
        let entry = entry?;
        let from = entry.path();
        let to = dest.join(entry.file_name());
        if from.is_dir() {
            let name = entry.file_name();
            if SKIP_DIRS.contains(&name.to_string_lossy().as_ref()) {
                continue;
            }
            copy_dir(&from, &to)?;
        } else {
            fs::copy(&from, &to)
                .with_context(|| format!("copying {} to {}", from.display(), to.display()))?;
        }
    }
    Ok(())
}

/// Read one root's subdirectories into skills. A missing or unreadable root is an
/// empty result, not an error.
fn scan_root(root: &Path) -> Vec<Skill> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    let mut skills = Vec::new();
    for entry in entries.filter_map(|e| e.ok()) {
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        let manifest = dir.join(SKILL_FILE);
        if !manifest.is_file() {
            continue;
        }
        let dir_name = dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        match load_skill(&manifest, &dir_name) {
            Ok(skill) => skills.push(skill),
            Err(e) => tracing::warn!(path = %manifest.display(), "skipping skill: {e:#}"),
        }
    }
    skills
}

/// Parse and validate one `SKILL.md`. `dir_name` is the fallback identity when
/// the frontmatter omits `name`.
fn load_skill(manifest: &Path, dir_name: &str) -> Result<Skill> {
    let raw =
        fs::read_to_string(manifest).with_context(|| format!("reading {}", manifest.display()))?;
    let (name, description) = parse_frontmatter(&raw, dir_name)?;
    Ok(Skill {
        name,
        description,
        path: manifest.to_path_buf(),
    })
}

/// Extract `name` and `description` from the leading `---` frontmatter fence and
/// validate them. `name` falls back to the directory name when absent.
fn parse_frontmatter(raw: &str, dir_name: &str) -> Result<(String, String)> {
    let front = frontmatter(raw).context("missing `---` frontmatter fence")?;

    let mut name = None;
    let mut description = None;
    let mut lines = front.lines().peekable();
    while let Some(line) = lines.next() {
        // Indented lines belong to the value above, never open a key of their own.
        if line.starts_with([' ', '\t']) {
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = read_value(value.trim(), &mut lines);
        match key.trim() {
            "name" => name = Some(value),
            "description" => description = Some(value),
            _ => {}
        }
    }

    let name = name
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| dir_name.to_string());
    let name = slugify(&name);
    validate_name(&name)?;

    let description = description.unwrap_or_default().trim().to_string();
    if description.is_empty() {
        bail!("`description` is required and must be non-empty");
    }
    if description.len() > MAX_DESCRIPTION_LEN {
        bail!("`description` exceeds {MAX_DESCRIPTION_LEN} characters");
    }

    Ok((name, description))
}

/// One scalar: an inline value, a `>`/`|` block, or a value continued on the
/// indented lines below it. Blocks and continuations fold to a single line,
/// which is what both the prompt index and `/skills` want; `|` keeps its
/// newlines. Consumes the continuation lines so the caller resumes at the next
/// key.
fn read_value(inline: &str, lines: &mut std::iter::Peekable<std::str::Lines>) -> String {
    let (marker, literal) = match inline {
        ">" | ">-" | ">+" => (true, false),
        "|" | "|-" | "|+" => (true, true),
        _ => (false, false),
    };
    let mut parts: Vec<String> = match marker {
        true => Vec::new(),
        false => vec![unquote(inline).to_string()],
    };
    while let Some(next) = lines.peek() {
        if !next.trim().is_empty() && !next.starts_with([' ', '\t']) {
            break;
        }
        let line = lines.next().unwrap_or_default();
        parts.push(line.trim().to_string());
    }
    let joiner = if literal { "\n" } else { " " };
    parts
        .into_iter()
        .filter(|p| literal || !p.is_empty())
        .collect::<Vec<_>>()
        .join(joiner)
        .trim()
        .to_string()
}

/// The text between the opening `---` and the next `---` line, if the file opens
/// with a frontmatter fence.
fn frontmatter(raw: &str) -> Option<&str> {
    let rest = raw
        .strip_prefix("---\n")
        .or_else(|| raw.strip_prefix("---\r\n"))?;
    let end = rest.find("\n---")?;
    Some(&rest[..end])
}

/// Everything after the frontmatter fence, or the whole input when there is none.
fn strip_frontmatter(raw: &str) -> &str {
    let Some(rest) = raw
        .strip_prefix("---\n")
        .or_else(|| raw.strip_prefix("---\r\n"))
    else {
        return raw;
    };
    match rest.find("\n---") {
        Some(end) => {
            let after = &rest[end + "\n---".len()..];
            after
                .strip_prefix("\r\n")
                .or_else(|| after.strip_prefix('\n'))
                .unwrap_or(after)
                .trim_start_matches(['-', '\r', '\n'])
        }
        None => raw,
    }
}

fn unquote(value: &str) -> &str {
    let bytes = value.as_bytes();
    if bytes.len() >= 2
        && (bytes[0] == b'"' && bytes[bytes.len() - 1] == b'"'
            || bytes[0] == b'\'' && bytes[bytes.len() - 1] == b'\'')
    {
        &value[1..value.len() - 1]
    } else {
        value
    }
}

/// Skills in the wild title-case their `name` ("Simplified Technical English
/// (ASD-STE100)"); fold those to kebab-case rather than rejecting the skill.
fn slugify(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.extend(c.to_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').to_string()
}

/// Structural checks on `name`: kebab-case identity, bounded length. The
/// Anthropic-platform reserved-word rule is intentionally not enforced here;
/// aster skills are local and provider-neutral.
fn validate_name(name: &str) -> Result<()> {
    if name.len() > MAX_NAME_LEN {
        bail!("`name` exceeds {MAX_NAME_LEN} characters");
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        bail!("`name` must contain only lowercase letters, digits, and hyphens");
    }
    Ok(())
}

#[cfg(test)]
mod index_tests {
    use super::first_sentences;

    #[test]
    fn first_sentences_keeps_a_short_description_whole() {
        let text = "Review Rust code for style.";
        assert_eq!(first_sentences(text, 180), text);
    }

    #[test]
    fn first_sentences_cuts_at_a_sentence_end() {
        let long = format!("First sentence here. {}", "padding ".repeat(60));
        let cut = first_sentences(&long, 180);
        assert_eq!(cut, "First sentence here.");
    }

    #[test]
    fn first_sentences_falls_back_to_a_word_boundary() {
        let long = "word ".repeat(100);
        let cut = first_sentences(&long, 180);
        // The budget plus a three-byte ellipsis.
        assert!(cut.len() <= 183, "was {}", cut.len());
        assert!(cut.ends_with('…'));
        assert!(!cut.contains("wor…"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_skill(root: &Path, name: &str, body: &str) {
        let dir = root.join(name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(SKILL_FILE), body).unwrap();
    }

    #[test]
    fn a_folded_description_reads_as_one_line() {
        let tmp = tempfile::tempdir().unwrap();
        write_skill(
            tmp.path(),
            "writer",
            "---\nname: writer\ndescription: >\n  Write blog posts and essays.\n  TRIGGER when drafting a post.\n---\n\nbody",
        );
        let set = SkillSet::discover(&[tmp.path().to_path_buf()]);
        let skill = set.get("writer").unwrap();
        assert_eq!(
            skill.description,
            "Write blog posts and essays. TRIGGER when drafting a post."
        );
        assert_eq!(skill.load_body().unwrap(), "body");
    }

    #[test]
    fn a_literal_description_keeps_its_newlines() {
        let tmp = tempfile::tempdir().unwrap();
        write_skill(
            tmp.path(),
            "writer",
            "---\nname: writer\ndescription: |\n  line one\n  line two\n---\n\nbody",
        );
        let set = SkillSet::discover(&[tmp.path().to_path_buf()]);
        assert_eq!(set.get("writer").unwrap().description, "line one\nline two");
    }

    #[test]
    fn an_indented_key_does_not_end_a_folded_description() {
        let tmp = tempfile::tempdir().unwrap();
        write_skill(
            tmp.path(),
            "writer",
            "---\nname: writer\ndescription: >\n  Reflects conventions: agentic ledes.\n  More prose.\n---\n\nbody",
        );
        let set = SkillSet::discover(&[tmp.path().to_path_buf()]);
        assert_eq!(
            set.get("writer").unwrap().description,
            "Reflects conventions: agentic ledes. More prose."
        );
    }

    #[test]
    fn discovers_and_parses_frontmatter() {
        let tmp = tempfile::tempdir().unwrap();
        write_skill(
            tmp.path(),
            "pdf-tools",
            "---\nname: pdf-tools\ndescription: Work with PDFs. Use when the user mentions PDFs.\n---\n\n# PDF Tools\n\nDo the thing.",
        );
        let set = SkillSet::discover(&[tmp.path().to_path_buf()]);
        assert_eq!(set.len(), 1);
        let skill = set.get("pdf-tools").unwrap();
        assert!(skill.description.starts_with("Work with PDFs"));
        assert_eq!(skill.load_body().unwrap(), "# PDF Tools\n\nDo the thing.");
    }

    #[test]
    fn name_falls_back_to_directory() {
        let tmp = tempfile::tempdir().unwrap();
        write_skill(
            tmp.path(),
            "reviewer",
            "---\ndescription: Review code carefully. Use before merging.\n---\nbody",
        );
        let set = SkillSet::discover(&[tmp.path().to_path_buf()]);
        assert!(set.get("reviewer").is_some());
    }

    #[test]
    fn title_cased_name_is_slugified() {
        let tmp = tempfile::tempdir().unwrap();
        write_skill(
            tmp.path(),
            "ste",
            "---\nname: Simplified Technical English (ASD-STE100)\ndescription: Rewrite text.\n---\nbody",
        );
        let set = SkillSet::discover(&[tmp.path().to_path_buf()]);
        assert!(set.get("simplified-technical-english-asd-ste100").is_some());
    }

    #[test]
    fn skips_skill_without_description() {
        let tmp = tempfile::tempdir().unwrap();
        write_skill(tmp.path(), "broken", "---\nname: broken\n---\nbody");
        let set = SkillSet::discover(&[tmp.path().to_path_buf()]);
        assert!(set.is_empty());
    }

    #[test]
    fn project_root_shadows_global() {
        let project = tempfile::tempdir().unwrap();
        let global = tempfile::tempdir().unwrap();
        write_skill(
            project.path(),
            "dup",
            "---\nname: dup\ndescription: project version.\n---\nproject",
        );
        write_skill(
            global.path(),
            "dup",
            "---\nname: dup\ndescription: global version.\n---\nglobal",
        );
        let set = SkillSet::discover(&[project.path().to_path_buf(), global.path().to_path_buf()]);
        assert_eq!(set.len(), 1);
        assert_eq!(set.get("dup").unwrap().description, "project version.");
    }

    #[test]
    fn empty_set_renders_no_index() {
        assert!(SkillSet::default().render_index().is_none());
    }

    #[test]
    fn find_skills_walks_nested_trees() {
        let tmp = tempfile::tempdir().unwrap();
        let nested = tmp.path().join("skills").join("writing");
        fs::create_dir_all(&nested).unwrap();
        write_skill(
            &nested,
            "haiku",
            "---\nname: haiku\ndescription: Write haiku. Use for short poems.\n---\nbody",
        );
        let found = find_skills(tmp.path(), false);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "haiku");
    }

    #[test]
    fn install_copies_bundled_resources() {
        let src_root = tempfile::tempdir().unwrap();
        let dir = src_root.path().join("pdf");
        fs::create_dir_all(dir.join("scripts")).unwrap();
        fs::write(
            dir.join(SKILL_FILE),
            "---\nname: pdf\ndescription: Handle PDFs. Use for PDF files.\n---\nbody",
        )
        .unwrap();
        fs::write(dir.join("scripts").join("fill.py"), "print('hi')").unwrap();

        let dest = tempfile::tempdir().unwrap();
        let skill = &find_skills(src_root.path(), false)[0];
        let installed = install_skill(skill, dest.path(), false).unwrap();
        assert!(installed.join(SKILL_FILE).is_file());
        assert!(installed.join("scripts").join("fill.py").is_file());

        assert!(install_skill(skill, dest.path(), false).is_err());
        assert!(install_skill(skill, dest.path(), true).is_ok());

        assert!(remove_skill(dest.path(), "pdf").unwrap());
        assert!(!remove_skill(dest.path(), "pdf").unwrap());
    }
}
