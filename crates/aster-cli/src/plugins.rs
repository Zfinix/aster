//! `aster plugins`: install and inspect Agent Plugins packages. A plugin is a
//! directory with a `plugin.json`; its skills join the session's skill index and
//! its MCP servers join the configured ones under `<plugin>/<server>`.
//! Plugins install user-global (`<config>/aster/plugins`) by default, or into
//! this project (`.aster/plugins`) with `-p`, where they shadow a global plugin
//! of the same name.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use aster_plugins::{Plugin, Transport};
use clap::{Args, Subcommand};
use cliclack::{confirm, input, intro, outro, outro_cancel};
use console::style;

use crate::picker::{Item, first_line, is_tty, multi_select};
use crate::util::or_cancel;

#[derive(Args)]
pub struct PluginsArgs {
    #[command(subcommand)]
    command: Option<PluginsCommand>,
}

#[derive(Subcommand)]
enum PluginsCommand {
    /// Install plugins from a repo or a local folder.
    #[command(visible_alias = "a")]
    Add {
        /// Source: `owner/repo`, a git URL, or a local path. Omit for a prompt.
        source: Option<String>,
        /// Install into this project (`.aster/plugins`) instead of the user-global root.
        #[arg(short = 'p', long)]
        project: bool,
        /// Install into the user-global root. The default; accepted for symmetry.
        #[arg(short = 'g', long, conflicts_with = "project")]
        global: bool,
        /// Install only these plugins by name (repeat or comma-separate).
        #[arg(long = "plugin", value_delimiter = ',')]
        plugin: Vec<String>,
        /// Install every plugin the source offers, no prompts.
        #[arg(long)]
        all: bool,
        /// Skip the confirmation prompt.
        #[arg(short = 'y', long)]
        yes: bool,
        /// List what a source offers without installing.
        #[arg(short = 'l', long)]
        list: bool,
        /// Overwrite plugins that are already installed.
        #[arg(long)]
        force: bool,
    },
    /// List installed plugins and what they contribute.
    #[command(visible_alias = "ls")]
    List {
        /// List only this project's plugins.
        #[arg(short = 'p', long)]
        project: bool,
        /// List only user-global plugins.
        #[arg(short = 'g', long, conflicts_with = "project")]
        global: bool,
    },
    /// Remove installed plugins.
    #[command(visible_alias = "rm")]
    Remove {
        /// Plugin names to remove.
        plugins: Vec<String>,
        /// Remove from this project instead of the user-global root.
        #[arg(short = 'p', long)]
        project: bool,
        /// Remove from the user-global root. The default; accepted for symmetry.
        #[arg(short = 'g', long, conflicts_with = "project")]
        global: bool,
        /// Also delete the plugin's data directory, which is otherwise kept.
        #[arg(long)]
        purge: bool,
        /// Skip the confirmation prompt.
        #[arg(short = 'y', long)]
        yes: bool,
    },
    /// Check a plugin directory against the Agent Plugins specification.
    Validate {
        /// The plugin root; defaults to the current directory.
        path: Option<PathBuf>,
    },
}

pub fn run(args: PluginsArgs, repo_root: Option<&Path>) -> Result<()> {
    let command = args.command.unwrap_or(PluginsCommand::List {
        project: false,
        global: false,
    });
    match command {
        PluginsCommand::Add {
            source,
            project,
            global: _,
            plugin,
            all,
            yes,
            list,
            force,
        } => add(
            repo_root,
            AddOpts {
                source,
                project,
                plugin,
                all,
                yes,
                list,
                force,
            },
        ),
        PluginsCommand::List { project, global } => list(repo_root, project, global),
        PluginsCommand::Remove {
            plugins,
            project,
            global: _,
            purge,
            yes,
        } => remove(repo_root, plugins, project, purge, yes),
        PluginsCommand::Validate { path } => validate(path),
    }
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Scope {
    Project,
    Global,
}

fn scope_word(scope: Scope) -> &'static str {
    match scope {
        Scope::Project => "project",
        Scope::Global => "global",
    }
}

/// Where a scope keeps installed packages and their persistent data. The data
/// root sits beside the packages so an update replaces one and not the other.
fn scope_roots(scope: Scope, repo_root: Option<&Path>) -> Result<(PathBuf, PathBuf)> {
    let base = match scope {
        Scope::Global => crate::persist::home()?,
        Scope::Project => match repo_root {
            Some(root) => root.join(".aster"),
            None => std::env::current_dir()
                .context("could not determine the current directory")?
                .join(".aster"),
        },
    };
    Ok((base.join("plugins"), base.join("plugin-data")))
}

/// A plugin Aster carries inside its own binary. It is written into the global
/// plugin root before discovery, so from then on it is an ordinary installed
/// plugin: listed, disableable, and removable.
struct Builtin {
    name: &'static str,
    manifest: &'static str,
    mcp: &'static str,
}

const BUILTINS: &[Builtin] = &[];

/// Bundled plugins Aster no longer ships. Their directories are removed on
/// startup: discovery would otherwise keep finding one and spawning a server
/// whose tools now live in the binary.
const RETIRED: &[&str] = &["websearch"];

/// Left in a builtin's data directory when the user removes it, so that a later
/// session does not helpfully put it back.
const UNINSTALLED: &str = ".uninstalled";

impl Builtin {
    fn install(&self, root: &Path, data_root: &Path) -> Result<()> {
        if data_root.join(self.name).join(UNINSTALLED).exists() {
            return Ok(());
        }
        let dir = root.join(self.name);
        std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
        write_if_changed(&dir.join(aster_plugins::MANIFEST_FILE), self.manifest)?;
        write_if_changed(&dir.join(aster_plugins::MCP_FILE), self.mcp)
    }
}

fn write_if_changed(path: &Path, contents: &str) -> Result<()> {
    if std::fs::read_to_string(path).is_ok_and(|existing| existing == contents) {
        return Ok(());
    }
    std::fs::write(path, contents).with_context(|| format!("writing {}", path.display()))
}

/// Materialize the bundled plugins and clear out the retired ones. A failure
/// here is logged, never fatal: the session still works, it just has fewer
/// tools.
fn install_builtins() {
    let Ok((root, data_root)) = scope_roots(Scope::Global, None) else {
        return;
    };
    for builtin in BUILTINS {
        if let Err(e) = builtin.install(&root, &data_root) {
            tracing::debug!(plugin = builtin.name, "bundled plugin not installed: {e:#}");
        }
    }
    remove_retired(&root);
}

/// Delete the directories of plugins Aster used to bundle. Only a directory
/// Aster wrote itself is removed, so a package the user installed under the
/// same name survives.
fn remove_retired(root: &Path) {
    for name in RETIRED {
        let dir = root.join(name);
        if !dir.join(aster_plugins::MCP_FILE).exists() {
            continue;
        }
        if let Err(e) = std::fs::remove_dir_all(&dir) {
            tracing::debug!(plugin = name, "retired plugin not removed: {e}");
        }
    }
}

/// Record that a bundled plugin was removed on purpose. Written even under
/// `--purge`, since the alternative is the plugin reappearing next session.
fn mark_uninstalled(name: &str, data_root: &Path) {
    if !BUILTINS.iter().any(|b| b.name == name) {
        return;
    }
    let dir = data_root.join(name);
    let marked =
        std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(dir.join(UNINSTALLED), ""));
    if let Err(e) = marked {
        tracing::debug!(plugin = name, "could not record the removal: {e}");
    }
}

/// Every installed plugin, this project's shadowing a global one of the same
/// name. Problems are returned rather than printed so each caller decides.
pub(crate) fn installed(repo_root: Option<&Path>) -> (Vec<Plugin>, Vec<String>) {
    install_builtins();
    let mut plugins: Vec<Plugin> = Vec::new();
    let mut problems = Vec::new();
    for scope in [Scope::Project, Scope::Global] {
        let Ok((root, data_root)) = scope_roots(scope, repo_root) else {
            continue;
        };
        let (found, issues) = aster_plugins::discover(&root, &data_root);
        problems.extend(issues);
        for plugin in found {
            match plugins.iter().any(|p| p.name() == plugin.name()) {
                true => tracing::debug!(
                    plugin = plugin.name(),
                    "shadowed by a higher-precedence plugin of the same name"
                ),
                false => plugins.push(plugin),
            }
        }
    }
    (plugins, problems)
}

/// The skill directories installed plugins contribute, in plugin-name order.
pub(crate) fn skill_dirs(plugins: &[Plugin]) -> Vec<PathBuf> {
    plugins
        .iter()
        .flat_map(|plugin| plugin.skills.iter().cloned())
        .collect()
}

/// Plugin servers as runtime configuration, keyed `<plugin>/<server>` so two
/// plugins can ship a server of the same name.
pub(crate) fn mcp_servers(plugins: &[Plugin]) -> Vec<(String, crate::mcp::ServerConfig)> {
    let mut out = Vec::new();
    for plugin in plugins {
        for server in &plugin.servers {
            let id = format!("{}/{}", plugin.name(), server.name);
            let config = match &server.transport {
                Transport::Stdio(stdio) => {
                    // The subprocess needs its data directory before it starts.
                    if let Err(e) = std::fs::create_dir_all(&plugin.data_dir) {
                        tracing::warn!(server = %id, "skipping MCP server: could not create {} ({e})", plugin.data_dir.display());
                        continue;
                    }
                    crate::mcp::ServerConfig {
                        command: stdio.command.clone(),
                        args: stdio.args.clone(),
                        env: stdio.env.clone(),
                        cwd: Some(stdio.cwd.clone()),
                        kind: Some(crate::mcp::Transport::Stdio),
                        ..Default::default()
                    }
                }
                Transport::Http(http) => crate::mcp::ServerConfig {
                    url: http.url.clone(),
                    headers: http.headers.clone(),
                    kind: Some(match http.streamable {
                        true => crate::mcp::Transport::StreamableHttp,
                        false => crate::mcp::Transport::Sse,
                    }),
                    ..Default::default()
                },
            };
            out.push((id, config));
        }
    }
    out
}

/// Log what loading found, once per process, so a broken package is visible
/// without every caller repeating the plumbing.
pub(crate) fn report(plugins: &[Plugin], problems: &[String]) {
    for problem in problems {
        tracing::warn!("skipping plugin {problem}");
    }
    for plugin in plugins {
        for warning in &plugin.warnings {
            tracing::warn!(plugin = plugin.name(), "{warning}");
        }
    }
}

struct AddOpts {
    source: Option<String>,
    project: bool,
    plugin: Vec<String>,
    all: bool,
    yes: bool,
    list: bool,
    force: bool,
}

fn add(repo_root: Option<&Path>, opts: AddOpts) -> Result<()> {
    let scope = match opts.project {
        true => Scope::Project,
        false => Scope::Global,
    };
    let (dest, data_root) = scope_roots(scope, repo_root)?;
    let tty = is_tty();

    let source = match opts.source.clone() {
        Some(source) => source,
        None if tty => {
            intro("Add plugins")?;
            match or_cancel(
                input("Source (owner/repo, a git URL, or a path)").interact::<String>(),
            )? {
                Some(source) => source.trim().to_string(),
                None => return cancel(),
            }
        }
        None => bail!("a source is required (owner/repo, a git URL, or a local path)"),
    };

    let checkout = crate::skills::checkout(&source)?;
    let roots = aster_plugins::candidates(checkout.path());
    if roots.is_empty() {
        let message = format!("no plugin.json found at {source}");
        if tty {
            outro_cancel(message)?;
            return Ok(());
        }
        bail!("{message}");
    }

    let mut offered = Vec::new();
    for root in &roots {
        match aster_plugins::load(root, &data_root) {
            Ok(plugin) => offered.push(plugin),
            Err(e) => eprintln!("skipping {}: {e:#}", root.display()),
        }
    }
    if offered.is_empty() {
        bail!("no valid plugin at {source}");
    }

    if opts.list {
        if crate::json_mode() {
            emit(serde_json::json!({ "source": source, "plugins": values(&offered) }));
        } else {
            print_plugins(&offered);
        }
        return Ok(());
    }

    let chosen = match select(&offered, &opts, tty)? {
        Some(chosen) if !chosen.is_empty() => chosen,
        _ => return cancel(),
    };

    if tty && !opts.yes {
        let ok = or_cancel(
            confirm(format!(
                "Install {} plugin(s) into the {} scope?",
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

    let mut installed = Vec::new();
    for plugin in &chosen {
        let target = dest.join(plugin.name());
        if target.exists() {
            if !opts.force {
                eprintln!(
                    "{} is already installed (--force to replace)",
                    plugin.name()
                );
                continue;
            }
            std::fs::remove_dir_all(&target)
                .with_context(|| format!("removing {}", target.display()))?;
        }
        copy_dir(&plugin.root, &target)?;
        installed.push(plugin.name().to_string());
        if !crate::json_mode() {
            println!(
                "installed {} ({} skill(s), {} MCP server(s))",
                plugin.name(),
                plugin.skills.len(),
                plugin.servers.len()
            );
        }
    }

    if crate::json_mode() {
        emit(serde_json::json!({
            "ok": true,
            "source": source,
            "scope": scope_word(scope),
            "root": dest.display().to_string(),
            "installed": installed,
        }));
        return Ok(());
    }
    let done = format!(
        "Installed {} plugin(s) into {}",
        installed.len(),
        dest.display()
    );
    if tty {
        outro(done)?;
    } else {
        println!("{done}");
    }
    Ok(())
}

fn select<'a>(offered: &'a [Plugin], opts: &AddOpts, tty: bool) -> Result<Option<Vec<&'a Plugin>>> {
    if opts.all {
        return Ok(Some(offered.iter().collect()));
    }
    if !opts.plugin.is_empty() {
        let mut chosen = Vec::new();
        for name in &opts.plugin {
            let plugin = offered
                .iter()
                .find(|p| p.name() == name)
                .with_context(|| format!("no plugin named {name:?} at that source"))?;
            chosen.push(plugin);
        }
        return Ok(Some(chosen));
    }
    if offered.len() == 1 {
        return Ok(Some(offered.iter().collect()));
    }
    if !tty {
        bail!("specify --plugin <names> or --all when not attached to a terminal");
    }
    let items: Vec<Item> = offered
        .iter()
        .map(|plugin| Item {
            name: plugin.name().to_string(),
            detail: summary(plugin),
        })
        .collect();
    Ok(multi_select("Select plugins to install", &items, false)?
        .map(|chosen| chosen.into_iter().map(|i| &offered[i]).collect()))
}

fn list(repo_root: Option<&Path>, project_only: bool, global_only: bool) -> Result<()> {
    let scopes: &[Scope] = match (project_only, global_only) {
        (true, _) => &[Scope::Project],
        (_, true) => &[Scope::Global],
        _ => &[Scope::Project, Scope::Global],
    };
    install_builtins();

    let mut sections = Vec::new();
    for &scope in scopes {
        let (root, data_root) = scope_roots(scope, repo_root)?;
        let (plugins, problems) = aster_plugins::discover(&root, &data_root);
        sections.push((scope, root, plugins, problems));
    }

    if crate::json_mode() {
        emit(serde_json::json!({
            "scopes": sections
                .iter()
                .map(|(scope, root, plugins, problems)| serde_json::json!({
                    "scope": scope_word(*scope),
                    "root": root.display().to_string(),
                    "plugins": values(plugins),
                    "problems": problems,
                }))
                .collect::<Vec<_>>(),
        }));
        return Ok(());
    }

    let total: usize = sections.iter().map(|(_, _, p, _)| p.len()).sum();
    if total == 0 {
        for (scope, root, _, _) in &sections {
            println!("no {} plugins in {}", scope_word(*scope), root.display());
        }
        println!("install some with: aster plugins add <owner/repo>");
        return Ok(());
    }

    for (scope, root, plugins, problems) in &sections {
        if plugins.is_empty() {
            println!("no {} plugins in {}\n", scope_word(*scope), root.display());
            continue;
        }
        println!("{} {} plugin(s):\n", plugins.len(), scope_word(*scope));
        print_plugins(plugins);
        for problem in problems {
            eprintln!("  {} {problem}", style("skipped").yellow());
        }
        println!();
    }
    Ok(())
}

fn remove(
    repo_root: Option<&Path>,
    names: Vec<String>,
    project: bool,
    purge: bool,
    yes: bool,
) -> Result<()> {
    let scope = match project {
        true => Scope::Project,
        false => Scope::Global,
    };
    let (root, data_root) = scope_roots(scope, repo_root)?;
    let tty = is_tty();

    let (plugins, _) = aster_plugins::discover(&root, &data_root);
    let targets: Vec<String> = if !names.is_empty() {
        names
    } else if tty {
        if plugins.is_empty() {
            println!("no {} plugins to remove", scope_word(scope));
            return Ok(());
        }
        let items: Vec<Item> = plugins
            .iter()
            .map(|plugin| Item {
                name: plugin.name().to_string(),
                detail: summary(plugin),
            })
            .collect();
        match multi_select("Select plugins to remove", &items, false)? {
            Some(chosen) => chosen
                .into_iter()
                .map(|i| plugins[i].name().to_string())
                .collect(),
            None => return cancel(),
        }
    } else {
        bail!("specify plugin names");
    };

    if tty && !yes {
        let ok = or_cancel(
            confirm(format!("Remove {} plugin(s)?", targets.len()))
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
        let dir = root.join(name);
        if !dir.is_dir() {
            missing.push(name.clone());
            if !json {
                eprintln!("no {} plugin named {name:?}", scope_word(scope));
            }
            continue;
        }
        std::fs::remove_dir_all(&dir).with_context(|| format!("removing {}", dir.display()))?;
        if purge {
            let data = data_root.join(name);
            if data.exists() {
                std::fs::remove_dir_all(&data)
                    .with_context(|| format!("removing {}", data.display()))?;
            }
        }
        mark_uninstalled(name, &data_root);
        removed.push(name.clone());
        if !json {
            println!("removed {name}");
        }
    }

    if json {
        emit(serde_json::json!({
            "ok": true,
            "scope": scope_word(scope),
            "root": root.display().to_string(),
            "removed": removed,
            "missing": missing,
            "purged": purge,
        }));
    } else if !purge && !removed.is_empty() {
        println!(
            "kept plugin data in {} (--purge to delete it)",
            data_root.display()
        );
    }
    Ok(())
}

/// Author-facing conformance check: the same loader the session uses, with
/// everything it reported printed instead of logged.
fn validate(path: Option<PathBuf>) -> Result<()> {
    let root = match path {
        Some(path) => path,
        None => std::env::current_dir().context("could not determine the current directory")?,
    };
    let data_root = std::env::temp_dir().join("aster-plugin-validate");
    let plugin = match aster_plugins::load(&root, &data_root) {
        Ok(plugin) => plugin,
        Err(e) => {
            if crate::json_mode() {
                emit(serde_json::json!({ "ok": false, "error": format!("{e:#}") }));
                std::process::exit(1);
            }
            return Err(e);
        }
    };

    if crate::json_mode() {
        emit(serde_json::json!({
            "ok": plugin.warnings.is_empty(),
            "spec_version": aster_plugins::SPEC_VERSION,
            "plugin": values(std::slice::from_ref(&plugin))[0],
            "warnings": plugin.warnings,
        }));
        return Ok(());
    }

    println!(
        "{} conforms to Agent Plugins {}",
        plugin.name(),
        aster_plugins::SPEC_VERSION
    );
    println!(
        "  skills:  {}",
        listed(
            plugin
                .skills
                .iter()
                .filter_map(|s| s.file_name())
                .map(|n| n.to_string_lossy().into_owned())
        )
    );
    println!(
        "  servers: {}",
        listed(
            plugin
                .servers
                .iter()
                .map(|s| format!("{} ({})", s.name, s.transport_name()))
        )
    );
    for warning in &plugin.warnings {
        println!("  {} {warning}", style("warning").yellow());
    }
    Ok(())
}

fn listed(items: impl Iterator<Item = String>) -> String {
    let items: Vec<String> = items.collect();
    match items.is_empty() {
        true => "none".to_string(),
        false => items.join(", "),
    }
}

fn print_plugins(plugins: &[Plugin]) {
    let width = plugins.iter().map(|p| p.name().len()).max().unwrap_or(0);
    for plugin in plugins {
        println!(
            "  {:<width$}  {}",
            plugin.name(),
            style(summary(plugin)).dim()
        );
        for warning in &plugin.warnings {
            println!("  {:<width$}  {}", "", style(warning).yellow());
        }
    }
}

/// One line for a picker or a listing: what the plugin contributes, plus its
/// description when it has one.
fn summary(plugin: &Plugin) -> String {
    let mut parts = vec![format!(
        "{} skill(s), {} server(s)",
        plugin.skills.len(),
        plugin.servers.len()
    )];
    if let Some(version) = &plugin.manifest.version {
        parts.push(format!("v{version}"));
    }
    let head = parts.join(", ");
    match &plugin.manifest.description {
        Some(description) => format!("{head} — {}", first_line(description)),
        None => head,
    }
}

fn values(plugins: &[Plugin]) -> Vec<serde_json::Value> {
    plugins
        .iter()
        .map(|plugin| {
            serde_json::json!({
                "name": plugin.name(),
                "version": plugin.manifest.version,
                "description": plugin.manifest.description,
                "root": plugin.root.display().to_string(),
                "data_dir": plugin.data_dir.display().to_string(),
                "skills": plugin.skills.iter()
                    .filter_map(|s| s.file_name())
                    .map(|n| n.to_string_lossy().into_owned())
                    .collect::<Vec<_>>(),
                "servers": plugin.servers.iter()
                    .map(|s| serde_json::json!({ "name": s.name, "transport": s.transport_name() }))
                    .collect::<Vec<_>>(),
                "warnings": plugin.warnings,
            })
        })
        .collect()
}

fn copy_dir(src: &Path, dest: &Path) -> Result<()> {
    std::fs::create_dir_all(dest).with_context(|| format!("creating {}", dest.display()))?;
    for entry in std::fs::read_dir(src).with_context(|| format!("reading {}", src.display()))? {
        let entry = entry?;
        let from = entry.path();
        let to = dest.join(entry.file_name());
        if from.is_dir() {
            if entry.file_name() == ".git" {
                continue;
            }
            copy_dir(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)
                .with_context(|| format!("copying {} to {}", from.display(), to.display()))?;
        }
    }
    Ok(())
}

fn emit(value: serde_json::Value) {
    println!("{value}");
}

fn cancel() -> Result<()> {
    outro_cancel("Cancelled")?;
    Ok(())
}

#[cfg(test)]
#[path = "tests/plugins_test.rs"]
mod tests;
