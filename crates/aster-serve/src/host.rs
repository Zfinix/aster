//! The host side of the editor panel's protocol, in a browser. Every message
//! the webview would send an extension arrives here as one POST, and every
//! answer goes back to the tab over the event stream.

use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::Arc;

use axum::Json;
use axum::extract::{Query, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use serde_json::{Value, json};
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::{Stream, StreamExt};

use crate::cli::Cli;
use crate::state::{AppState, Instance};
use crate::{files, info, run, sessions};

pub async fn message(
    State(state): State<Arc<AppState>>,
    Json(message): Json<Value>,
) -> Json<Value> {
    match handle(&state, &message).await {
        Ok(()) => Json(json!({ "ok": true })),
        // The browser fires and forgets, so anything it needs to see has
        // already gone out over the stream; this is for the network tab.
        Err(error) => Json(json!({ "ok": false, "error": error })),
    }
}

/// The live feed. Each tab subscribes to its own instance, so a reloaded tab
/// is caught up by asking for `ready` again and never sees another tab's
/// turns.
pub async fn events(
    State(state): State<Arc<AppState>>,
    Query(query): Query<HashMap<String, String>>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let instance = state
        .instance(
            query
                .get("instance")
                .map(String::as_str)
                .unwrap_or("default"),
        )
        .await;
    // A tab that falls far enough behind to lag the channel has dropped lines
    // either way; skipping them beats tearing the stream down.
    let stream = BroadcastStream::new(instance.events.subscribe())
        .filter_map(|message| message.ok().map(|data| Ok(Event::default().data(data))));
    Sse::new(stream).keep_alive(KeepAlive::default())
}

async fn handle(state: &Arc<AppState>, message: &Value) -> Result<(), String> {
    let instance = state.instance_for(message).await;
    let id = || message["id"].as_str().unwrap_or_default().to_string();
    match message["type"].as_str().unwrap_or_default() {
        "ready" => {
            instance.post(init(state).await);
            instance.post_run_state().await;
        }
        "chat" => {
            if let Err(error) = run::chat(state, &instance, id(), message).await {
                instance.post(json!({ "type": "chatError", "id": id(), "message": error }));
            }
            instance.post_run_state().await;
        }
        "review" => {
            if let Err(error) = run::review(state, &instance, id(), &message["source"]).await {
                instance.post(json!({ "type": "reviewError", "id": id(), "message": error }));
            }
            instance.post_run_state().await;
        }
        // Cancel both: whichever is idle is a no-op, and this removes any
        // dependence on the browser guessing which kind of run is in flight.
        "cancelChat" | "cancelReview" => {
            run::cancel(&mut *instance.chat.lock().await).await;
            run::cancel(&mut *instance.review.lock().await).await;
            instance.post_run_state().await;
        }
        "approval" => {
            let mut line = json!({ "allow": message["allow"].as_bool().unwrap_or(false) });
            if message["always"].as_bool() == Some(true) {
                line["always"] = json!(true);
            }
            instance.answer(line).await?;
        }
        "answer" => {
            instance
                .answer(json!({ "choice": message["choice"] }))
                .await?
        }
        "inject" => {
            instance
                .answer(json!({ "message": message["text"] }))
                .await?
        }
        "setPermissionMode" => {
            let mut settings = state.settings.lock().await;
            if let Some(mode) = message["mode"].as_str() {
                settings.permission_mode = mode.to_string();
                settings.save();
            }
        }
        "setEffort" => {
            let mut settings = state.settings.lock().await;
            settings.effort = message["effort"].as_str().map(str::to_owned);
            settings.save();
        }
        "setModel" => {
            if let Some(model) = message["model"].as_str().filter(|m| !m.is_empty()) {
                // Saved to aster.yaml, so the terminal and the browser agree.
                if let Err(error) = state.cli.json(&["model", "use", model]).await {
                    tracing::warn!(model, error, "could not save the model to the config");
                }
                let vetted = recommended(state).await;
                let vetted: Vec<&str> = vetted.iter().map(String::as_str).collect();
                let mut settings = state.settings.lock().await;
                settings.remember_model(model, &vetted);
                settings.save();
            }
        }
        "searchFiles" => {
            let paths = files::search(&state.cli.root, message["query"].as_str().unwrap_or(""));
            instance.post(json!({
                "type": "fileResults",
                "requestId": message["requestId"],
                "paths": paths,
            }));
        }
        "readFile" => {
            let file = files::preview(
                &state.cli.root,
                message["path"].as_str().unwrap_or_default(),
            );
            instance.post(json!({
                "type": "filePreview",
                "requestId": message["requestId"],
                "file": file,
            }));
        }
        "listSessions" => {
            instance
                .post(json!({ "type": "sessions", "sessions": sessions::list(&state.cli).await }));
        }
        "loadSession" => {
            // Switching sessions abandons the turn in flight; stop it so the
            // loaded session starts clean.
            run::cancel(&mut *instance.chat.lock().await).await;
            let turns =
                sessions::load(&state.cli, message["id"].as_str().unwrap_or_default()).await?;
            instance.post(json!({ "type": "sessionLoaded", "id": id(), "turns": turns }));
        }
        // Both answer with the fresh list, so the browser never has to guess
        // what the store now holds.
        "deleteSession" | "renameSession" => {
            let session = message["id"].as_str().unwrap_or_default();
            let outcome = match message["type"].as_str() {
                Some("deleteSession") => sessions::delete(&state.cli, session).await,
                _ => {
                    sessions::rename(
                        &state.cli,
                        session,
                        message["title"].as_str().unwrap_or_default(),
                    )
                    .await
                }
            };
            instance
                .post(json!({ "type": "sessions", "sessions": sessions::list(&state.cli).await }));
            outcome?;
        }
        "fetchModels" => instance.post(models(state).await),
        "info" => instance.post(card(state, &id(), message["topic"].as_str().unwrap_or("")).await),
        "listMcp" => {
            instance.post(
                json!({ "type": "mcpServers", "servers": info::mcp_servers(&state.cli).await }),
            );
        }
        "toggleMcp" => {
            let outcome = info::toggle_mcp(
                &state.cli,
                message["name"].as_str().unwrap_or_default(),
                message["disabled"].as_bool().unwrap_or(false),
            )
            .await;
            // Re-read rather than assume: the toggle writes a config file, and
            // what landed there is what the next turn will start.
            instance.post(
                json!({ "type": "mcpServers", "servers": info::mcp_servers(&state.cli).await }),
            );
            outcome?;
        }
        "listProviders" => {
            instance.post(json!({
                "type": "providers",
                "providers": info::providers(&state.cli).await,
            }));
        }
        "setProvider" => switch_provider(state, &instance, message).await?,
        "login" => {
            let target = message["target"].as_str().unwrap_or_default();
            run::login(state, &instance, target).await?;
        }
        "compact" => {
            let model = message["model"].as_str().filter(|m| !m.is_empty());
            match info::compact(&state.cli, &message["messages"], model).await {
                Ok(result) => instance.post(json!({
                    "type": "compacted",
                    "id": id(),
                    "summary": result["summary"],
                    "folded": result["folded"],
                    "messages": result["messages"],
                })),
                Err(error) => {
                    instance.post(json!({ "type": "chatError", "id": id(), "message": error }))
                }
            }
        }
        "fixFinding" => {
            let finding = message["finding"].clone();
            let results = fix(state, &json!([finding.clone()])).await;
            if let Some(result) = results.first() {
                instance.post(json!({
                    "type": "fixResult",
                    "finding": finding,
                    "status": result["status"],
                    "reason": result["reason"],
                    "patch": result["patch"],
                }));
            }
        }
        "fixAllFindings" => {
            let findings = message["findings"].clone();
            let results = fix(state, &findings).await;
            let paired: Vec<Value> = findings
                .as_array()
                .unwrap_or(&Vec::new())
                .iter()
                .enumerate()
                .map(|(index, finding)| {
                    let result = results.get(index).cloned().unwrap_or_default();
                    json!({
                        "finding": finding,
                        "status": result["status"],
                        "reason": result["reason"],
                    })
                })
                .collect();
            instance.post(json!({ "type": "fixAllResult", "results": paired }));
        }
        // A drag carries paths; the clipboard rarely does, so a paste arrives
        // as bytes and a name for this side to place.
        "dropFiles" => mention_paths(state, &instance, &message["uris"]),
        "pasteFiles" => stage_files(state, &instance, &message["files"]),
        // No editor to reveal a file in, so it opens the way double-clicking it
        // would. Scoped to the repo: a link in a reply does not get to open
        // whatever it names.
        "openFile" => open_in_repo(state, message["path"].as_str().unwrap_or_default()),
        "openFinding" => open_in_repo(
            state,
            message["finding"]["file_path"].as_str().unwrap_or_default(),
        ),
        // No settings panel in a browser, so settings is the config file
        // itself, opened the way `openFile` opens a repo file.
        "openSettings" => open_settings(state),
        // The page answers `openExternal`, `openUntitled` and `runCommand`
        // itself, since all three are things a browser already does.
        "dismissAnnouncements" => {
            let ids: Vec<String> = message["ids"]
                .as_array()
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| item.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            if !ids.is_empty() {
                let joined = ids.join(",");
                let _ = state
                    .cli
                    .run(&["announce", "--dismiss", &joined], None)
                    .await;
            }
        }
        _ => {}
    }
    Ok(())
}

async fn init(state: &Arc<AppState>) -> Value {
    let root = state.cli.root.clone();
    let settings = state.settings.lock().await.clone();
    let recommended = recommended(state).await;
    // The vetted shortlist, plus anything typed by hand; `fetchModels` fills in
    // the endpoint's own catalog when the picker asks.
    // The model in use comes from the config, the same file the CLI reads.
    let model = state
        .cli
        .json(&["config", "model"])
        .await
        .ok()
        .and_then(|read| read["model"].as_str().map(str::to_owned));
    let mut models = recommended.clone();
    for model in settings
        .custom_models
        .iter()
        .chain(settings.recent_models.iter())
        .chain(model.iter())
    {
        if !models.contains(model) {
            models.push(model.clone());
        }
    }
    json!({
        "type": "init",
        "workspaceRoot": root.display().to_string(),
        "repoName": root.file_name().map(|name| name.to_string_lossy().into_owned()),
        "branch": info::branch(&root).await,
        "model": model,
        "models": models,
        "recommended": recommended,
        "recent": settings.recent_models,
        "contextBudget": info::context_budget(&state.cli).await,
        "permissionMode": settings.permission_mode,
        "effort": settings.effort,
        // The server is the binary, so there is nothing to go missing.
        "binaryOk": true,
        "skills": info::skills(&state.cli).await,
        "setup": info::setup(&state.cli).await,
        "announcements": announcements(&state.cli).await,
    })
}

async fn announcements(cli: &Cli) -> Option<Value> {
    let items = cli
        .json(&["announce"])
        .await
        .ok()
        .and_then(|out| out["items"].as_array().cloned())
        .filter(|items| !items.is_empty())?;
    Some(Value::Array(items))
}

async fn recommended(state: &Arc<AppState>) -> Vec<String> {
    state
        .cli
        .json(&["model", "recommended"])
        .await
        .ok()
        .and_then(|models| serde_json::from_value(models).ok())
        .unwrap_or_default()
}

async fn models(state: &Arc<AppState>) -> Value {
    let out = state.cli.run(&["models", "--json"], None).await;
    let Ok(out) = out else {
        return json!({ "type": "modelsLoaded", "models": [], "error": "could not run aster" });
    };
    match serde_json::from_str::<Value>(out.stdout.trim()) {
        Ok(Value::Array(models)) if out.code == 0 => {
            json!({ "type": "modelsLoaded", "models": models })
        }
        parsed => {
            let error = parsed
                .ok()
                .and_then(|parsed| parsed["error"].as_str().map(str::to_owned))
                .filter(|error| !error.is_empty())
                .unwrap_or_else(|| match out.stderr.trim().is_empty() {
                    true => "This endpoint did not list its models.".to_string(),
                    false => out.stderr.trim().to_string(),
                });
            json!({ "type": "modelsLoaded", "models": [], "error": error })
        }
    }
}

async fn card(state: &Arc<AppState>, id: &str, topic: &str) -> Value {
    let answer = match topic {
        "status" => info::status(&state.cli)
            .await
            .map(|rows| json!({ "title": "Status", "rows": rows })),
        "memory" => info::memory_rows(&state.cli).await.map(|rows| {
            let empty = rows.as_array().is_none_or(Vec::is_empty);
            json!({
                "title": "Memory",
                "rows": rows,
                "note": empty.then_some("Nothing remembered yet."),
            })
        }),
        _ => info::working_diff(&state.cli.root).await.map(|diff| {
            let diff = diff.trim().to_string();
            let empty = diff.is_empty();
            json!({
                "title": "Uncommitted changes",
                "body": (!empty).then_some(diff),
                "lang": "diff",
                "note": empty.then_some("No uncommitted changes."),
            })
        }),
    };
    match answer {
        Ok(mut card) => {
            card["type"] = json!("infoCard");
            card["id"] = json!(id);
            card
        }
        Err(error) => json!({
            "type": "infoCard", "id": id, "title": topic, "note": error, "error": true,
        }),
    }
}

async fn switch_provider(
    state: &Arc<AppState>,
    instance: &Arc<Instance>,
    message: &Value,
) -> Result<(), String> {
    let base_url = message["baseUrl"].as_str().unwrap_or_default().to_string();
    let model = message["model"].as_str().unwrap_or_default().to_string();

    let mut args = vec!["provider", "use", base_url.as_str()];
    if !model.is_empty() {
        args.extend(["--model", model.as_str()]);
    }
    state.cli.json(&args).await?;

    let catalog = info::providers(&state.cli).await;
    let name = catalog
        .as_array()
        .and_then(|catalog| catalog.iter().find(|p| p["base_url"] == json!(base_url)))
        .and_then(|p| p["name"].as_str())
        .unwrap_or(&base_url)
        .to_string();
    let models = info::models_for(&state.cli, &model).await;

    instance.post(json!({
        "type": "providerChanged", "provider": name, "model": model, "models": models,
    }));
    Ok(())
}

async fn fix(state: &Arc<AppState>, findings: &Value) -> Vec<Value> {
    let failed = |reason: String| -> Vec<Value> {
        findings
            .as_array()
            .unwrap_or(&Vec::new())
            .iter()
            .map(|_| json!({ "status": "error", "reason": reason }))
            .collect()
    };
    let out = state
        .cli
        .run(
            &["fix", "--findings-json", "-", "--apply", "--json"],
            Some(&findings.to_string()),
        )
        .await;
    match out {
        Err(error) => failed(error),
        Ok(out) if out.code != 0 => failed(format!("aster fix exited with code {}", out.code)),
        Ok(out) => serde_json::from_str::<Vec<Value>>(out.stdout.trim())
            .unwrap_or_else(|_| failed("unexpected output from aster fix".into())),
    }
}

fn mention_paths(state: &Arc<AppState>, instance: &Arc<Instance>, uris: &Value) {
    let mentions: Vec<String> = uris
        .as_array()
        .unwrap_or(&Vec::new())
        .iter()
        .filter_map(Value::as_str)
        .filter_map(|uri| files::mention(&state.cli.root, uri))
        .map(|path| format!("@{path}"))
        .collect();
    if !mentions.is_empty() {
        instance.post(json!({ "type": "insertMention", "text": mentions.join(" ") }));
    }
}

fn open_settings(state: &Arc<AppState>) {
    let repo = state.cli.root.join("aster.yaml");
    let target = if repo.exists() {
        repo
    } else if let Some(home) = dirs::home_dir() {
        home.join(".aster").join("aster.yaml")
    } else {
        repo
    };
    if let Err(e) = open::that_detached(&target) {
        tracing::warn!("could not open {}: {e}", target.display());
    }
}

fn open_in_repo(state: &Arc<AppState>, path: &str) {
    let target = state.cli.root.join(path);
    let Ok(target) = target.canonicalize() else {
        return;
    };
    if !target.starts_with(&state.cli.root) {
        tracing::warn!("refusing to open {} outside the repo", target.display());
        return;
    }
    if let Err(e) = open::that_detached(&target) {
        tracing::warn!("could not open {}: {e}", target.display());
    }
}

fn stage_files(state: &Arc<AppState>, instance: &Arc<Instance>, files: &Value) {
    let mut mentions = Vec::new();
    for file in files.as_array().unwrap_or(&Vec::new()) {
        let Some(name) = file["name"].as_str() else {
            continue;
        };
        let size = file["size"].as_u64().unwrap_or(0);
        let data = file["data"].as_str().and_then(decode).unwrap_or_default();
        match files::stage(&state.cli.root, name, size, &data) {
            Ok(path) => mentions.push(format!("@{path}")),
            Err(error) => tracing::warn!("{error}"),
        }
    }
    if !mentions.is_empty() {
        instance.post(json!({ "type": "insertMention", "text": mentions.join(" ") }));
    }
}

fn decode(data: &str) -> Option<Vec<u8>> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.decode(data).ok()
}

#[cfg(test)]
#[path = "tests/host_test.rs"]
mod tests;
