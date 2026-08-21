use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use tauri_plugin_dialog::DialogExt;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};

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

/// Persisted provider settings, injected as env (`ASTER_API_KEY` / `ASTER_BASE_URL`
/// / `ASTER_MODEL`) when spawning the CLI, since a launched app can't see a repo
/// `.env`.
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

/// Spawn an `aster` subcommand with the repo as cwd, write `payload` to stdin,
/// and return its parsed stdout JSON. Shared by `chat` and `apply_fix`.
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

/// The one live `chat --stream` child, so a turn can be cancelled and a new
/// turn never overlaps an old one. Mirrors the extension's ChatRunner slot.
/// The mutex only ever guards a state swap; it is never held across an await
/// (std MutexGuards are not Send, which Tauri commands require).
static ACTIVE_CHAT: Mutex<Option<ActiveChat>> = Mutex::new(None);

struct ActiveChat {
    child: Child,
}

/// Free the slot if the child in it already exited. True when no turn is live.
fn reap_finished_chat() -> Result<bool, String> {
    let mut slot = ACTIVE_CHAT.lock().map_err(|e| e.to_string())?;
    match slot.as_mut() {
        None => Ok(true),
        Some(active) => match active.child.try_wait() {
            Ok(Some(_)) => {
                slot.take();
                Ok(true)
            }
            Ok(None) => Ok(false),
            Err(e) => Err(e.to_string()),
        },
    }
}

/// Spawn a streaming chat turn. Stdout NDJSON is forwarded line-by-line as
/// `aster://chat-event` payloads and stderr as `aster://log`; the returned
/// future resolves once the child is launched — turn end arrives as a `done`
/// or `error` event, and process exit as `aster://chat-exit`. Approval
/// replies go through `answer_approval`.
#[tauri::command]
async fn chat(
    app: AppHandle,
    messages: Vec<ChatMessage>,
    repo_path: Option<String>,
    model: Option<String>,
    permission_mode: Option<String>,
    session: Option<String>,
) -> Result<(), String> {
    let mut args: Vec<String> = vec![
        "chat".into(),
        "--messages-json".into(),
        "-".into(),
        "--stream".into(),
    ];
    match permission_mode.as_deref().filter(|m| !m.is_empty()) {
        Some(mode @ ("plan" | "manual" | "auto" | "edit" | "yolo")) => {
            args.push("--permission-mode".into());
            args.push(mode.into());
        }
        // No explicit mode keeps the previous behavior: edits allowed, gated
        // only by the repo's aster.yaml.
        None => args.push("--allow-edits".into()),
        Some(other) => return Err(format!("unknown permission mode {other:?}")),
    }
    if let Some(session) = session.as_deref().filter(|s| !s.is_empty()) {
        args.push("--session".into());
        args.push(session.into());
    }

    let mut cmd = Command::new(resolve_bin());
    cmd.args(&args);
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

    // Reap a finished child before taking the slot; a live one means the UI
    // double-sent, so refuse rather than run two turns at once.
    if !reap_finished_chat()? {
        let _ = child.kill().await;
        return Err("a chat turn is already running".into());
    }

    // The messages go in as a single line; stdin then stays open, because
    // that is the channel the CLI reads approval replies from.
    let mut stdin = child.stdin.take().ok_or("aster chat has no stdin")?;
    let line = serde_json::to_string(&messages).map_err(|e| e.to_string())?;
    stdin
        .write_all(line.as_bytes())
        .await
        .map_err(|e| format!("could not send messages to aster: {e}"))?;
    stdin
        .write_all(b"\n")
        .await
        .map_err(|e| format!("could not send messages to aster: {e}"))?;

    // stderr is the CLI's log feed; keep a tail so a crashed turn (one that
    // never emitted a terminal event) still surfaces the real error.
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
                    let _ = app.emit("aster://log", clean);
                }
            }
        });
    }

    // stdout is pure NDJSON: forward every line to the UI as a chat event.
    if let Some(stdout) = child.stdout.take() {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if !line.trim().is_empty() {
                    let _ = app.emit("aster://chat-event", line);
                }
            }
        });
    }

    // Watch for exit so the UI can settle even when the stream went quiet:
    // emit the exit code (None = killed) plus the stderr tail on failure.
    let app_exit = app.clone();
    tauri::async_runtime::spawn(async move {
        let status = loop {
            let poll = {
                let mut slot = ACTIVE_CHAT.lock().unwrap();
                match slot.as_mut() {
                    None => None, // cancelled; cancel_chat already reaped
                    Some(active) => Some(active.child.try_wait()),
                }
            };
            match poll {
                None => return,
                Some(Ok(Some(status))) => break status,
                Some(Ok(None)) => {}
                Some(Err(_)) => return,
            }
            tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        };
        ACTIVE_CHAT.lock().unwrap().take();
        let code = status.code();
        let error = if code.is_some_and(|c| c != 0) {
            let tail = stderr_tail(&err_buf.lock().unwrap());
            Some(friendly_provider_error(if tail.trim().is_empty() {
                format!("aster exited with {status}")
            } else {
                tail
            }))
        } else {
            None
        };
        let _ = app_exit.emit(
            "aster://chat-exit",
            serde_json::json!({ "code": code, "error": error }),
        );
    });

    *ACTIVE_CHAT.lock().map_err(|e| e.to_string())? = Some(ActiveChat { child });
    Ok(())
}

/// Answer a pending `approval_request`; the CLI reads one `{"allow": bool}`
/// line per prompt.
#[tauri::command]
async fn answer_approval(allow: bool) -> Result<(), String> {
    let mut stdin = {
        let mut slot = ACTIVE_CHAT.lock().map_err(|e| e.to_string())?;
        slot.as_mut()
            .and_then(|active| active.child.stdin.take())
            .ok_or("no chat turn is waiting for an answer")?
    };
    stdin
        .write_all(format!("{{\"allow\":{allow}}}\n").as_bytes())
        .await
        .map_err(|e| format!("could not answer the approval prompt: {e}"))
}

/// Stop the running turn. The monitor task sees the kill, frees the slot, and
/// reports a null exit code so the UI settles without an error.
#[tauri::command]
async fn cancel_chat() -> Result<(), String> {
    let active = ACTIVE_CHAT.lock().map_err(|e| e.to_string())?.take();
    if let Some(mut active) = active {
        let _ = active.child.kill().await;
    }
    Ok(())
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
async fn list_sessions(repo_path: Option<String>) -> Result<serde_json::Value, String> {
    run_aster_json(
        &["sessions", "list", "--json"],
        repo_path.as_deref(),
        None,
        Vec::new(),
    )
    .await
}

#[tauri::command]
async fn show_session(id: String, repo_path: Option<String>) -> Result<serde_json::Value, String> {
    run_aster_json(
        &["sessions", "show", &id, "--json"],
        repo_path.as_deref(),
        None,
        Vec::new(),
    )
    .await
}

#[tauri::command]
async fn delete_session(
    id: String,
    repo_path: Option<String>,
) -> Result<serde_json::Value, String> {
    run_aster_json(
        &["sessions", "delete", &id, "--json"],
        repo_path.as_deref(),
        None,
        Vec::new(),
    )
    .await
}

#[tauri::command]
async fn memory_list() -> Result<serde_json::Value, String> {
    run_aster_json(&["memory", "list", "--json"], None, None, Vec::new()).await
}

/// Save a durable fact. With `title` it writes a named block; otherwise it
/// appends to project memory.
#[tauri::command]
async fn memory_add(text: String, title: Option<String>) -> Result<serde_json::Value, String> {
    let mut args: Vec<String> = vec!["memory".into(), "add".into(), "--json".into()];
    if let Some(title) = &title {
        args.push("--title".into());
        args.push(title.clone());
    }
    args.push("--".into());
    args.push(text);
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    run_aster_json(&refs, None, None, Vec::new()).await
}

/// The shared catalog, the same file `aster_ai::keys` reads. Parsed here rather
/// than imported because the desktop shell is a standalone workspace, kept that
/// way so it never perturbs the library crates or their lockfile.
const PROVIDERS_JSON: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../providers.json"));

#[derive(serde::Deserialize)]
struct KeyCatalog {
    providers: Vec<KeyCatalogEntry>,
}

#[derive(serde::Deserialize)]
struct KeyCatalogEntry {
    base_url: String,
    #[serde(default)]
    key_env: Vec<String>,
}

fn host_only(url: &str) -> &str {
    url.split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(url)
        .split('/')
        .next()
        .unwrap_or(url)
}

/// Mirrors `aster_ai::keys::provider_key_vars`, off the same catalog, so an
/// endpoint's key lives in the same var whichever surface stored it.
fn provider_key_vars(base_url: &str) -> &'static [&'static str] {
    static TABLE: std::sync::OnceLock<Vec<(Vec<String>, &'static [&'static str])>> =
        std::sync::OnceLock::new();
    let table = TABLE.get_or_init(|| {
        let Ok(catalog) = serde_json::from_str::<KeyCatalog>(PROVIDERS_JSON) else {
            return Vec::new();
        };
        catalog
            .providers
            .into_iter()
            .filter(|entry| !entry.key_env.is_empty())
            .filter_map(|entry| {
                let vars: &'static [&'static str] = Box::leak(
                    entry
                        .key_env
                        .into_iter()
                        .map(|var| &*Box::leak(var.into_boxed_str()))
                        .collect::<Vec<_>>()
                        .into_boxed_slice(),
                );
                let host = host_only(entry.base_url.trim_end_matches('/'));
                let segments: Vec<String> = host
                    .split(|c| c == '{' || c == '}')
                    .enumerate()
                    .filter(|(i, part)| i % 2 == 0 && !part.is_empty())
                    .map(|(_, part)| part.to_string())
                    .collect();
                (!segments.is_empty()).then_some((segments, vars))
            })
            .collect()
    });
    let host = host_only(base_url.trim_end_matches('/'));
    // An exact host wins, so the plainer of two overlapping hosts cannot
    // swallow the more specific one.
    table
        .iter()
        .find(|(segments, _)| segments.len() == 1 && segments[0] == host)
        .or_else(|| {
            table
                .iter()
                .find(|(segments, _)| segments.iter().all(|s| host.contains(s.as_str())))
        })
        .map(|(_, vars)| *vars)
        .unwrap_or(&[])
}

#[tauri::command]
fn auth_status() -> AuthStatus {
    let p = load_provider();
    // A key named for another vendor is not a key for this endpoint, so only
    // this endpoint's var and the shared one count as configured.
    let env_key = provider_key_vars(p.base_url.as_deref().unwrap_or_default())
        .iter()
        .copied()
        .chain(["ASTER_API_KEY"])
        .any(|var| std::env::var(var).is_ok_and(|v| !v.trim().is_empty()));
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

/// `ASTER_BIN` always wins. A dev build runs the workspace CLI (never the sidecar,
/// which dev doesn't stage); a release build uses the bundled sidecar, then PATH.
fn resolve_bin() -> String {
    if let Ok(p) = std::env::var("ASTER_BIN") {
        if !p.is_empty() {
            return p;
        }
    }
    if cfg!(debug_assertions) {
        if let Some(p) = workspace_bin("debug").or_else(|| workspace_bin("release")) {
            return p.to_string_lossy().into_owned();
        }
    }
    find_sidecar_bin()
        .or_else(|| workspace_bin("release"))
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| "aster".to_string())
}

/// The bundled Tauri `externalBin`, next to the app executable. Named `aster-cli`,
/// distinct from the `Aster` app binary so the two never collide on a
/// case-insensitive filesystem. `None` in dev, where no sidecar is staged.
fn find_sidecar_bin() -> Option<PathBuf> {
    let dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    let name = if cfg!(windows) {
        "aster-cli.exe"
    } else {
        "aster-cli"
    };
    let cand = dir.join(name);
    cand.is_file().then_some(cand)
}

/// The workspace CLI at `target/<profile>/aster`. Selected by explicit profile,
/// not mtime, so dev always runs current source instead of guessing.
fn workspace_bin(profile: &str) -> Option<PathBuf> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()?
        .parent()?
        .to_path_buf();
    let cand = root.join("target").join(profile).join("aster");
    cand.is_file().then_some(cand)
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

/// Follow a link the agent printed. Opening it in the OS browser rather than
/// the webview: a plain anchor would navigate the app itself away, and there
/// is no way back. Pages only, so a crafted link cannot launch a handler.
#[tauri::command]
fn open_external(url: String) -> Result<(), String> {
    let scheme = url
        .split_once(':')
        .map(|(head, _)| head.to_ascii_lowercase());
    if !matches!(scheme.as_deref(), Some("http" | "https" | "file")) {
        return Err(format!("refusing to open {url}"));
    }
    open::that_detached(&url).map_err(|e| e.to_string())
}

#[tauri::command]
fn startup_info() -> StartupInfo {
    StartupInfo {
        default_repo: default_repo(),
        bin_path: Some(resolve_bin()),
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

/// Tracked and untracked-but-not-ignored files, for @-mentions in the
/// composer. Capped so huge repos stay cheap to ship to the UI.
#[tauri::command]
async fn list_repo_files(repo_path: String) -> Result<Vec<String>, String> {
    let out = Command::new("git")
        .args(["ls-files", "--cached", "--others", "--exclude-standard"])
        .current_dir(&repo_path)
        .env("PATH", augmented_path())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .await
        .map_err(|e| format!("could not run git: {e}"))?;
    if !out.status.success() {
        return Err("not a git repository".into());
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .take(20_000)
        .map(str::to_owned)
        .collect())
}

/// A launched macOS/Linux GUI app inherits a minimal `PATH`, so the CLI's own
/// subprocesses (`git`, `gh`) can go missing even when they work in a shell.
/// Prepend the common install locations so those tools resolve.
fn augmented_path() -> String {
    let existing = std::env::var("PATH").unwrap_or_default();
    let extras = ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/bin"];
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
    let mut builder = tauri::Builder::default().plugin(tauri_plugin_dialog::init());

    #[cfg(desktop)]
    {
        builder = builder
            .plugin(tauri_plugin_process::init())
            .plugin(tauri_plugin_updater::Builder::new().build());
    }

    builder
        .invoke_handler(tauri::generate_handler![
            startup_info,
            pick_repo,
            pick_diff,
            run_review,
            auth_status,
            save_provider,
            chat,
            answer_approval,
            cancel_chat,
            apply_fix,
            list_sessions,
            show_session,
            delete_session,
            memory_list,
            memory_add,
            list_repo_files,
            open_external
        ])
        .run(tauri::generate_context!())
        .expect("error while running aster desktop");
}
