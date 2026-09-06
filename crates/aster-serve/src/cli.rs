//! Running the `aster` binary. Every answer the browser gets comes from a CLI
//! run in the served repo, the same way the editor extension gets its own.

use std::path::PathBuf;
use std::process::Stdio;

use serde_json::Value;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

pub struct Cli {
    pub bin: PathBuf,
    pub root: PathBuf,
}

pub struct Output {
    pub stdout: String,
    pub stderr: String,
    pub code: i32,
}

impl Cli {
    pub fn new(root: PathBuf) -> Self {
        let bin = match std::env::var("ASTER_BIN") {
            Ok(bin) if !bin.trim().is_empty() => PathBuf::from(bin),
            _ => std::env::current_exe().unwrap_or_else(|_| PathBuf::from("aster")),
        };
        Self { bin, root }
    }

    pub fn command(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new(&self.bin);
        cmd.args(args).current_dir(&self.root);
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        cmd
    }

    pub async fn run(&self, args: &[&str], stdin: Option<&str>) -> Result<Output, String> {
        self.run_with(self.command(args), stdin).await
    }

    /// A run under a changed environment: `set` adds or replaces variables,
    /// `unset` removes them for this child only.
    pub async fn run_env(
        &self,
        args: &[&str],
        set: &[(&str, &str)],
        unset: &[&str],
    ) -> Result<Output, String> {
        let mut cmd = self.command(args);
        for name in unset {
            cmd.env_remove(name);
        }
        for (name, value) in set {
            cmd.env(name, value);
        }
        self.run_with(cmd, None).await
    }

    async fn run_with(&self, mut cmd: Command, stdin: Option<&str>) -> Result<Output, String> {
        let mut child = cmd
            .spawn()
            .map_err(|e| format!("could not run {}: {e}", self.bin.display()))?;
        if let Some(mut pipe) = child.stdin.take() {
            pipe.write_all(stdin.unwrap_or_default().as_bytes())
                .await
                .map_err(|e| format!("could not send input to aster: {e}"))?;
            // Dropping it closes the pipe, so the CLI sees EOF and starts work.
        }
        let out = child
            .wait_with_output()
            .await
            .map_err(|e| format!("aster did not finish: {e}"))?;
        Ok(Output {
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
            code: out.status.code().unwrap_or(0),
        })
    }

    pub async fn json(&self, args: &[&str]) -> Result<Value, String> {
        self.json_in(args, None).await
    }

    /// `json` with something on stdin, for the commands that read a secret
    /// there rather than from the process list.
    pub async fn json_in(&self, args: &[&str], stdin: Option<&str>) -> Result<Value, String> {
        let mut args = args.to_vec();
        args.push("--json");
        let out = self.run(&args, stdin).await?;
        let parsed: Value = serde_json::from_str(out.stdout.trim()).map_err(|_| {
            let stderr = out.stderr.trim();
            match stderr.is_empty() {
                true => format!("aster {} failed (exit {})", args.join(" "), out.code),
                false => stderr.to_string(),
            }
        })?;
        if parsed.get("ok") == Some(&Value::Bool(false)) {
            return Err(parsed
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("the command failed")
                .to_string());
        }
        Ok(parsed)
    }
}
