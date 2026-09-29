//! The composer's mic button runs one `aster dictate` child at a time. Its
//! NDJSON goes to the UI as `aster://dictation`; stopping writes it a newline.

use std::process::Stdio;
use std::sync::Mutex;

use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};

struct Active {
    child: Child,
    stdin: Option<ChildStdin>,
}

static ACTIVE: Mutex<Option<Active>> = Mutex::new(None);

#[tauri::command]
pub(crate) async fn start_dictation(app: AppHandle) -> Result<(), String> {
    let mut cmd = Command::new(crate::resolve_bin());
    crate::hide_console(&mut cmd);
    cmd.arg("dictate")
        .env("PATH", crate::augmented_path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("could not launch aster: {e}"))?;
    let stdin = child.stdin.take();
    let stdout = child.stdout.take().ok_or("aster dictate has no stdout")?;
    let previous = ACTIVE
        .lock()
        .map_err(|e| e.to_string())?
        .replace(Active { child, stdin });
    if let Some(mut previous) = previous {
        let _ = previous.child.start_kill();
    }

    tauri::async_runtime::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if !line.trim().is_empty() {
                let _ = app.emit("aster://dictation", line);
            }
        }
        let _ = app.emit("aster://dictation", r#"{"type":"closed"}"#);
    });
    Ok(())
}

#[tauri::command]
pub(crate) async fn stop_dictation() -> Result<(), String> {
    let stdin = ACTIVE
        .lock()
        .map_err(|e| e.to_string())?
        .as_mut()
        .and_then(|active| active.stdin.take());
    let Some(mut stdin) = stdin else {
        return Ok(());
    };
    stdin
        .write_all(b"\n")
        .await
        .map_err(|e| format!("could not stop the recording: {e}"))
}

#[tauri::command]
pub(crate) async fn cancel_dictation() -> Result<(), String> {
    let active = ACTIVE.lock().map_err(|e| e.to_string())?.take();
    if let Some(mut active) = active {
        let _ = active.child.start_kill();
    }
    Ok(())
}
