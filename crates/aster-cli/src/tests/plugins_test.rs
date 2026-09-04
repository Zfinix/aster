use std::fs;
use std::path::Path;

use super::*;

const MANIFEST: &str = r#"{
  "$schema": "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
  "name": "demo",
  "version": "1.2.0",
  "description": "A demo plugin"
}"#;

const MCP: &str = r#"{
  "$schema": "https://agent-plugins.org/schemas/1.0.0/mcp.schema.json",
  "mcpServers": {
    "local": {"type": "stdio", "command": "npx", "args": ["-y", "demo"]},
    "remote": {"type": "streamable-http", "url": "https://example.com/mcp"}
  }
}"#;

fn fixture(dir: &Path) -> Plugin {
    let root = dir.join("plugins/demo");
    fs::create_dir_all(root.join("skills/summarize")).expect("dirs");
    fs::write(root.join("plugin.json"), MANIFEST).expect("manifest");
    fs::write(root.join("mcp.json"), MCP).expect("mcp");
    fs::write(
        root.join("skills/summarize/SKILL.md"),
        "---\nname: summarize\ndescription: Summarize things.\n---\nbody\n",
    )
    .expect("skill");
    aster_plugins::load(&root, &dir.join("plugin-data")).expect("load")
}

#[test]
fn servers_are_namespaced_by_plugin() {
    let dir = tempfile::tempdir().expect("tempdir");
    let plugin = fixture(dir.path());
    let servers = mcp_servers(std::slice::from_ref(&plugin));
    assert_eq!(servers.len(), 2);

    let (name, local) = &servers[0];
    assert_eq!(name, "demo/local");
    assert_eq!(local.command, "npx");
    assert_eq!(local.cwd.as_deref(), Some(plugin.root.as_path()));
    assert_eq!(local.env["PLUGIN_ROOT"], plugin.root.display().to_string());
    assert_eq!(local.transport(), Some(crate::mcp::Transport::Stdio));
    assert!(!local.disabled);

    let (name, remote) = &servers[1];
    assert_eq!(name, "demo/remote");
    assert_eq!(remote.url, "https://example.com/mcp");
    assert_eq!(
        remote.transport(),
        Some(crate::mcp::Transport::StreamableHttp)
    );
}

#[test]
fn the_data_directory_exists_before_a_server_can_start() {
    let dir = tempfile::tempdir().expect("tempdir");
    let plugin = fixture(dir.path());
    assert!(!plugin.data_dir.exists());
    mcp_servers(std::slice::from_ref(&plugin));
    assert!(plugin.data_dir.is_dir());
}

#[test]
fn plugin_skills_load_into_a_skill_set() {
    let dir = tempfile::tempdir().expect("tempdir");
    let plugin = fixture(dir.path());
    let set =
        aster_skills::SkillSet::default().extend_dirs(&skill_dirs(std::slice::from_ref(&plugin)));
    assert_eq!(set.len(), 1);
    assert!(set.get("summarize").is_some());
}

#[test]
fn a_project_plugin_shadows_a_global_one_of_the_same_name() {
    let dir = tempfile::tempdir().expect("tempdir");
    let repo = dir.path();
    let root = repo.join(".aster/plugins/demo");
    fs::create_dir_all(&root).expect("dirs");
    fs::write(root.join("plugin.json"), MANIFEST).expect("manifest");
    let (plugins, problems) = installed(Some(repo));
    assert!(problems.is_empty(), "{problems:?}");
    let demos: Vec<&Plugin> = plugins.iter().filter(|p| p.name() == "demo").collect();
    assert_eq!(demos.len(), 1);
    // The loader canonicalizes, which on macOS resolves the temp dir's symlink.
    let project = fs::canonicalize(repo).expect("canonicalize").join(".aster");
    assert!(demos[0].root.starts_with(project), "{:?}", demos[0].root);
}

#[test]
fn summarizes_what_a_plugin_contributes() {
    let dir = tempfile::tempdir().expect("tempdir");
    let plugin = fixture(dir.path());
    let line = summary(&plugin);
    assert!(
        line.starts_with("1 skill(s), 2 server(s), v1.2.0"),
        "{line}"
    );
    assert!(line.ends_with("A demo plugin"), "{line}");
}

const BUNDLED: Builtin = Builtin {
    name: "demo",
    manifest: MANIFEST,
    mcp: MCP,
};

#[test]
fn a_bundled_package_installs_as_a_valid_plugin() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (root, data_root) = (dir.path().join("plugins"), dir.path().join("plugin-data"));
    BUNDLED.install(&root, &data_root).expect("install");

    let plugin = aster_plugins::load(&root.join("demo"), &data_root).expect("load");
    assert!(plugin.warnings.is_empty(), "{:?}", plugin.warnings);
    assert_eq!(plugin.name(), "demo");

    let servers: Vec<&str> = plugin.stdio_servers().map(|(name, _)| name).collect();
    assert_eq!(servers, ["local"]);
}

#[test]
fn installing_twice_leaves_the_files_untouched() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (root, data_root) = (dir.path().join("plugins"), dir.path().join("plugin-data"));
    BUNDLED.install(&root, &data_root).expect("install");

    let manifest = root.join("demo/plugin.json");
    let before = fs::metadata(&manifest)
        .and_then(|m| m.modified())
        .expect("mtime");
    BUNDLED.install(&root, &data_root).expect("reinstall");
    let after = fs::metadata(&manifest)
        .and_then(|m| m.modified())
        .expect("mtime");
    assert_eq!(before, after);
}

#[test]
fn a_local_edit_is_restored_on_the_next_run() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (root, data_root) = (dir.path().join("plugins"), dir.path().join("plugin-data"));
    BUNDLED.install(&root, &data_root).expect("install");

    let mcp = root.join("demo/mcp.json");
    fs::write(&mcp, "{}").expect("clobber");
    BUNDLED.install(&root, &data_root).expect("reinstall");
    assert!(
        fs::read_to_string(&mcp)
            .expect("read")
            .contains("mcpServers")
    );
}

#[test]
fn a_removed_builtin_is_not_reinstalled() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (root, data_root) = (dir.path().join("plugins"), dir.path().join("plugin-data"));
    fs::create_dir_all(data_root.join("demo")).expect("dirs");
    fs::write(data_root.join("demo").join(UNINSTALLED), "").expect("marker");

    BUNDLED
        .install(&root, &data_root)
        .expect("install is a no-op");
    assert!(!root.join("demo").exists());
}

#[test]
fn a_retired_bundle_is_cleared_out_of_an_existing_install() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().join("plugins");
    let stale = root.join("websearch");
    fs::create_dir_all(&stale).expect("dirs");
    fs::write(stale.join(aster_plugins::MANIFEST_FILE), MANIFEST).expect("manifest");
    fs::write(stale.join(aster_plugins::MCP_FILE), MCP).expect("mcp");

    remove_retired(&root);
    assert!(!stale.exists());
}

#[test]
fn a_package_the_user_installed_under_a_retired_name_survives() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().join("plugins");
    let theirs = root.join("websearch");
    fs::create_dir_all(&theirs).expect("dirs");
    fs::write(theirs.join(aster_plugins::MANIFEST_FILE), MANIFEST).expect("manifest");

    remove_retired(&root);
    assert!(theirs.exists());
}

#[test]
fn only_bundled_names_get_an_uninstall_marker() {
    let dir = tempfile::tempdir().expect("tempdir");
    let data_root = dir.path().join("plugin-data");
    mark_uninstalled("demo", &data_root);
    assert!(!data_root.join("demo").exists());
}
