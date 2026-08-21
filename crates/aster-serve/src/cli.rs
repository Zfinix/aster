//! Running the `aster` binary. Every answer the browser gets comes from a CLI
//! run in the served repo, the same way the editor extension gets its own.

use std::path::PathBuf;
use std::process::Stdio;

use serde_json::Value;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use crate::settings::ProviderOverride;

pub struct Cli {
    /// The binary turns are spawned from: this one, unless `ASTER_BIN` says
    /// otherwise. `aster serve` is the CLI, so a turn runs the build the
    /// browser is already talking to.
    pub bin: PathBuf,
    /// The repo every run happens in: where the server was started.
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

    /// An `aster` invocation in the served repo. A provider chosen in the UI
    /// travels as environment, so it belongs to this server rather than being
    /// written into the repo's aster.yaml.
    pub fn command(&self, args: &[&str], provider: Option<&ProviderOverride>) -> Command {
        let mut cmd = Command::new(&self.bin);
        cmd.args(args).current_dir(&self.root);
        if let Some(provider) = provider {
            cmd.env("ASTER_BASE_URL", &provider.base_url);
            if let Some(key) = provider.key() {
                cmd.env("ASTER_API_KEY", key);
            }
        }
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        cmd
    }

    /// Run to completion, writing `stdin` first.
    pub async fn run(
        &self,
        args: &[&str],
        stdin: Option<&str>,
        provider: Option<&ProviderOverride>,
    ) -> Result<Output, String> {
        let mut child = self
            .command(args, provider)
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

    /// `aster <args> --json`, parsed. `--json` turns failures into
    /// `{ok: false, error}` rather than stderr, so those come back as errors.
    pub async fn json(
        &self,
        args: &[&str],
        provider: Option<&ProviderOverride>,
    ) -> Result<Value, String> {
        let mut args = args.to_vec();
        args.push("--json");
        let out = self.run(&args, None, provider).await?;
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
