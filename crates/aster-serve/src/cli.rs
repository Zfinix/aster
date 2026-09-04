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
        let mut child = self
            .command(args)
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
        let mut args = args.to_vec();
        args.push("--json");
        let out = self.run(&args, None).await?;
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
