#![forbid(unsafe_code)]
//! Apple Shortcuts tools for Aster: list and run macOS shortcuts via the
//! `shortcuts` CLI. [`ShortcutsBackend`] shells out to `/usr/bin/shortcuts`;
//! [`register_tools`] is the catalog agents discover.

use aster_mcp::McpTool;
use serde_json::{Value, json};

/// Backend that shells out to `/usr/bin/shortcuts`.
#[derive(Clone, Default)]
pub struct ShortcutsBackend;

impl ShortcutsBackend {
    pub fn new() -> Self {
        Self
    }

    /// List every shortcut available on this Mac.
    pub async fn list(&self) -> anyhow::Result<Vec<String>> {
        let output = tokio::process::Command::new("/usr/bin/shortcuts")
            .arg("list")
            .output()
            .await?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("shortcuts list failed: {stderr}");
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(stdout
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(str::to_string)
            .collect())
    }

    /// Run a shortcut by name via `osascript`, which gives the shortcut
    /// GUI access for dialogs that `shortcuts run` (headless) would block on.
    pub async fn run(&self, name: &str) -> anyhow::Result<String> {
        let escaped = name.replace('\\', "\\\\").replace('"', "\\\"");
        let script = format!("tell application \"Shortcuts\" to run shortcut \"{escaped}\"");
        let output = tokio::process::Command::new("/usr/bin/osascript")
            .arg("-e")
            .arg(&script)
            .output()
            .await?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("shortcuts run '{name}' failed: {stderr}");
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    /// Run one tool by its bare name. The single dispatch table.
    pub async fn call(&self, tool: &str, arguments: &Value) -> anyhow::Result<Value> {
        use anyhow::Context;
        match tool {
            "list" => {
                let shortcuts = self.list().await?;
                Ok(json!({ "shortcuts": shortcuts }))
            }
            "run" => {
                let name = arguments["name"].as_str().context("missing name")?;
                let output = self.run(name).await?;
                Ok(json!({ "output": output }))
            }
            other => anyhow::bail!("unknown shortcuts tool: {other}"),
        }
    }
}

/// Register the tools an agent can discover.
pub fn register_tools() -> Vec<McpTool> {
    vec![
        McpTool {
            server: "shortcuts".into(),
            name: "list".into(),
            description: "List all available macOS Shortcuts on this machine.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        },
        McpTool {
            server: "shortcuts".into(),
            name: "run".into(),
            description: "Run a macOS Shortcut by name and return its output.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Exact name of the shortcut to run"
                    }
                },
                "required": ["name"]
            }),
        },
    ]
}

#[cfg(test)]
#[path = "tests/lib_test.rs"]
mod tests;
