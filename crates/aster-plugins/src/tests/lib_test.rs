use std::fs;
use std::path::{Path, PathBuf};

use super::*;

const MANIFEST: &str = r#"{
  "$schema": "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
  "name": "demo"
}"#;

struct Fixture {
    _dir: tempfile::TempDir,
    root: PathBuf,
    data_root: PathBuf,
}

impl Fixture {
    fn new(manifest: &str) -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().join("demo");
        fs::create_dir_all(&root).expect("plugin root");
        fs::write(root.join(MANIFEST_FILE), manifest).expect("manifest");
        let data_root = dir.path().join("data");
        Self {
            _dir: dir,
            root,
            data_root,
        }
    }

    fn write(&self, rel: &str, body: &str) {
        let path = self.root.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("parent");
        }
        fs::write(path, body).expect("write");
    }

    fn load(&self) -> Result<Plugin> {
        load(&self.root, &self.data_root)
    }
}

fn mcp_config(servers: &str) -> String {
    format!(
        r#"{{"$schema": "https://agent-plugins.org/schemas/1.0.0/mcp.schema.json", "mcpServers": {servers}}}"#
    )
}

#[test]
fn loads_a_minimal_plugin() {
    let fixture = Fixture::new(MANIFEST);
    let plugin = fixture.load().expect("load");
    assert_eq!(plugin.name(), "demo");
    assert!(plugin.skills.is_empty());
    assert!(plugin.servers.is_empty());
    assert!(plugin.warnings.is_empty(), "{:?}", plugin.warnings);
    assert!(plugin.data_dir.ends_with("data/demo"));
}

#[test]
fn reports_and_ignores_unknown_manifest_fields() {
    let fixture = Fixture::new(
        r#"{
          "$schema": "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
          "name": "demo",
          "commands": ["nope"]
        }"#,
    );
    let plugin = fixture.load().expect("unknown fields are not fatal");
    assert_eq!(plugin.warnings.len(), 1, "{:?}", plugin.warnings);
    assert!(plugin.warnings[0].contains("commands"));
}

#[test]
fn rejects_a_manifest_with_a_wrong_field_type() {
    let fixture = Fixture::new(
        r#"{
          "$schema": "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
          "name": "demo",
          "keywords": "one"
        }"#,
    );
    assert!(fixture.load().is_err());
}

#[test]
fn rejects_an_unsupported_spec_version() {
    let fixture = Fixture::new(
        r#"{
          "$schema": "https://agent-plugins.org/schemas/2.0.0/plugin.schema.json",
          "name": "demo"
        }"#,
    );
    let err = fixture.load().expect_err("version must be recognized");
    assert!(format!("{err:#}").contains("unsupported"), "{err:#}");
}

#[test]
fn rejects_invalid_plugin_names() {
    for name in ["My-Plugin", "-start", "has--double", "too.many..dots", ""] {
        let fixture = Fixture::new(&format!(
            r#"{{"$schema": "{PLUGIN_SCHEMA}", "name": "{name}"}}"#
        ));
        assert!(fixture.load().is_err(), "{name:?} should be rejected");
    }
    for name in ["my-plugin", "acme.tools", "lint3r", "a"] {
        let fixture = Fixture::new(&format!(
            r#"{{"$schema": "{PLUGIN_SCHEMA}", "name": "{name}"}}"#
        ));
        assert!(fixture.load().is_ok(), "{name:?} should be accepted");
    }
}

#[test]
fn keeps_extensions_for_namespaces_it_does_not_implement() {
    let fixture = Fixture::new(
        r#"{
          "$schema": "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
          "name": "demo",
          "extensions": {"com.example.client": {"setting": true}}
        }"#,
    );
    let plugin = fixture.load().expect("load");
    assert!(
        plugin
            .manifest
            .extensions
            .contains_key("com.example.client")
    );
    assert!(plugin.warnings.is_empty());
}

#[test]
fn a_non_object_extensions_field_is_reported_not_fatal() {
    let fixture = Fixture::new(
        r#"{
          "$schema": "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
          "name": "demo",
          "extensions": []
        }"#,
    );
    let plugin = fixture.load().expect("load");
    assert!(plugin.manifest.extensions.is_empty());
    assert_eq!(plugin.warnings.len(), 1, "{:?}", plugin.warnings);
}

#[test]
fn discovers_immediate_skill_directories_only() {
    let fixture = Fixture::new(MANIFEST);
    fixture.write("skills/summarize/SKILL.md", "---\nname: summarize\n---\n");
    fixture.write("skills/nested/inner/SKILL.md", "---\nname: inner\n---\n");
    let plugin = fixture.load().expect("load");
    assert_eq!(plugin.skills.len(), 1);
    assert!(plugin.skills[0].ends_with("summarize"));
}

#[test]
fn a_skills_file_instead_of_a_directory_is_reported_not_fatal() {
    let fixture = Fixture::new(MANIFEST);
    fixture.write("skills", "not a directory");
    let plugin = fixture.load().expect("load");
    assert!(plugin.skills.is_empty());
    assert_eq!(plugin.warnings.len(), 1, "{:?}", plugin.warnings);
}

#[test]
fn expands_plugin_variables_in_args_env_and_cwd() {
    let fixture = Fixture::new(MANIFEST);
    fixture.write(
        MCP_FILE,
        &mcp_config(
            r#"{"local": {
                "type": "stdio",
                "command": "npx",
                "args": ["--data", "${PLUGIN_DATA}/store"],
                "env": {"CONFIG": "${PLUGIN_ROOT}/config.json"},
                "cwd": "${PLUGIN_ROOT}"
            }}"#,
        ),
    );
    let plugin = fixture.load().expect("load");
    let (name, stdio) = plugin.stdio_servers().next().expect("one stdio server");
    assert_eq!(name, "local");
    assert_eq!(stdio.command, "npx");
    assert_eq!(
        stdio.args[1],
        format!("{}/store", plugin.data_dir.display())
    );
    assert_eq!(
        stdio.env["CONFIG"],
        format!("{}/config.json", plugin.root.display())
    );
    assert_eq!(stdio.env["PLUGIN_ROOT"], plugin.root.display().to_string());
    assert_eq!(
        stdio.env["PLUGIN_DATA"],
        plugin.data_dir.display().to_string()
    );
    assert_eq!(stdio.cwd, plugin.root);
}

#[test]
fn leaves_unrecognized_placeholders_literal() {
    assert_eq!(
        path::expand("${PLUGIN_HOME}/x ${PLUGIN_ROOT}", "/root", "/data"),
        "${PLUGIN_HOME}/x /root"
    );
}

#[test]
fn does_not_rescan_expanded_text() {
    assert_eq!(
        path::expand("${PLUGIN_ROOT}", "${PLUGIN_DATA}", "/data"),
        "${PLUGIN_DATA}"
    );
}

#[test]
fn defaults_the_working_directory_to_the_plugin_root() {
    let fixture = Fixture::new(MANIFEST);
    fixture.write(
        MCP_FILE,
        &mcp_config(r#"{"local": {"type": "stdio", "command": "npx"}}"#),
    );
    let plugin = fixture.load().expect("load");
    let (_, stdio) = plugin.stdio_servers().next().expect("one stdio server");
    assert_eq!(stdio.cwd, plugin.root);
}

#[test]
fn resolves_a_plugin_relative_command_against_the_root() {
    let fixture = Fixture::new(MANIFEST);
    fixture.write("bin/server", "#!/bin/sh\n");
    fixture.write(
        MCP_FILE,
        &mcp_config(r#"{"local": {"type": "stdio", "command": "./bin/server"}}"#),
    );
    let plugin = fixture.load().expect("load");
    let (_, stdio) = plugin.stdio_servers().next().expect("one stdio server");
    assert_eq!(
        stdio.command,
        plugin.root.join("bin/server").display().to_string()
    );
}

#[test]
fn skips_only_the_offending_server_entry() {
    let fixture = Fixture::new(MANIFEST);
    fixture.write(
        MCP_FILE,
        &mcp_config(
            r#"{
                "escaping": {"type": "stdio", "command": "../bin/server"},
                "reserved": {"type": "stdio", "command": "npx", "env": {"PLUGIN_ROOT": "/tmp"}},
                "outside": {"type": "stdio", "command": "npx", "cwd": "data"},
                "unknown": {"type": "stdio", "command": "npx", "shell": true},
                "good": {"type": "stdio", "command": "npx"}
            }"#,
        ),
    );
    let plugin = fixture.load().expect("load");
    assert_eq!(plugin.servers.len(), 1);
    assert_eq!(plugin.servers[0].name, "good");
    assert_eq!(plugin.warnings.len(), 4, "{:?}", plugin.warnings);
}

#[test]
fn a_broken_mcp_config_disables_mcp_but_keeps_skills() {
    let fixture = Fixture::new(MANIFEST);
    fixture.write("skills/summarize/SKILL.md", "---\nname: summarize\n---\n");
    fixture.write(MCP_FILE, "{ not json");
    let plugin = fixture.load().expect("load");
    assert_eq!(plugin.skills.len(), 1);
    assert!(plugin.servers.is_empty());
    assert!(
        plugin.warnings[0].contains("MCP disabled"),
        "{:?}",
        plugin.warnings
    );
}

#[test]
fn a_mismatched_mcp_schema_version_disables_mcp() {
    let fixture = Fixture::new(MANIFEST);
    fixture.write(
        MCP_FILE,
        r#"{"$schema": "https://agent-plugins.org/schemas/2.0.0/mcp.schema.json", "mcpServers": {}}"#,
    );
    let plugin = fixture.load().expect("load");
    assert!(plugin.servers.is_empty());
    assert_eq!(plugin.warnings.len(), 1, "{:?}", plugin.warnings);
}

#[test]
fn keeps_remote_servers_with_their_declared_transport() {
    let fixture = Fixture::new(MANIFEST);
    fixture.write(
        MCP_FILE,
        &mcp_config(
            r#"{
                "api": {"type": "streamable-http", "url": "https://deploy.example.com/mcp", "headers": {"X-Tenant": "public"}},
                "legacy": {"type": "sse", "url": "https://legacy.example.com/sse"},
                "local-plain": {"type": "streamable-http", "url": "http://127.0.0.1:8931/mcp"}
            }"#,
        ),
    );
    let plugin = fixture.load().expect("load");
    assert_eq!(plugin.servers.len(), 3);
    assert_eq!(plugin.servers[0].transport_name(), "streamable-http");
    assert_eq!(plugin.servers[1].transport_name(), "sse");
    assert!(plugin.warnings.is_empty(), "{:?}", plugin.warnings);
}

#[test]
fn rejects_unsafe_remote_urls() {
    for url in [
        "http://deploy.example.com/mcp",
        "https://user:pw@deploy.example.com/mcp",
        "https://deploy.example.com/mcp#frag",
        "ftp://deploy.example.com/mcp",
    ] {
        let fixture = Fixture::new(MANIFEST);
        fixture.write(
            MCP_FILE,
            &mcp_config(&format!(
                r#"{{"api": {{"type": "streamable-http", "url": "{url}"}}}}"#
            )),
        );
        let plugin = fixture.load().expect("load");
        assert!(plugin.servers.is_empty(), "{url} should be rejected");
    }
}

#[test]
fn rejects_headers_repeated_under_different_casing() {
    let fixture = Fixture::new(MANIFEST);
    fixture.write(
        MCP_FILE,
        &mcp_config(
            r#"{"api": {"type": "streamable-http", "url": "https://example.com/mcp", "headers": {"X-Tenant": "a", "x-tenant": "b"}}}"#,
        ),
    );
    let plugin = fixture.load().expect("load");
    assert!(plugin.servers.is_empty());
}

#[test]
fn discovers_installed_plugins_in_name_order() {
    let dir = tempfile::tempdir().expect("tempdir");
    for name in ["beta", "alpha"] {
        let root = dir.path().join(name);
        fs::create_dir_all(&root).expect("root");
        fs::write(
            root.join(MANIFEST_FILE),
            format!(r#"{{"$schema": "{PLUGIN_SCHEMA}", "name": "{name}"}}"#),
        )
        .expect("manifest");
    }
    fs::create_dir_all(dir.path().join("not-a-plugin")).expect("dir");
    let (plugins, problems) = discover(dir.path(), Path::new("/tmp/aster-plugin-data"));
    assert_eq!(
        plugins.iter().map(Plugin::name).collect::<Vec<_>>(),
        ["alpha", "beta"]
    );
    assert!(problems.is_empty());
}

#[test]
fn candidates_finds_plugins_at_the_root_or_one_level_down() {
    let dir = tempfile::tempdir().expect("tempdir");
    let nested = dir.path().join("plugins/tools");
    fs::create_dir_all(&nested).expect("dir");
    fs::write(nested.join(MANIFEST_FILE), MANIFEST).expect("manifest");
    assert_eq!(candidates(dir.path()), vec![nested.clone()]);
    fs::write(dir.path().join(MANIFEST_FILE), MANIFEST).expect("manifest");
    assert_eq!(candidates(dir.path()), vec![dir.path().to_path_buf()]);
}
