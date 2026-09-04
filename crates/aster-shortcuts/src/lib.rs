#![forbid(unsafe_code)]
//! Apple Shortcuts tools for Aster. [`ShortcutsBackend`] lists shortcuts with
//! `/usr/bin/shortcuts` and runs one by name with `/usr/bin/osascript` via
//! `Shortcuts Events`, so the Shortcuts app stays closed and focus is not
//! stolen. [`register_tools`] is the catalog agents discover.

use aster_mcp::McpTool;
use serde_json::{Value, json};

/// Backend that lists and runs macOS shortcuts.
#[derive(Clone, Default)]
pub struct ShortcutsBackend;

impl ShortcutsBackend {
    pub fn new() -> Self {
        Self
    }

    /// Shortcut names on this machine, one per line. Folder separator lines
    /// (`---- Folder Name`) are dropped so the list reads as flat names.
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
            .filter(|l| !l.is_empty() && !l.starts_with("---- "))
            .map(str::to_string)
            .collect())
    }

    /// Run a shortcut by name in the background. `input` becomes the shortcut's
    /// input when provided, so a shortcut that would otherwise prompt for text
    /// can be driven without a dialog.
    ///
    /// Some shortcuts write output to the clipboard instead of returning it.
    /// We snapshot the clipboard before running and include any new clipboard
    /// content in the result so those shortcuts work without modification.
    pub async fn run(&self, name: &str, input: Option<&str>) -> anyhow::Result<String> {
        let prev_clipboard = clipboard_content().await;

        let script = match input {
            Some(input) => format!(
                "tell application \"Shortcuts Events\" to run shortcut \"{}\" with input \"{}\"",
                escape_applescript(name),
                escape_applescript(input),
            ),
            None => format!(
                "tell application \"Shortcuts Events\" to run shortcut \"{}\"",
                escape_applescript(name),
            ),
        };
        let output = tokio::process::Command::new("/usr/bin/osascript")
            .arg("-e")
            .arg(&script)
            .output()
            .await?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            if stderr.contains("-10004") || stderr.to_lowercase().contains("privilege violation") {
                anyhow::bail!(
                    "could not run '{name}': macOS denied automation access. Open \
                     System Settings > Privacy & Security > Automation and allow Aster \
                     to control Shortcuts Events, then try again."
                );
            }
            if stderr.contains("-128") || stderr.to_lowercase().contains("user canceled") {
                anyhow::bail!(
                    "'{name}' was canceled. If it asks for input, pass it with the \
                     `input` argument instead of leaving a dialog to fill in."
                );
            }
            anyhow::bail!("shortcuts run '{name}' failed: {stderr}");
        }

        let mut result = String::from_utf8_lossy(&output.stdout).trim().to_string();

        let new_clipboard = clipboard_content().await;
        if let (Some(prev), Some(new)) = (&prev_clipboard, &new_clipboard)
            && prev != new
            && !result.contains(new)
        {
            if !result.is_empty() {
                result.push('\n');
            }
            result.push_str(new);
        }

        Ok(result)
    }

    /// Run one tool by its bare name. The single dispatch table.
    pub async fn call(&self, tool: &str, arguments: &Value) -> anyhow::Result<Value> {
        use anyhow::Context;
        match tool {
            "list" => {
                let shortcuts = self.list().await?;
                Ok(json!({ "count": shortcuts.len(), "shortcuts": shortcuts }))
            }
            "run" => {
                let name = arguments["name"].as_str().context("missing name")?;
                let input = arguments["input"].as_str();
                let output = self.run(name, input).await?;
                Ok(json!({ "output": output }))
            }
            other => anyhow::bail!("unknown shortcuts tool: {other}"),
        }
    }
}

/// Escape a string for embedding inside an AppleScript double-quoted literal.
fn escape_applescript(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Read the current macOS clipboard content, or None if it can't be read.
async fn clipboard_content() -> Option<String> {
    let out = tokio::process::Command::new("/usr/bin/pbpaste")
        .output()
        .await
        .ok()?;
    if out.status.success() {
        Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        None
    }
}

/// Register the tools an agent can discover.
pub fn register_tools() -> Vec<McpTool> {
    vec![
        McpTool {
            server: "shortcuts".into(),
            name: "list".into(),
            description: "List every macOS Shortcut on this machine by name. Call this first to discover shortcut names before running one.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        },
        McpTool {
            server: "shortcuts".into(),
            name: "run".into(),
            description: "Run a macOS Shortcut by exact name in the background and return its output. Pass `input` to feed the shortcut a value instead of letting it prompt.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Exact name of the shortcut to run"
                    },
                    "input": {
                        "type": "string",
                        "description": "Optional text to pass as the shortcut's input"
                    }
                },
                "required": ["name"]
            }),
        },
    ]
}
