//! `aster skills`: manage agent skills, mirroring the `npx skills@latest`
//! surface. Sources include other agents' skills roots, keyed as in that CLI. Skills install user-global (`<config>/aster/skills`) by default, or
//! into this project (`.aster/skills`) with `-p`, where they shadow a global
//! skill of the same name. A git source is fetched lazily: a treeless partial
//! clone with only the `SKILL.md` manifests checked out, so browsing is cheap
//! and a skill's full contents download only when chosen.

use std::collections::BTreeMap;
use std::io::{self, IsTerminal};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::util::or_cancel;
use anyhow::{Context, Result, bail};
use aster_skills::agents::{Agent, agent_by_key, installed_agents};
use aster_skills::{Skill, SkillSet, find_skills_report, install_skill, remove_skill};
use clap::{Args, Subcommand};
use cliclack::{confirm, input, intro, log, outro, outro_cancel, select};
use console::{Term, style};
use serde::{Deserialize, Serialize};

#[derive(Args)]
pub struct SkillsArgs {
    #[command(subcommand)]
    command: Option<SkillsCommand>,
}

#[derive(Subcommand)]
enum SkillsCommand {
    /// Add skills from a repo, a local folder, or another agent.
    #[command(visible_alias = "a")]
    Add {
        /// Source: `owner/repo`, a git URL, a local path, or an agent key
        /// (`claude-code`, `cursor`, …). Omit for the wizard.
        source: Option<String>,
        /// Install into this project (`.aster/skills`) instead of the user-global root.
        #[arg(short = 'p', long)]
        project: bool,
        /// Install into the user-global root. The default; accepted for symmetry.
        #[arg(short = 'g', long, conflicts_with = "project")]
        global: bool,
        /// Install only these skills by name (repeat or comma-separate; `*` for all).
        #[arg(short = 's', long = "skill", value_delimiter = ',')]
        skill: Vec<String>,
        /// Install every skill found, no prompts.
        #[arg(long)]
        all: bool,
        /// Skip the confirmation prompt.
        #[arg(short = 'y', long)]
        yes: bool,
        /// List the skills a source offers without installing.
        #[arg(short = 'l', long)]
        list: bool,
        /// Search all subdirectories even when a directory already has a SKILL.md.
        #[arg(long)]
        full_depth: bool,
        /// Overwrite skills that are already installed.
        #[arg(long)]
        force: bool,
    },
    /// List installed skills.
    #[command(visible_alias = "ls")]
    List {
        /// List only this project's skills.
        #[arg(short = 'p', long)]
        project: bool,
        /// List only user-global skills.
        #[arg(short = 'g', long, conflicts_with = "project")]
        global: bool,
    },
    /// Remove installed skills (interactive when no name is given).
    #[command(visible_alias = "rm")]
    Remove {
        /// Skill names to remove.
        skills: Vec<String>,
        /// Remove from this project instead of the user-global root.
        #[arg(short = 'p', long)]
        project: bool,
        /// Remove from the user-global root. The default; accepted for symmetry.
        #[arg(short = 'g', long, conflicts_with = "project")]
        global: bool,
        /// Remove every installed skill.
        #[arg(long)]
        all: bool,
        /// Skip the confirmation prompt.
        #[arg(short = 'y', long)]
        yes: bool,
    },
    /// Print a skill's instructions to stdout without installing it.
    Use {
        /// `owner/repo@skill`, `owner/repo`, or an installed skill name.
        target: String,
        /// The skill to use when the target is a repo with several.
        #[arg(short = 's', long = "skill")]
        skill: Option<String>,
        /// Search all subdirectories even when a directory already has a SKILL.md.
        #[arg(long)]
        full_depth: bool,
    },
    /// Search GitHub for skills and install interactively.
    Find {
        /// Search terms.
        query: Option<String>,
        /// Restrict to repositories from this GitHub owner.
        #[arg(long)]
        owner: Option<String>,
        /// Install into this project instead of the user-global root.
        #[arg(short = 'p', long)]
        project: bool,
        /// Install into the user-global root. The default; accepted for symmetry.
        #[arg(short = 'g', long, conflicts_with = "project")]
        global: bool,
    },
    /// Update installed skills to their latest versions.
    #[command(visible_alias = "upgrade")]
    Update {
        /// Skill names to update; omit for all.
        skills: Vec<String>,
        /// Update this project's skills instead of the user-global ones.
        #[arg(short = 'p', long)]
        project: bool,
        /// Update user-global skills. The default; accepted for symmetry.
        #[arg(short = 'g', long, conflicts_with = "project")]
        global: bool,
    },
    /// List or install the optional skills bundled with aster.
    Bundled {
        /// Skill names to install; omit to list what is bundled.
        skills: Vec<String>,
        /// Install into this project (`.aster/skills`) instead of the user-global root.
        #[arg(short = 'p', long)]
        project: bool,
        /// Install into the user-global root. The default; accepted for symmetry.
        #[arg(short = 'g', long, conflicts_with = "project")]
        global: bool,
        /// Overwrite an already-installed copy.
        #[arg(long)]
        force: bool,
    },
    /// Scaffold a new skill (creates <name>/SKILL.md).
    Init {
        /// The skill name; omit to write ./SKILL.md.
        name: Option<String>,
    },
}

pub async fn run(args: SkillsArgs) -> Result<()> {
    let command = args.command.unwrap_or(SkillsCommand::List {
        project: false,
        global: false,
    });
    match command {
        SkillsCommand::Add {
            source,
            project,
            global: _,
            skill,
            all,
            yes,
            list,
            full_depth,
            force,
        } => add(AddOpts {
            project,
            source,
            skill,
            all,
            yes,
            list,
            full_depth,
            force,
        }),
        SkillsCommand::List { project, global } => list(project, global),
        SkillsCommand::Remove {
            skills,
            project,
            global: _,
            all,
            yes,
        } => remove(skills, project, all, yes),
        SkillsCommand::Use {
            target,
            skill,
            full_depth,
        } => use_skill(&target, skill.as_deref(), full_depth),
        SkillsCommand::Find {
            query,
            owner,
            project,
            global: _,
        } => find(query, owner, project).await,
        SkillsCommand::Update {
            skills,
            project,
            global: _,
        } => update(skills, project),
        SkillsCommand::Bundled {
            skills,
            project,
            global: _,
            force,
        } => bundled(skills, project, force),
        SkillsCommand::Init { name } => init(name.as_deref()),
    }
}

/// With names: install those bundled skills. Without: list the bundle, marking
/// what is already installed in either root.
fn bundled(names: Vec<String>, project: bool, force: bool) -> Result<()> {
    let scope = scope_of(project);
    if names.is_empty() {
        let installed: Vec<String> = [Scope::Project, Scope::Global]
            .into_iter()
            .filter_map(|s| scope_root(s).ok())
            .flat_map(|root| {
                SkillSet::discover(std::slice::from_ref(&root))
                    .iter()
                    .map(|s| s.name.clone())
                    .collect::<Vec<_>>()
            })
            .collect();
        println!("bundled optional skills (install with `aster skills bundled <name>`):\n");
        for skill in aster_skills::optional_skills() {
            let mark = if installed.contains(&skill.name) {
                " (installed)"
            } else {
                ""
            };
            println!("  {}{mark}\n    {}", skill.name, skill.description);
        }
        return Ok(());
    }
    let root = scope_root(scope)?;
    for name in &names {
        let dest = aster_skills::install_bundled(name, &root, force)?;
        println!(
            "installed {name} {} ({})",
            scope_phrase(scope),
            dest.display()
        );
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum Scope {
    Project,
    Global,
}

/// Global is the default: the agent reads both roots on every run, so a skill
/// installed once is available in every repo. `--project` opts into pinning a
/// skill to this checkout, where it shadows the global copy of the same name.
fn scope_of(project: bool) -> Scope {
    if project {
        Scope::Project
    } else {
        Scope::Global
    }
}

fn scope_root(scope: Scope) -> Result<PathBuf> {
    match scope {
        Scope::Global => Ok(crate::persist::home()?.join("skills")),
        Scope::Project => Ok(std::env::current_dir()
            .context("could not determine the current directory")?
            .join(".aster")
            .join("skills")),
    }
}

fn scope_word(scope: Scope) -> &'static str {
    match scope {
        Scope::Project => "project",
        Scope::Global => "global",
    }
}

fn other_scope(scope: Scope) -> Scope {
    match scope {
        Scope::Project => Scope::Global,
        Scope::Global => Scope::Project,
    }
}

/// The flag that would have targeted the other root, for error messages.
fn other_scope_flag(scope: Scope) -> &'static str {
    match scope {
        Scope::Project => "--global",
        Scope::Global => "--project",
    }
}

/// Reads as a phrase in a sentence, unlike [`scope_word`].
fn scope_phrase(scope: Scope) -> &'static str {
    match scope {
        Scope::Project => "in this project",
        Scope::Global => "globally",
    }
}

fn other_scope_has(scope: Scope, name: &str) -> bool {
    let Ok(root) = scope_root(other_scope(scope)) else {
        return false;
    };
    SkillSet::discover(std::slice::from_ref(&root))
        .iter()
        .any(|s| s.name == name)
}

struct AddOpts {
    source: Option<String>,
    project: bool,
    skill: Vec<String>,
    all: bool,
    yes: bool,
    list: bool,
    full_depth: bool,
    force: bool,
}

fn add(opts: AddOpts) -> Result<()> {
    let scope = scope_of(opts.project);
    let dest = scope_root(scope)?;
    let tty = is_tty();

    let source = match opts.source.clone() {
        Some(s) => s,
        None if tty => {
            intro("Add skills")?;
            match prompt_source()? {
                Some(s) => s,
                None => return cancel(),
            }
        }
        None => {
            bail!("a source is required (owner/repo, a git URL, a local path, or an agent key)")
        }
    };

    let (src, mut skills) = resolve_and_list(&source, opts.full_depth, tty)?;
    if skills.is_empty() {
        let msg = "No skills (SKILL.md folders) found at that source";
        if tty {
            outro_cancel(msg)?;
        } else {
            bail!("{msg}");
        }
        return Ok(());
    }

    if opts.list {
        if crate::json_mode() {
            emit_json(serde_json::json!({
                "source": source,
                "skills": skill_values(&skills),
            }));
        } else {
            print_available(&skills);
        }
        return Ok(());
    }

    let Some(chosen) = select_skills("Select skills to install", &skills, &opts, tty)? else {
        return cancel();
    };
    if chosen.is_empty() {
        return cancel();
    }

    if tty && !opts.yes {
        let ok = or_cancel(
            confirm(format!(
                "Install {} skill(s) into the {} scope?",
                chosen.len(),
                scope_word(scope)
            ))
            .initial_value(true)
            .interact(),
        )?;
        if ok != Some(true) {
            return cancel();
        }
    }

    let installed = fetch_and_install(&src, &chosen, &dest, opts.force, &source)?;
    if crate::json_mode() {
        emit_json(serde_json::json!({
            "ok": true,
            "source": source,
            "scope": scope_word(scope),
            "root": dest.display().to_string(),
            "installed": installed,
            "skills": skill_values(&chosen),
        }));
        return Ok(());
    }
    let done = format!("Installed {installed} skill(s) into {}", dest.display());
    if tty {
        outro(done)?;
    } else {
        println!("{done}");
    }
    // Keep the discovered list from being flagged as unused on the headless path.
    let _ = &mut skills;
    Ok(())
}

/// Resolve a source and enumerate its skills, with a spinner in a terminal.
fn resolve_and_list(source: &str, full_depth: bool, tty: bool) -> Result<(Source, Vec<Skill>)> {
    let spinner = tty.then(|| {
        let s = cliclack::spinner();
        s.start(format!("Reading skills from {source}"));
        s
    });
    let src = match resolve_source(source) {
        Ok(s) => s,
        Err(e) => {
            if let Some(s) = spinner {
                s.error("could not read source");
            }
            return Err(e);
        }
    };
    let (mut skills, skipped) = src.list_report(full_depth);
    src.filter(&mut skills);
    if let Some(s) = spinner {
        s.stop(format!("Found {} skill(s)", skills.len()));
    }
    if skills.is_empty() {
        for reason in &skipped {
            eprintln!("skipped {reason}");
        }
    }
    Ok((src, skills))
}

/// Pick which skills to install from flags, or interactively.
fn select_skills(
    title: &str,
    skills: &[Skill],
    opts: &AddOpts,
    tty: bool,
) -> Result<Option<Vec<Skill>>> {
    if opts.all || opts.skill.iter().any(|s| s == "*") {
        return Ok(Some(skills.to_vec()));
    }
    if !opts.skill.is_empty() {
        let mut chosen = Vec::new();
        for name in &opts.skill {
            let skill = skills
                .iter()
                .find(|s| &s.name == name)
                .with_context(|| format!("no skill named {name:?} in the source"))?;
            chosen.push(skill.clone());
        }
        return Ok(Some(chosen));
    }
    if tty {
        return choose_skills(title, skills);
    }
    bail!("specify --skill <names> or --all when not attached to a terminal");
}

/// Download the chosen skills, install them, and record their origin for updates.
fn fetch_and_install(
    src: &Source,
    chosen: &[Skill],
    dest: &Path,
    force: bool,
    source: &str,
) -> Result<usize> {
    src.materialize(chosen)?;
    let installed = install_all(chosen, dest, force)?;
    record_installed(dest, source, chosen);
    Ok(installed)
}

fn install_all(skills: &[Skill], dest: &Path, force: bool) -> Result<usize> {
    let mut count = 0;
    for skill in skills {
        match install_skill(skill, dest, force) {
            Ok(_) => {
                if !crate::json_mode() {
                    let _ = log::success(format!("installed {}", skill.name));
                }
                count += 1;
            }
            Err(e) => {
                if crate::json_mode() {
                    tracing::warn!("skipped {}: {e:#}", skill.name);
                } else {
                    let _ = log::warning(format!("skipped {}: {e:#}", skill.name));
                }
            }
        }
    }
    Ok(count)
}

fn print_available(skills: &[Skill]) {
    let width = width();
    let namew = skills.iter().map(|s| s.name.len()).max().unwrap_or(0);
    for skill in skills {
        let room = width.saturating_sub(namew + 4).clamp(20, 120);
        let desc = console::truncate_str(first_line(&skill.description), room, "…");
        println!("  {:<namew$}  {}", skill.name, style(desc).dim());
    }
}

/// Both scopes by default, because that is what the agent loads. `-p` or `-g`
/// narrows to one. A project skill shadowing a global one is called out, since
/// otherwise the global copy looks active when it is not.
fn list(project_only: bool, global_only: bool) -> Result<()> {
    let scopes: &[Scope] = match (project_only, global_only) {
        (true, _) => &[Scope::Project],
        (_, true) => &[Scope::Global],
        _ => &[Scope::Project, Scope::Global],
    };

    let mut sections = Vec::new();
    for &scope in scopes {
        let root = scope_root(scope)?;
        let set = SkillSet::discover(std::slice::from_ref(&root));
        sections.push((scope, root, set.iter().cloned().collect::<Vec<Skill>>()));
    }

    let shadowed: Vec<String> = match scopes.len() {
        2 => {
            let project: Vec<&str> = sections[0].2.iter().map(|s| s.name.as_str()).collect();
            sections[1]
                .2
                .iter()
                .filter(|s| project.contains(&s.name.as_str()))
                .map(|s| s.name.clone())
                .collect()
        }
        _ => Vec::new(),
    };

    if crate::json_mode() {
        emit_json(serde_json::json!({
            "scopes": sections
                .iter()
                .map(|(scope, root, skills)| serde_json::json!({
                    "scope": scope_word(*scope),
                    "root": root.display().to_string(),
                    "skills": skill_values(skills),
                }))
                .collect::<Vec<_>>(),
            "shadowed": shadowed,
        }));
        return Ok(());
    }

    let total: usize = sections.iter().map(|(_, _, s)| s.len()).sum();
    if total == 0 {
        for (scope, root, _) in &sections {
            println!("no {} skills in {}", scope_word(*scope), root.display());
        }
        println!("install some with: aster skills add");
        return Ok(());
    }

    for (scope, root, skills) in &sections {
        if skills.is_empty() {
            println!("no {} skills in {}\n", scope_word(*scope), root.display());
            continue;
        }
        println!("{} {} skill(s):\n", skills.len(), scope_word(*scope));
        print_available(skills);
        println!();
    }

    if !shadowed.is_empty() {
        println!("shadowed by this project: {}", shadowed.join(", "));
    }
    Ok(())
}

fn remove(names: Vec<String>, project: bool, all: bool, yes: bool) -> Result<()> {
    let scope = scope_of(project);
    let root = scope_root(scope)?;
    let set = SkillSet::discover(std::slice::from_ref(&root));
    let tty = is_tty();

    let targets: Vec<String> = if all {
        set.iter().map(|s| s.name.clone()).collect()
    } else if !names.is_empty() {
        names
    } else if tty {
        if set.is_empty() {
            println!("no {} skills to remove", scope_word(scope));
            return Ok(());
        }
        let installed: Vec<Skill> = set.iter().cloned().collect();
        match choose_skills("Select skills to remove", &installed)? {
            Some(chosen) => chosen.into_iter().map(|s| s.name).collect(),
            None => return cancel(),
        }
    } else {
        bail!("specify skill names or --all");
    };

    if targets.is_empty() {
        if crate::json_mode() {
            emit_json(serde_json::json!({ "ok": true, "removed": [], "missing": [] }));
        } else {
            println!("nothing to remove");
        }
        return Ok(());
    }

    if tty && !yes {
        let ok = or_cancel(
            confirm(format!("Remove {} skill(s)?", targets.len()))
                .initial_value(true)
                .interact(),
        )?;
        if ok != Some(true) {
            return cancel();
        }
    }

    let json = crate::json_mode();
    let (mut removed, mut missing) = (Vec::new(), Vec::new());
    for name in &targets {
        if remove_skill(&root, name)? {
            removed.push(name.clone());
            if !json {
                println!("removed {name}");
            }
        } else {
            missing.push(name.clone());
            if !json {
                // The scopes are separate roots, so name the other one rather
                // than reporting a skill absent when it is merely elsewhere.
                match other_scope_has(scope, name) {
                    true => eprintln!(
                        "no {} skill named {name:?}; it is installed {}. Retry with {}",
                        scope_word(scope),
                        scope_phrase(other_scope(scope)),
                        other_scope_flag(scope)
                    ),
                    false => eprintln!("no installed skill named {name:?}"),
                }
            }
        }
    }
    forget_installed(&root, &targets);
    if json {
        emit_json(serde_json::json!({
            "ok": true,
            "scope": scope_word(scope),
            "root": root.display().to_string(),
            "removed": removed,
            "missing": missing,
        }));
    }
    Ok(())
}

fn use_skill(target: &str, skill: Option<&str>, full_depth: bool) -> Result<()> {
    if let Some((pkg, name)) = target.split_once('@') {
        return print_from_source(pkg, Some(name), full_depth);
    }
    if looks_like_source(target) {
        return print_from_source(target, skill, full_depth);
    }
    let body = read_installed(target)?;
    emit_body(target, &body);
    Ok(())
}

/// Print one skill's body pulled straight from a source, without installing.
fn print_from_source(pkg: &str, skill: Option<&str>, full_depth: bool) -> Result<()> {
    let src = resolve_source(pkg)?;
    let mut skills = src.list(full_depth);
    src.filter(&mut skills);
    let target = match skill {
        Some(name) => skills
            .into_iter()
            .find(|s| s.name == name)
            .with_context(|| format!("no skill named {name:?} in {pkg}"))?,
        None if skills.len() == 1 => skills.remove(0),
        None => bail!(
            "{pkg} has {} skills; choose one with pkg@skill or --skill",
            skills.len()
        ),
    };
    src.materialize(std::slice::from_ref(&target))?;
    emit_body(&target.name, &target.load_body()?);
    Ok(())
}

/// Read an installed skill's body, project scope shadowing global.
fn read_installed(name: &str) -> Result<String> {
    for scope in [Scope::Project, Scope::Global] {
        let root = scope_root(scope)?;
        let set = SkillSet::discover(std::slice::from_ref(&root));
        if let Some(skill) = set.get(name) {
            return skill.load_body();
        }
    }
    bail!("no installed skill named {name:?}")
}

/// A skill's instructions: raw markdown, or wrapped in JSON for a caller.
fn emit_body(name: &str, body: &str) {
    if crate::json_mode() {
        emit_json(serde_json::json!({ "name": name, "body": body }));
    } else {
        print!("{body}");
    }
}

fn looks_like_source(s: &str) -> bool {
    s.contains('/') || s.starts_with("http") || s.starts_with("git@") || Path::new(s).exists()
}

async fn find(query: Option<String>, owner: Option<String>, project: bool) -> Result<()> {
    let tty = is_tty();
    let query = match query {
        Some(q) => q,
        None if tty => match or_cancel(input("Search skills").interact::<String>())? {
            Some(q) => q.trim().to_string(),
            None => return cancel(),
        },
        None => bail!("a search query is required"),
    };

    let repos = github_search(&query, owner.as_deref()).await?;
    if crate::json_mode() {
        emit_json(serde_json::json!({
            "query": query,
            "repos": repos.iter().map(|r| serde_json::json!({
                "full_name": r.full_name,
                "description": r.description,
            })).collect::<Vec<_>>(),
        }));
        return Ok(());
    }
    if repos.is_empty() {
        println!("no repositories matched {query:?}");
        return Ok(());
    }

    if !tty {
        for repo in &repos {
            println!("  {}  {}", repo.full_name, first_line(&repo.description));
        }
        return Ok(());
    }

    let mut menu = select::<usize>("Pick a repository").max_rows(12);
    for (i, repo) in repos.iter().enumerate() {
        let desc = console::truncate_str(first_line(&repo.description), 70, "…");
        menu = menu.item(i, &repo.full_name, desc);
    }
    let Some(idx) = or_cancel(menu.interact())? else {
        return cancel();
    };

    add(AddOpts {
        source: Some(repos[idx].full_name.clone()),
        project,
        skill: Vec::new(),
        all: false,
        yes: false,
        list: false,
        full_depth: false,
        force: false,
    })
}

#[derive(Deserialize)]
struct SearchResponse {
    items: Vec<RepoItem>,
}

#[derive(Deserialize)]
struct RepoItem {
    full_name: String,
    #[serde(default)]
    description: Option<String>,
}

struct Repo {
    full_name: String,
    description: String,
}

async fn github_search(query: &str, owner: Option<&str>) -> Result<Vec<Repo>> {
    let mut q = format!("{query} skill");
    if let Some(owner) = owner {
        q.push_str(&format!(" user:{owner}"));
    }
    let resp = reqwest::Client::new()
        .get("https://api.github.com/search/repositories")
        .query(&[("q", q.as_str()), ("per_page", "20")])
        .header("User-Agent", "aster")
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .context("querying the GitHub search API")?
        .error_for_status()
        .context("GitHub search failed (rate limited?)")?;
    let body: SearchResponse = resp.json().await.context("parsing GitHub search results")?;
    Ok(body
        .items
        .into_iter()
        .map(|i| Repo {
            full_name: i.full_name,
            description: i.description.unwrap_or_default(),
        })
        .collect())
}

fn update(names: Vec<String>, project: bool) -> Result<()> {
    let scope = scope_of(project);
    let root = scope_root(scope)?;
    let lock = load_lock(&root);
    if lock.skills.is_empty() {
        if crate::json_mode() {
            emit_json(serde_json::json!({
                "ok": true,
                "scope": scope_word(scope),
                "updated": 0,
            }));
        } else {
            println!("no tracked {} skills to update", scope_word(scope));
        }
        return Ok(());
    }

    // Group the wanted skills by their source so each source is fetched once.
    let mut by_source: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (name, entry) in &lock.skills {
        if names.is_empty() || names.contains(name) {
            by_source
                .entry(entry.source.clone())
                .or_default()
                .push(name.clone());
        }
    }
    if by_source.is_empty() {
        bail!("none of the named skills are tracked in this scope");
    }

    let mut updated = 0;
    for (source, wanted) in by_source {
        let src = match resolve_source(&source) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("skipping {source}: {e:#}");
                continue;
            }
        };
        let mut skills = src.list(false);
        src.filter(&mut skills);
        let chosen: Vec<Skill> = skills
            .into_iter()
            .filter(|s| wanted.contains(&s.name))
            .collect();
        if chosen.is_empty() {
            eprintln!("skipping {source}: its skills are no longer present");
            continue;
        }
        src.materialize(&chosen)?;
        updated += install_all(&chosen, &root, true)?;
    }
    if crate::json_mode() {
        emit_json(serde_json::json!({
            "ok": true,
            "scope": scope_word(scope),
            "root": root.display().to_string(),
            "updated": updated,
        }));
    } else {
        println!("updated {updated} skill(s)");
    }
    Ok(())
}

fn init(name: Option<&str>) -> Result<()> {
    let (dir, ident) = match name {
        Some(name) => (std::env::current_dir()?.join(name), name.to_string()),
        None => {
            let cwd = std::env::current_dir()?;
            let ident = cwd
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "skill".to_string());
            (cwd, ident)
        }
    };
    let manifest = dir.join("SKILL.md");
    if manifest.exists() {
        bail!("{} already exists", manifest.display());
    }
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    std::fs::write(&manifest, skill_template(&ident))
        .with_context(|| format!("writing {}", manifest.display()))?;
    if crate::json_mode() {
        emit_json(serde_json::json!({
            "ok": true,
            "name": ident,
            "path": manifest.display().to_string(),
        }));
    } else {
        println!("created {}", manifest.display());
    }
    Ok(())
}

fn skill_template(name: &str) -> String {
    format!(
        "---\n\
        name: {name}\n\
        description: What this skill does and when to use it. Be specific so the agent knows when to reach for it.\n\
        ---\n\n\
        # {name}\n\n\
        ## Instructions\n\n\
        Step-by-step guidance for the agent to follow.\n\n\
        ## Examples\n\n\
        Concrete examples of using this skill.\n"
    )
}

const LOCK_FILE: &str = "skills-lock.json";

#[derive(Default, Serialize, Deserialize)]
struct Lock {
    skills: BTreeMap<String, LockEntry>,
}

#[derive(Serialize, Deserialize)]
struct LockEntry {
    source: String,
}

fn load_lock(root: &Path) -> Lock {
    std::fs::read_to_string(root.join(LOCK_FILE))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_lock(root: &Path, lock: &Lock) {
    if let Err(e) = std::fs::create_dir_all(root)
        .and_then(|_| serde_json::to_string_pretty(lock).map_err(io::Error::other))
        .and_then(|json| std::fs::write(root.join(LOCK_FILE), json))
    {
        tracing::warn!("could not write {LOCK_FILE}: {e:#}");
    }
}

fn record_installed(root: &Path, source: &str, skills: &[Skill]) {
    let mut lock = load_lock(root);
    for skill in skills {
        lock.skills.insert(
            skill.name.clone(),
            LockEntry {
                source: source.to_string(),
            },
        );
    }
    save_lock(root, &lock);
}

fn forget_installed(root: &Path, names: &[String]) {
    let mut lock = load_lock(root);
    for name in names {
        lock.skills.remove(name);
    }
    save_lock(root, &lock);
}

fn prompt_source() -> Result<Option<String>> {
    let others = importable_agents();
    let mut menu = select::<u8>("Where should skills come from?")
        .item(0, "GitHub repo", "owner/repo, e.g. anthropics/skills")
        .item(1, "Git URL", "https://… or git@…")
        .item(2, "Local folder", "a path on this machine");
    if !others.is_empty() {
        menu = menu.item(
            3,
            "Another agent",
            format!("import from {} installed agent(s)", others.len()),
        );
    }
    let kind = match or_cancel(menu.interact())? {
        Some(k) => k,
        None => return Ok(None),
    };

    let source = match kind {
        3 => match prompt_agent(&others)? {
            Some(key) => key,
            None => return Ok(None),
        },
        _ => {
            let prompt = match kind {
                0 => "GitHub repo (owner/repo)",
                1 => "Git URL",
                _ => "Local folder path",
            };
            match or_cancel(input(prompt).interact::<String>())? {
                Some(s) => s.trim().to_string(),
                None => return Ok(None),
            }
        }
    };
    Ok(Some(source))
}

/// Pick which other agent to import from, listing where its skills live.
fn prompt_agent(others: &[&'static Agent]) -> Result<Option<String>> {
    let cwd = std::env::current_dir().ok();
    let mut menu = select::<usize>("Import from which agent?");
    for (i, agent) in others.iter().enumerate() {
        let roots = agent.existing_roots(cwd.as_deref());
        let detail = roots
            .iter()
            .map(|r| display_home(r))
            .collect::<Vec<_>>()
            .join(", ");
        menu = menu.item(i, agent.display_name, detail);
    }
    Ok(or_cancel(menu.interact())?.map(|i| others[i].key.to_string()))
}

/// Paths read better in a picker with the home directory back as `~`.
fn display_home(path: &Path) -> String {
    let shown = path.display().to_string();
    match dirs::home_dir() {
        Some(home) => match path.strip_prefix(&home) {
            Ok(rel) => format!("~/{}", rel.display()),
            Err(_) => shown,
        },
        None => shown,
    }
}

/// The shared multi-select over the source's skills, none preselected.
fn choose_skills(title: &str, skills: &[Skill]) -> Result<Option<Vec<Skill>>> {
    let items: Vec<crate::picker::Item> = skills
        .iter()
        .map(|s| crate::picker::Item {
            name: s.name.clone(),
            detail: first_line(&s.description).to_string(),
        })
        .collect();
    Ok(crate::picker::multi_select(title, &items, false)?
        .map(|chosen| chosen.into_iter().map(|i| skills[i].clone()).collect()))
}

/// A git source is a treeless partial clone with only `SKILL.md` files checked
/// out, so listing is cheap; a chosen skill's contents are fetched on demand.
enum Source {
    /// One or more local roots; an agent import can have both a global and a
    /// project root.
    Local(Vec<PathBuf>),
    Git {
        /// Kept alive so the clone survives until installation finishes.
        _guard: tempfile::TempDir,
        root: PathBuf,
        subpath: Option<String>,
    },
}

impl Source {
    fn roots(&self) -> Vec<&Path> {
        match self {
            Source::Local(paths) => paths.iter().map(PathBuf::as_path).collect(),
            Source::Git { root, .. } => vec![root.as_path()],
        }
    }

    /// The skills this source offers, deduped by name with earlier roots winning.
    fn list(&self, full_depth: bool) -> Vec<Skill> {
        self.list_report(full_depth).0
    }

    /// Like [`Source::list`], but also returns the manifests that failed to parse.
    fn list_report(&self, full_depth: bool) -> (Vec<Skill>, Vec<String>) {
        let mut skills: Vec<Skill> = Vec::new();
        let mut skipped = Vec::new();
        for root in self.roots() {
            let (found, reasons) = find_skills_report(root, full_depth);
            for skill in found {
                if !skills.iter().any(|s| s.name == skill.name) {
                    skills.push(skill);
                }
            }
            skipped.extend(reasons);
        }
        (skills, skipped)
    }

    /// When a repo subpath was given, keep only skills beneath it.
    fn filter(&self, skills: &mut Vec<Skill>) {
        if let Source::Git {
            root,
            subpath: Some(sub),
            ..
        } = self
        {
            let prefix = root.join(sub);
            skills.retain(|s| s.path.starts_with(&prefix));
        }
    }

    /// A git source expands its sparse checkout to the chosen directories, which
    /// lazily downloads only those blobs; local sources already have everything.
    fn materialize(&self, chosen: &[Skill]) -> Result<()> {
        let Source::Git { root, .. } = self else {
            return Ok(());
        };
        let mut patterns = vec![MANIFEST_PATTERN.to_string()];
        for skill in chosen {
            if let Some(dir) = skill.path.parent()
                && let Ok(rel) = dir.strip_prefix(root)
            {
                patterns.push(format!("/{}/**", rel.to_string_lossy()));
            }
        }
        sparse_set(root, &patterns)
    }
}

const MANIFEST_PATTERN: &str = "**/SKILL.md";

/// A local path is used as-is and an agent key expands to that agent's skills
/// roots; a `owner/repo` shorthand or git URL becomes a treeless partial clone.
/// A trailing `/subpath` narrows browsing to that subtree.
fn resolve_source(source: &str) -> Result<Source> {
    let source = source.trim();

    let local = Path::new(source);
    if local.exists() {
        return Ok(Source::Local(vec![local.to_path_buf()]));
    }

    if let Some(roots) = agent_roots(source)? {
        return Ok(Source::Local(roots));
    }

    let Some((url, subpath)) = git_source(source) else {
        bail!("could not resolve {source:?} as a local path, an agent, or a git source");
    };

    let tmp = tempfile::tempdir().context("creating a temp dir for the checkout")?;
    partial_clone(&url, tmp.path())?;
    sparse_set(tmp.path(), &[MANIFEST_PATTERN.to_string()])?;
    Ok(Source::Git {
        root: tmp.path().to_path_buf(),
        subpath,
        _guard: tmp,
    })
}

/// Recognises full git URLs and `owner/repo[/subpath]` GitHub shorthands.
fn git_source(source: &str) -> Option<(String, Option<String>)> {
    if source.starts_with("http://")
        || source.starts_with("https://")
        || source.starts_with("git@")
        || source.ends_with(".git")
    {
        return Some((source.to_string(), None));
    }

    let mut parts = source.splitn(3, '/');
    let owner = parts.next().filter(|s| is_ghslug(s))?;
    let repo = parts.next().filter(|s| is_ghslug(s))?;
    let subpath = parts.next().map(str::to_string);
    Some((format!("https://github.com/{owner}/{repo}"), subpath))
}

fn is_ghslug(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
}

/// Clone without blobs so only the tree is fetched; blobs arrive on demand. Falls
/// back to a shallow clone for remotes that do not support partial clone.
fn partial_clone(url: &str, into: &Path) -> Result<()> {
    let dest = into.to_str().context("clone path is not valid UTF-8")?;
    let filtered = git_try(&[
        "clone",
        "--quiet",
        "--filter=blob:none",
        "--sparse",
        "--depth",
        "1",
        url,
        dest,
    ])?;
    if filtered {
        return Ok(());
    }
    tracing::debug!("partial clone unsupported; falling back to a shallow clone");
    git(&["clone", "--quiet", "--sparse", "--depth", "1", url, dest])
        .with_context(|| format!("git clone failed for {url}"))
}

/// Point the sparse checkout at `patterns`; in a partial clone this fetches
/// exactly the blobs the new patterns need.
fn sparse_set(dir: &Path, patterns: &[String]) -> Result<()> {
    let d = dir.to_str().context("checkout path is not valid UTF-8")?;
    let mut args = vec!["-C", d, "sparse-checkout", "set", "--no-cone"];
    args.extend(patterns.iter().map(String::as_str));
    git(&args)
}

/// Output captured so nothing corrupts an active spinner.
fn git(args: &[&str]) -> Result<()> {
    if !git_try(args)? {
        bail!("git {} failed", args.join(" "));
    }
    Ok(())
}

/// Returns `false` on a non-zero exit rather than erroring.
fn git_try(args: &[&str]) -> Result<bool> {
    let output = Command::new("git")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .context("running git (is it installed?)")?;
    if !output.status.success() {
        tracing::debug!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
        return Ok(false);
    }
    Ok(true)
}

/// Resolve another agent's `--agent` key to the skills roots it has on disk.
/// An unknown key is not an agent, so the caller falls through to git.
fn agent_roots(source: &str) -> Result<Option<Vec<PathBuf>>> {
    let Some(agent) = agent_by_key(source) else {
        return Ok(None);
    };
    let roots = agent.existing_roots(std::env::current_dir().ok().as_deref());
    if roots.is_empty() {
        bail!(
            "{} has no skills directory on this machine",
            agent.display_name
        );
    }
    Ok(Some(roots))
}

/// The agents with skills on this machine, for the wizard's import list.
fn importable_agents() -> Vec<&'static Agent> {
    let cwd = std::env::current_dir().ok();
    installed_agents(cwd.as_deref())
        .into_iter()
        .filter(|a| a.key != "aster")
        .collect()
}

fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or(text)
}

fn is_tty() -> bool {
    !crate::json_mode() && io::stdout().is_terminal() && io::stdin().is_terminal()
}

/// One JSON object on stdout, the shape every `--json` skills command returns.
fn emit_json(value: serde_json::Value) {
    println!("{value}");
}

fn skill_values(skills: &[Skill]) -> Vec<serde_json::Value> {
    skills
        .iter()
        .map(|s| serde_json::json!({ "name": s.name, "description": s.description }))
        .collect()
}

/// Terminal width for laying out plain output, with a sane fallback when piped.
fn width() -> usize {
    let w = Term::stdout().size().1 as usize;
    if w == 0 { 100 } else { w }
}

fn cancel() -> Result<()> {
    outro_cancel("Cancelled")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn github_shorthand_becomes_clone_url() {
        assert_eq!(
            git_source("anthropics/skills"),
            Some(("https://github.com/anthropics/skills".into(), None))
        );
    }

    #[test]
    fn shorthand_carries_subpath() {
        assert_eq!(
            git_source("anthropics/skills/document/pdf"),
            Some((
                "https://github.com/anthropics/skills".into(),
                Some("document/pdf".into())
            ))
        );
    }

    #[test]
    fn full_urls_pass_through() {
        assert_eq!(
            git_source("https://github.com/a/b.git"),
            Some(("https://github.com/a/b.git".into(), None))
        );
        assert_eq!(
            git_source("git@github.com:a/b.git"),
            Some(("git@github.com:a/b.git".into(), None))
        );
    }

    #[test]
    fn plain_words_are_not_git_sources() {
        assert_eq!(git_source("just-a-name"), None);
    }

    #[test]
    fn source_detection() {
        assert!(looks_like_source("owner/repo"));
        assert!(looks_like_source("https://github.com/a/b"));
        assert!(!looks_like_source("pdf"));
    }

    #[test]
    fn template_has_valid_frontmatter() {
        let t = skill_template("my-skill");
        assert!(t.starts_with("---\nname: my-skill\n"));
        assert!(t.contains("description:"));
    }

    /// The agent loads both roots every run, so installing once globally is what
    /// makes a skill available everywhere. Project scope is the opt-in.
    #[test]
    fn scope_defaults_to_global() {
        assert!(matches!(scope_of(false), Scope::Global));
        assert!(matches!(scope_of(true), Scope::Project));
    }

    #[test]
    fn global_and_project_roots_are_distinct() {
        let global = scope_root(Scope::Global).unwrap();
        let project = scope_root(Scope::Project).unwrap();
        assert_ne!(global, project);
        assert!(project.ends_with(".aster/skills"), "{}", project.display());
        assert!(global.ends_with("skills"), "{}", global.display());
    }

    #[test]
    fn the_other_scope_is_named_for_error_messages() {
        assert!(matches!(other_scope(Scope::Global), Scope::Project));
        assert!(matches!(other_scope(Scope::Project), Scope::Global));
        assert_eq!(other_scope_flag(Scope::Global), "--project");
        assert_eq!(other_scope_flag(Scope::Project), "--global");
    }
}
