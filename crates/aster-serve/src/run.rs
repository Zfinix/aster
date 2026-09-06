//! The two kinds of run a browser can start: a chat turn and a review. Each
//! owns at most one child, streams its NDJSON stdout to every tab, and answers
//! prompts on the stdin it keeps open.

use std::sync::Arc;

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStderr, ChildStdout};
use tokio::sync::oneshot;

use crate::info;
use crate::state::{AppState, Instance};

/// A live child. Cancelling drops the sender, which the waiter reads as a kill.
pub struct Run {
    stdin: Option<tokio::process::ChildStdin>,
    cancel: Option<oneshot::Sender<()>>,
    pub(crate) id: String,
    pending: Arc<std::sync::Mutex<Option<Value>>>,
}

impl Run {
    pub async fn write(&mut self, line: &str) -> Result<(), String> {
        let stdin = self.stdin.as_mut().ok_or("this turn takes no answers")?;
        stdin
            .write_all(format!("{line}\n").as_bytes())
            .await
            .map_err(|e| format!("could not answer the prompt: {e}"))
    }

    /// The event the child is waiting on, if any.
    pub fn blocked_on(&self) -> Option<Value> {
        self.pending.lock().ok().and_then(|slot| slot.clone())
    }

    pub(crate) fn clear_pending(&self) {
        if let Ok(mut slot) = self.pending.lock() {
            *slot = None;
        }
    }
}

/// Start a chat turn. Returns once the child is up: the turn's own end arrives
/// as a `done` or `error` event, and a child that dies without one is reported
/// as a `chatError`.
pub async fn chat(
    state: &Arc<AppState>,
    instance: &Arc<Instance>,
    id: String,
    message: &Value,
) -> Result<(), String> {
    let settings = state.settings.lock().await;
    let mode = message
        .get("permissionMode")
        .and_then(Value::as_str)
        .unwrap_or(&settings.permission_mode)
        .to_string();
    drop(settings);

    let mut args: Vec<String> = vec![
        "chat".into(),
        "--messages-json".into(),
        "-".into(),
        "--stream".into(),
        "--permission-mode".into(),
        mode,
    ];
    // Only when the browser picked one: the flag outranks ASTER_EFFORT and
    // aster.yaml, so sending a default would silently override the repo's.
    for (flag, value) in [
        ("--effort", message.get("effort")),
        ("--model", message.get("model")),
        ("--session", message.get("session")),
    ] {
        if let Some(value) = value.and_then(Value::as_str).filter(|v| !v.is_empty()) {
            args.push(flag.into());
            args.push(value.into());
        }
    }

    // Claim the slot before spawning: a refused turn must not leave a child
    // behind, and two turns must never share a repo.
    let mut slot = instance.chat.lock().await;
    if slot.is_some() {
        return Err("A turn is already running.".into());
    }
    let argv: Vec<&str> = args.iter().map(String::as_str).collect();
    let mut child = state
        .cli
        .command(&argv)
        .spawn()
        .map_err(|e| format!("could not launch aster: {e}"))?;

    // The messages go in as a single line; stdin then stays open, because that
    // is the channel the CLI reads approval replies from. Any failure before
    // the slot is claimed must take the child with it.
    let mut stdin = match child.stdin.take() {
        Some(stdin) => stdin,
        None => {
            let _ = child.kill().await;
            return Err("aster chat has no stdin".into());
        }
    };
    let messages = message
        .get("messages")
        .cloned()
        .unwrap_or_else(|| json!([]));
    if let Err(e) = stdin.write_all(format!("{messages}\n").as_bytes()).await {
        let _ = child.kill().await;
        return Err(format!("could not send messages to aster: {e}"));
    }

    log(child.stderr.take());
    let pending = Arc::new(std::sync::Mutex::new(None));
    let terminal = stream(instance, child.stdout.take(), {
        let id = id.clone();
        let pending = pending.clone();
        move |event| {
            match event.get("type").and_then(Value::as_str) {
                // Remember what the child is blocked on, so a tab that loads
                // later can be handed the prompt it never saw.
                Some("approval_request" | "question") => {
                    if let Ok(mut slot) = pending.lock() {
                        *slot = Some(event.clone());
                    }
                }
                Some("done" | "error") => {
                    if let Ok(mut slot) = pending.lock() {
                        *slot = None;
                    }
                }
                _ => {}
            }
            let terminal = matches!(
                event.get("type").and_then(Value::as_str),
                Some("done" | "error")
            );
            (
                json!({ "type": "chatEvent", "id": id, "event": event }),
                terminal,
            )
        }
    });

    let (cancel, cancelled) = oneshot::channel();
    *slot = Some(Run {
        stdin: Some(stdin),
        cancel: Some(cancel),
        id: id.clone(),
        pending,
    });
    drop(slot);

    let instance = instance.clone();
    tokio::spawn(async move {
        let code = wait(child, cancelled).await;
        instance.chat.lock().await.take();
        // A turn that ended on its own already said so. One that did not left
        // the UI waiting, so the exit code is the only explanation available.
        if let Some(code) = code
            && !terminal.load(std::sync::atomic::Ordering::SeqCst)
        {
            instance.post(json!({
                "type": "chatError",
                "id": id,
                "message": format!("aster chat exited with code {code}. See the terminal running aster serve."),
            }));
        }
        instance.post_run_state().await;
    });
    Ok(())
}

/// Start a review. Findings stream as they land; the run ends with `reviewDone`
/// or `reviewError`.
pub async fn review(
    state: &Arc<AppState>,
    instance: &Arc<Instance>,
    id: String,
    source: &Value,
) -> Result<(), String> {
    let mut args: Vec<String> = vec!["review".into(), "--stream".into()];
    let kind = source
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or("working");
    if let Some(value) = source.get("value").and_then(Value::as_str)
        && matches!(kind, "range" | "pr")
    {
        args.push(format!("--{kind}"));
        args.push(value.into());
    }

    let mut slot = instance.review.lock().await;
    if slot.is_some() {
        return Err("A review is already running.".into());
    }
    let argv: Vec<&str> = args.iter().map(String::as_str).collect();
    let mut child = state
        .cli
        .command(&argv)
        .spawn()
        .map_err(|e| format!("could not launch aster: {e}"))?;

    // Nothing answers a review, so its stdin is closed at once.
    drop(child.stdin.take());
    log(child.stderr.take());
    stream(instance, child.stdout.take(), {
        let id = id.clone();
        move |event| {
            (
                json!({ "type": "reviewEvent", "id": id, "event": event }),
                false,
            )
        }
    });

    let (cancel, cancelled) = oneshot::channel();
    *slot = Some(Run {
        stdin: None,
        cancel: Some(cancel),
        id: id.clone(),
        pending: Arc::new(std::sync::Mutex::new(None)),
    });
    drop(slot);

    let instance = instance.clone();
    tokio::spawn(async move {
        let code = wait(child, cancelled).await;
        instance.review.lock().await.take();
        match code {
            // Cancelled: the browser asked, so it is not news.
            None => {}
            Some(0) => instance.post(json!({ "type": "reviewDone", "id": id })),
            Some(code) => instance.post(json!({
                "type": "reviewError",
                "id": id,
                "message": format!("aster exited with code {code}. See the terminal running aster serve."),
            })),
        }
        instance.post_run_state().await;
    });
    Ok(())
}

/// Start `aster login <target>` and relay what it prints as `loginOutput`; the
/// end arrives as `loginDone`. The flow opens a browser on this machine, so it
/// serves the local case. A login already running is replaced.
pub async fn login(
    state: &Arc<AppState>,
    instance: &Arc<Instance>,
    target: &str,
) -> Result<(), String> {
    if target.is_empty() {
        return Err("no login target given".into());
    }
    let mut child = state
        .cli
        .command(&["login", target])
        .spawn()
        .map_err(|e| format!("could not launch aster login: {e}"))?;
    drop(child.stdin.take());
    let last = Arc::new(std::sync::Mutex::new(String::new()));
    relay(instance, child.stdout.take(), last.clone());
    relay(instance, child.stderr.take(), last.clone());

    let (cancel, cancelled) = oneshot::channel();
    if let Some(previous) = state.login.lock().await.replace(cancel) {
        let _ = previous.send(());
    }
    let instance = instance.clone();
    let state = state.clone();
    tokio::spawn(async move {
        let Some(code) = wait(child, cancelled).await else {
            return;
        };
        let last = last.lock().map(|l| l.clone()).unwrap_or_default();
        let message = match (code, last.is_empty()) {
            (0, _) => "Signed in.".to_string(),
            (_, false) => last,
            (code, true) => format!("aster login exited with code {code}."),
        };
        // The credentials landed, so a fresh init no longer asks for setup. It
        // goes first, so the tab carries the new model before it acts on the result.
        if code == 0 {
            instance.post(crate::host::init(&state).await);
        }
        instance.post(json!({ "type": "loginDone", "ok": code == 0, "message": message }));
    });
    Ok(())
}

/// Onboarding from the panel: the endpoint is checked with the key it is
/// about to be given before anything is written, then made current. A fresh
/// init follows, which is what turns the card into the greeting.
pub async fn connect(
    state: &Arc<AppState>,
    instance: &Arc<Instance>,
    message: &Value,
) -> Result<(), String> {
    let base_url = message["baseUrl"].as_str().unwrap_or_default().to_string();
    let auth = &message["auth"];
    let failed = |message: String| {
        instance.post(json!({ "type": "connectDone", "ok": false, "message": message }));
    };

    let catalog = info::providers(&state.cli).await;
    let Some(provider) = catalog
        .as_array()
        .and_then(|catalog| catalog.iter().find(|p| p["base_url"] == json!(base_url)))
        .cloned()
    else {
        failed("That provider is not in the catalog.".into());
        return Ok(());
    };
    let model = message["model"]
        .as_str()
        .filter(|m| !m.is_empty())
        .or_else(|| provider["example_model"].as_str())
        .unwrap_or_default()
        .to_string();
    let key_env: Vec<String> = provider["key_env"]
        .as_array()
        .map(|vars| {
            vars.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();

    if auth["kind"] == "login" {
        if let Err(message) = use_provider(&state.cli, &base_url, &model).await {
            failed(message);
            return Ok(());
        }
        return login(state, instance, auth["target"].as_str().unwrap_or_default()).await;
    }

    let key = (auth["kind"] == "key")
        .then(|| {
            auth["value"]
                .as_str()
                .unwrap_or_default()
                .trim()
                .to_string()
        })
        .filter(|key| !key.is_empty());
    if let Err(raw) = probe(&state.cli, &base_url, &key_env, key.as_deref()).await {
        failed(explain(&provider, &base_url, key.is_some(), &raw));
        return Ok(());
    }
    if let Err(message) = use_provider(&state.cli, &base_url, &model).await {
        failed(message);
        return Ok(());
    }
    // A local server gets the placeholder `aster init` would store, but only
    // if the CLI still asks for a key once the endpoint is current.
    let stored = match (key.as_deref(), key_env.first()) {
        (Some(key), Some(var)) => Some((var.as_str(), key)),
        (None, _) if info::setup(&state.cli).await != Value::Null => {
            Some(("ASTER_API_KEY", LOCAL_KEY))
        }
        _ => None,
    };
    if let Some((var, value)) = stored
        && let Err(message) = state
            .cli
            .json_in(&["key", "set", var, "--stdin"], Some(value))
            .await
    {
        failed(message);
        return Ok(());
    }
    instance.post(crate::host::init(state).await);
    instance.post(json!({ "type": "connectDone", "ok": true, "message": "Connected." }));
    Ok(())
}

async fn use_provider(cli: &crate::cli::Cli, base_url: &str, model: &str) -> Result<(), String> {
    let mut args = vec!["provider", "use", base_url];
    if !model.is_empty() {
        args.extend(["--model", model]);
    }
    cli.json(&args).await.map(|_| ())
}

/// What a server on this machine gets instead of a key: the CLI resolves a key
/// for every endpoint, so `aster init` stores this placeholder for local ones.
const LOCAL_KEY: &str = "local";

/// Asks the endpoint for its model list with the key it is about to be given,
/// writing nothing. The key rides in as `ASTER_API_KEY` and as the provider's
/// own vars, since those win and the CLI would otherwise fill them from an
/// older env file.
async fn probe(
    cli: &crate::cli::Cli,
    base_url: &str,
    key_env: &[String],
    key: Option<&str>,
) -> Result<(), String> {
    let value = key.unwrap_or(LOCAL_KEY);
    let mut set = vec![("ASTER_BASE_URL", base_url), ("ASTER_API_KEY", value)];
    set.extend(key_env.iter().map(|var| (var.as_str(), value)));
    let out = cli.run_env(&["models", "--json"], &set, &[]).await?;
    if out.code == 0 {
        return Ok(());
    }
    let parsed: Option<Value> = serde_json::from_str(out.stdout.trim()).ok();
    let error = parsed
        .as_ref()
        .and_then(|p| p["error"].as_str())
        .filter(|e| !e.is_empty())
        .map(str::to_string)
        .or_else(|| Some(out.stderr.trim().to_string()).filter(|e| !e.is_empty()))
        .unwrap_or_else(|| format!("aster exited with code {}", out.code));
    Err(error)
}

fn explain(provider: &Value, base_url: &str, had_key: bool, raw: &str) -> String {
    let name = provider["name"].as_str().unwrap_or(base_url);
    let name = name.split(" (").next().unwrap_or(name);
    let lower = raw.to_lowercase();
    let rejected = [
        "authentication failed",
        "unauthorized",
        "unauthorised",
        "invalid api key",
        "incorrect api key",
        "401",
    ];
    if rejected.iter().any(|needle| lower.contains(needle)) {
        return format!("{name} rejected that key. Check it and try again.");
    }
    let unreachable = [
        "error sending request",
        "connection refused",
        "dns error",
        "could not connect",
        "timed out",
        "no route",
        "network",
    ];
    if unreachable.iter().any(|needle| lower.contains(needle)) {
        return match had_key {
            true => format!("Nothing answered at {base_url}. Check your connection and try again."),
            false => format!("Nothing answered at {base_url}. Start it and try again."),
        };
    }
    raw.lines().next().unwrap_or(raw).to_string()
}

fn relay<R>(instance: &Arc<Instance>, pipe: Option<R>, last: Arc<std::sync::Mutex<String>>)
where
    R: AsyncRead + Unpin + Send + 'static,
{
    let Some(pipe) = pipe else { return };
    let instance = instance.clone();
    tokio::spawn(async move {
        let mut lines = BufReader::new(pipe).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Ok(mut slot) = last.lock() {
                *slot = line.to_string();
            }
            instance.post(json!({ "type": "loginOutput", "line": line }));
        }
    });
}

/// Stop a run. The waiter frees the slot and reports what happened.
pub async fn cancel(run: &mut Option<Run>) {
    if let Some(run) = run.as_mut()
        && let Some(cancel) = run.cancel.take()
    {
        let _ = cancel.send(());
    }
}

fn stream<F>(
    instance: &Arc<Instance>,
    stdout: Option<ChildStdout>,
    wrap: F,
) -> Arc<std::sync::atomic::AtomicBool>
where
    F: Fn(Value) -> (Value, bool) + Send + 'static,
{
    let terminal = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let Some(stdout) = stdout else {
        return terminal;
    };
    let (instance, saw) = (instance.clone(), terminal.clone());
    tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            match serde_json::from_str::<Value>(line) {
                Ok(event) => {
                    let (message, ended) = wrap(event);
                    if ended {
                        saw.store(true, std::sync::atomic::Ordering::SeqCst);
                    }
                    instance.post(message);
                }
                // Not every line a child prints is an event; a stray one is
                // log material, not a reason to tear the stream down.
                Err(_) => tracing::debug!("{line}"),
            }
        }
    });
    terminal
}

fn log(stderr: Option<ChildStderr>) {
    let Some(stderr) = stderr else { return };
    tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if !line.trim().is_empty() {
                eprintln!("{line}");
            }
        }
    });
}

async fn wait(mut child: Child, cancelled: oneshot::Receiver<()>) -> Option<i32> {
    tokio::select! {
        status = child.wait() => status.ok().and_then(|status| status.code()),
        _ = cancelled => {
            let _ = child.kill().await;
            None
        }
    }
}
