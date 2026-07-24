use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use tauri_plugin_dialog::DialogExt;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReviewOpts {
    repo_path: String,
    source_kind: String,
    source_value: Option<String>,
    min_confidence: Option<f32>,
    no_index: bool,
    model: Option<String>,
    api_key: Option<String>,
    #[serde(default)]
    analyzers: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct StartupInfo {
    default_repo: Option<String>,
    bin_path: Option<String>,
}

/// Persisted provider settings for the LLM. aster resolves these from the
/// environment (`ASTER_API_KEY` / `ASTER_BASE_URL` / `ASTER_MODEL`); we store
/// them once, next to the CLI's own `credentials.json`, and inject them as env
/// when spawning the CLI. That keeps one source of truth instead of relying on
/// a per-repo `.env` a launched app can't see.
#[derive(Debug, Default, Serialize, Deserialize)]
struct Provider {
    api_key: Option<String>,
    base_url: Option<String>,
    model: Option<String>,
}

/// What the UI is allowed to know: whether a key is set, and the non-secret
/// settings. The key itself is never returned.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AuthStatus {
    has_key: bool,
    base_url: Option<String>,
    model: Option<String>,
}

fn provider_path() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("aster").join("desktop.json"))
}

fn load_provider() -> Provider {
    provider_path()
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn write_provider(p: &Provider) -> Result<(), String> {
    let path = provider_path().ok_or("no config directory on this platform")?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let bytes = serde_json::to_vec_pretty(p).map_err(|e| e.to_string())?;
    // Mirror the CLI's 0600 write so the key is never world-readable.
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&path)
            .map_err(|e| e.to_string())?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| e.to_string())?;
        f.write_all(&bytes).map_err(|e| e.to_string())?;
    }
    #[cfg(not(unix))]
    std::fs::write(&path, &bytes).map_err(|e| e.to_string())?;
    Ok(())
}

/// Inject the persisted provider settings as env, never clobbering a var the
/// process already has (a repo `.env` / shell export still wins).
/// `model_override` (the composer's model pill) wins over the stored default.
fn inject_provider_env(
    cmd: &mut Command,
    model_override: Option<String>,
    api_key_override: Option<String>,
) {
    let provider = load_provider();
    if std::env::var("ASTER_API_KEY").is_err() {
        if let Some(key) = provider.api_key.filter(|k| !k.trim().is_empty()) {
            cmd.env("ASTER_API_KEY", key);
        }
    }
    if std::env::var("ASTER_BASE_URL").is_err() {
        if let Some(url) = provider.base_url.filter(|u| !u.trim().is_empty()) {
            cmd.env("ASTER_BASE_URL", url);
        }
    }
    let model = model_override
        .filter(|m| !m.trim().is_empty())
        .or(provider.model.filter(|m| !m.trim().is_empty()));
    if let Some(model) = model {
        cmd.env("ASTER_MODEL", model);
    }
    if let Some(key) = api_key_override.filter(|k| !k.is_empty()) {
        cmd.env("ASTER_API_KEY", key);
    }
}

/// The CLI's provider errors are shell-oriented; append the app's own remedy.
fn friendly_provider_error(msg: String) -> String {
    if msg.contains("no API key") {
        format!("{msg}\nYou can also add a key in Settings, under Provider.")
    } else {
        msg
    }
}

/// The useful lines of a failed CLI run: ANSI-stripped stderr with clap's
/// boilerplate footer dropped, so the surfaced error is the real one.
fn stderr_tail(lines: &[String]) -> String {
    lines
        .iter()
        .filter(|l| {
            let t = l.trim();
            !t.starts_with("For more information") && !t.starts_with("Usage: aster")
        })
        .cloned()
        .collect::<Vec<_>>()
        .join("\n")
}

#[derive(Debug, Serialize, Deserialize)]
struct ChatMessage {
    role: String,
    content: String,
}

/// Spawn an `aster` subcommand with the repo as cwd, write `payload` to its
/// stdin, and return its parsed stdout JSON. Shared by `chat` and `apply_fix`
/// so both surface errors the same way.
async fn run_aster_json(
    cli_args: &[&str],
    repo_path: Option<&str>,
    model: Option<String>,
    payload: Vec<u8>,
) -> Result<serde_json::Value, String> {
    let mut cmd = Command::new(resolve_bin());
    cmd.args(cli_args);
    if let Some(repo) = repo_path.filter(|p| !p.is_empty() && std::path::Path::new(p).is_dir()) {
        cmd.current_dir(repo);
    }
    cmd.env("PATH", augmented_path());
    inject_provider_env(&mut cmd, model, None);
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("could not launch aster: {e}"))?;

    if let Some(mut stdin) = child.stdin.take() {
        use tokio::io::AsyncWriteExt;
        stdin
            .write_all(&payload)
            .await
            .map_err(|e| format!("could not send input to aster: {e}"))?;
        // Drop closes the pipe so the CLI sees EOF and starts the request.
    }

    let out = child
        .wait_with_output()
        .await
        .map_err(|e| format!("aster did not finish: {e}"))?;

    if !out.status.success() {
        let lines: Vec<String> = String::from_utf8_lossy(&out.stderr)
            .lines()
            .map(strip_ansi)
            .filter(|l| !l.trim().is_empty())
            .collect();
        let tail = stderr_tail(&lines[lines.len().saturating_sub(20)..]);
        return Err(friendly_provider_error(if tail.trim().is_empty() {
            format!("aster exited with {}", out.status)
        } else {
            tail
        }));
    }

    let stdout = String::from_utf8_lossy(&out.stdout);
    serde_json::from_str::<serde_json::Value>(stdout.trim())
        .map_err(|_| "unexpected output from aster".to_string())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ChatReply {
    reply: String,
    edits: Vec<String>,
}

/// A single-shot conversational reply from Aster, governed by the agent skill.
/// Spawns `aster chat` with the repo as cwd so provider resolution (repo
/// `.env`, env, aster.yaml, stored settings) is identical to `run_review`.
/// The agent may read/search the repo; with `allow_edits` it may change it.
#[tauri::command]
async fn chat(
    messages: Vec<ChatMessage>,
    repo_path: Option<String>,
    model: Option<String>,
    allow_edits: Option<bool>,
) -> Result<ChatReply, String> {
    let mut args = vec!["chat", "--messages-json", "-", "--json"];
    if allow_edits.unwrap_or(false) {
        args.push("--allow-edits");
    }
    let payload = serde_json::to_vec(&messages).map_err(|e| e.to_string())?;
    let v = run_aster_json(&args, repo_path.as_deref(), model, payload).await?;
    let reply = v["reply"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| "unexpected output from aster chat".to_string())?;
    let edits = v["edits"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|e| e.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    Ok(ChatReply { reply, edits })
}

/// Ask the fix engine to patch one finding in place. Returns the CLI's
/// per-finding result object (status, reason, patch).
#[tauri::command]
async fn apply_fix(
    finding: serde_json::Value,
    repo_path: Option<String>,
    model: Option<String>,
) -> Result<serde_json::Value, String> {
    let payload = serde_json::to_vec(&vec![finding]).map_err(|e| e.to_string())?;
    let v = run_aster_json(
        &["fix", "--findings-json", "-", "--json", "--apply"],
        repo_path.as_deref(),
        model,
        payload,
    )
    .await?;
    v.as_array()
        .and_then(|a| a.first().cloned())
        .ok_or_else(|| "unexpected output from aster fix".to_string())
}

#[tauri::command]
fn auth_status() -> AuthStatus {
    let p = load_provider();
    let env_key = std::env::var("ASTER_API_KEY")
        .or_else(|_| std::env::var("OPEN_ROUTER_API_KEY"))
        .ok()
        .filter(|s| !s.trim().is_empty())
        .is_some();
    AuthStatus {
        has_key: env_key || p.api_key.as_deref().is_some_and(|k| !k.trim().is_empty()),
        base_url: p.base_url,
        model: p.model,
    }
}

#[tauri::command]
fn save_provider(
    api_key: Option<String>,
    base_url: Option<String>,
    model: Option<String>,
) -> Result<(), String> {
    let mut p = load_provider();
    // An omitted / empty key leaves the stored one untouched; explicit clears
    // are handled by passing a single space, which we treat as "remove".
    match api_key.as_deref() {
        Some(" ") => p.api_key = None,
        Some(k) if !k.trim().is_empty() => p.api_key = Some(k.trim().to_string()),
        _ => {}
    }
    p.base_url = base_url.filter(|s| !s.trim().is_empty());
    p.model = model.filter(|s| !s.trim().is_empty());
    write_provider(&p)
}

/// The command used to spawn the CLI. Prefers an explicit override, then a
/// freshly built workspace binary, and finally `aster` on PATH.
fn resolve_bin() -> String {
    if let Ok(p) = std::env::var("ASTER_BIN") {
        if !p.is_empty() {
            return p;
        }
    }
    find_workspace_bin()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| "aster".to_string())
}

/// A concrete binary on disk, if one exists next to the workspace. Picks the
/// **most recently built** of release/debug, not a fixed profile order: a stale
/// release build otherwise shadows a fresh debug one and runs old CLI args.
fn find_workspace_bin() -> Option<PathBuf> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()?
        .parent()?
        .to_path_buf();
    ["release", "debug"]
        .iter()
        .map(|p| root.join("target").join(p).join("aster"))
        .filter(|c| c.is_file())
        .max_by_key(|c| {
            std::fs::metadata(c)
                .and_then(|m| m.modified())
                .ok()
        })
}

fn default_repo() -> Option<String> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()?
        .parent()?
        .to_path_buf();
    if root.join(".git").exists() {
        return Some(root.to_string_lossy().into_owned());
    }
    None
}

/// Strip ANSI escape sequences so the live feed reads as plain text; the CLI's
/// streaming path emits color codes regardless of NO_COLOR.
fn strip_ansi(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for e in chars.by_ref() {
                    if e.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

#[tauri::command]
fn startup_info() -> StartupInfo {
    StartupInfo {
        default_repo: default_repo(),
        bin_path: find_workspace_bin().map(|p| p.to_string_lossy().into_owned()),
    }
}

#[tauri::command]
async fn pick_diff(app: AppHandle) -> Option<String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .add_filter("diff", &["diff", "patch", "txt"])
        .pick_file(move |path| {
            let _ = tx.send(path);
        });
    rx.await
        .ok()
        .flatten()
        .and_then(|p| p.into_path().ok())
        .map(|p| p.to_string_lossy().into_owned())
}

#[tauri::command]
async fn pick_repo(app: AppHandle) -> Option<String> {
    // Must be non-blocking: `blocking_pick_folder` on the main thread deadlocks
    // the webview (the folder dialog appears to freeze the app). Drive the async
    // picker and await the result over a channel instead.
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog().file().pick_folder(move |path| {
        let _ = tx.send(path);
    });
    rx.await
        .ok()
        .flatten()
        .and_then(|p| p.into_path().ok())
        .map(|p| p.to_string_lossy().into_owned())
}

/// A launched macOS/Linux GUI app inherits a minimal `PATH`, so the CLI's own
/// subprocesses (`git`, `gh`) can go missing even when they work in a shell.
/// Prepend the common install locations so those tools resolve.
fn augmented_path() -> String {
    let existing = std::env::var("PATH").unwrap_or_default();
    let extras = [
        "/opt/homebrew/bin",
        "/usr/local/bin",
        "/usr/bin",
        "/bin",
    ];
    let mut parts: Vec<String> = extras.iter().map(|s| s.to_string()).collect();
    for p in existing.split(':').filter(|p| !p.is_empty()) {
        if !parts.iter().any(|e| e == p) {
            parts.push(p.to_string());
        }
    }
    parts.join(":")
}

#[tauri::command]
async fn run_review(app: AppHandle, opts: ReviewOpts) -> Result<(), String> {
    let mut cmd = Command::new(resolve_bin());
    cmd.arg("review").arg("--stream");
    cmd.current_dir(&opts.repo_path);
    cmd.env("PATH", augmented_path());

    match opts.source_kind.as_str() {
        "range" => {
            if let Some(v) = opts.source_value.filter(|v| !v.is_empty()) {
                cmd.arg("--range").arg(v);
            }
        }
        "pr" => {
            if let Some(v) = opts.source_value.filter(|v| !v.is_empty()) {
                cmd.arg("--pr").arg(v);
            }
        }
        "diff" => {
            if let Some(v) = opts.source_value.filter(|v| !v.is_empty()) {
                cmd.arg("--diff").arg(v);
            }
        }
        _ => {}
    }
    if opts.no_index {
        cmd.arg("--no-index");
    }
    if !opts.analyzers.is_empty() {
        cmd.env("ASTER_ANALYZERS", opts.analyzers.join(","));
    }
    if let Some(mc) = opts.min_confidence {
        cmd.arg("--min-confidence").arg(mc.to_string());
    }

    inject_provider_env(&mut cmd, opts.model, opts.api_key);

    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("could not launch aster: {e}"))?;

    // stderr is the CLI's log feed in stream mode. Forward it to the activity
    // log, and keep the last several lines so a failure surfaces the real error
    // (the useful message is often above clap's "try --help" footer).
    let err_buf: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    if let Some(stderr) = child.stderr.take() {
        let app = app.clone();
        let err_buf = err_buf.clone();
        tauri::async_runtime::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let clean = strip_ansi(&line);
                if !clean.trim().is_empty() {
                    let mut b = err_buf.lock().unwrap();
                    b.push(clean.clone());
                    let overflow = b.len().saturating_sub(20);
                    if overflow > 0 {
                        b.drain(0..overflow);
                    }
                }
                let _ = app.emit("aster://log", clean);
            }
        });
    }

    // stdout is pure NDJSON: forward every line to the UI as a structured event.
    if let Some(stdout) = child.stdout.take() {
        let app = app.clone();
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if !line.trim().is_empty() {
                let _ = app.emit("aster://event", line);
            }
        }
    }

    let status = child
        .wait()
        .await
        .map_err(|e| format!("aster did not finish: {e}"))?;

    if !status.success() {
        let tail = stderr_tail(&err_buf.lock().unwrap());
        return Err(friendly_provider_error(if tail.trim().is_empty() {
            format!("aster exited with {status}")
        } else {
            tail
        }));
    }
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            startup_info,
            pick_repo,
            pick_diff,
            run_review,
            auth_status,
            save_provider,
            chat,
            apply_fix
        ])
        .run(tauri::generate_context!())
        .expect("error while running aster desktop");
}
