//! A turn's life in a chat: starting it, steering it with messages sent
//! mid-turn, relaying its prompts, finishing it, and the wake-ups that start
//! one on their own.

use std::env;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Result, anyhow};
use tokio::sync::mpsc;
use tokio::task;

use super::ChatKey;
use super::api::Typing;
use super::commands::photo_policy;
use super::state::{Admission, Bridge, Outbox, Pending};
use super::text::{APPROVAL_ASK, approval_text, question_text, shot_paths};
use crate::bridge::{Agent, Answer, Turn, TurnEvent, TurnOutcome};
use crate::channel::{console_text, console_tool, truncate};
use crate::telegram::{Wakeup, is_stop_request, parse_wakeup};

/// Typing shows for a few seconds on each platform, so it is refreshed for as
/// long as the turn runs, and only while nothing is waiting on the person.
const TYPING_EVERY: Duration = Duration::from_secs(8);

const WAKE_POLL: Duration = Duration::from_secs(2);
const ERRAND_CHARS: usize = 90;

const ASIDE_NOTE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../aster-cli/prompts/spectrum-aside.md"
))
.trim_ascii_end();

const SPECTRUM_SYSTEM: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../aster-cli/prompts/spectrum-system.md"
))
.trim_ascii_end();

const REMINDER: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../aster-cli/prompts/spectrum-reminder.md"
))
.trim_ascii_end();

pub(super) fn start_turn(
    bridge: &Arc<Bridge>,
    chat: &ChatKey,
    reply_to: Option<String>,
    prompt: String,
) {
    // Admitted and marked running under one lock, so another message, a
    // wake-up, or a queued run cannot slip in between and start a second turn
    // in this chat. Only the spawns happen under it; the turn runs outside.
    let admission = bridge.chat(chat, |state| {
        let admission = state.admit(reply_to.as_deref(), &prompt);
        if let Admission::Start = admission {
            let turn = Turn {
                bin: bridge.cfg.bin.clone(),
                repo_root: bridge.cfg.repo_root.clone(),
                session: format!("spectrum-{}", chat.slug()),
                mode: state
                    .mode
                    .clone()
                    .unwrap_or_else(|| bridge.cfg.mode.clone()),
                model: None,
                effort: None,
                extra_env: vec![],
            };
            let agent = state.agent.clone().filter(|agent| agent.is_alive());
            let session = state.session.clone();
            eprintln!("[{}] user: {}", chat.slug(), console_text(&prompt, 200));
            let (events_tx, events_rx) = mpsc::channel::<TurnEvent>(8);
            let turn_bridge = Arc::clone(bridge);
            let turn_chat = chat.clone();
            let prompt = prompt.clone();
            let turn_task = tokio::spawn(async move {
                // One agent process per chat, kept across turns; a session saved
                // before a restart is loaded back rather than started over.
                let agent = match agent {
                    Some(agent) => agent,
                    None => {
                        let agent = Agent::spawn(&turn).await?;
                        turn_bridge
                            .chat(&turn_chat, |state| state.agent = Some(Arc::clone(&agent)));
                        agent
                    }
                };
                let session_id = agent
                    .ensure_session(&turn.repo_root, session.as_deref())
                    .await?;
                if session.as_deref() != Some(&session_id) {
                    turn_bridge.chat(&turn_chat, |state| state.session = Some(session_id.clone()));
                    turn_bridge.save_chat(&turn_chat);
                }
                agent.configure(&session_id, &turn).await;
                // The standing rules go in with the first message of a session
                // rather than every one, so the recorded conversation stays
                // conversation.
                let text = if agent.primed() {
                    prompt
                } else {
                    format!("{SPECTRUM_SYSTEM}\n\n{prompt}")
                };
                agent.prompt(&session_id, &text, &events_tx).await
            });
            state.running = Some(turn_task.abort_handle());

            let bridge = Arc::clone(bridge);
            let chat = chat.clone();
            let reply_to = reply_to.clone();
            tokio::spawn(async move {
                let turn = turn_task.id();
                let outbox = bridge.outbox(&chat, reply_to.as_deref(), "turn");
                let result = drive_turn(&bridge, &chat, &outbox, events_rx, turn_task).await;
                finish_turn(&bridge, &chat, turn, &outbox, reply_to.as_deref(), result).await;
                run_queued(&bridge, &chat);
            });
        }
        admission
    });

    match admission {
        Admission::Start | Admission::Queued => {}
        // "Stop" is an order to the bridge, not a note for the agent to weigh
        // against the job it is in the middle of. Everything else joins the
        // turn rather than starting a second conversation about the same work.
        Admission::Busy if is_stop_request(&prompt) => {
            let bridge = Arc::clone(bridge);
            let chat = chat.clone();
            tokio::spawn(async move {
                let outbox = bridge.outbox(&chat, reply_to.as_deref(), "cmd");
                stop_turn(&bridge, &chat, &outbox).await;
            });
        }
        Admission::Busy => steer_running_turn(bridge, chat, reply_to, prompt),
    }
}

/// A message sent mid-turn goes to the turn in flight, so the agent hears it
/// while it works instead of picking it up as a second, contextless task. One
/// that arrives as the turn is ending waits and runs next.
fn steer_running_turn(
    bridge: &Arc<Bridge>,
    chat: &ChatKey,
    reply_to: Option<String>,
    prompt: String,
) {
    let agent = bridge.chat(chat, |state| {
        state.agent.clone().filter(|agent| agent.is_alive())
    });
    let bridge = Arc::clone(bridge);
    let chat = chat.clone();
    tokio::spawn(async move {
        let joined = match agent {
            Some(agent) => agent
                .steer(&format!("{ASIDE_NOTE}\n\n{prompt}"))
                .await
                .unwrap_or(false),
            None => false,
        };
        eprintln!(
            "[{}] {} {}",
            chat.slug(),
            if joined { "mid-turn" } else { "queued" },
            console_text(&prompt, 200)
        );
        if joined {
            return;
        }
        let orphaned = bridge.chat(&chat, |state| {
            state.queued.push((reply_to, prompt));
            state.running.is_none()
        });
        // The turn ended while this was in flight, so its finish already ran
        // whatever was waiting and nothing else will pick this up.
        if orphaned {
            run_queued(&bridge, &chat);
        }
    });
}

/// Whatever came in too late to join the last turn runs now, as one turn.
fn run_queued(bridge: &Arc<Bridge>, chat: &ChatKey) {
    let queued = bridge.chat(chat, |state| std::mem::take(&mut state.queued));
    if let Some((reply_to, prompt)) = merge_queued(queued) {
        start_turn(bridge, chat, reply_to, prompt);
    }
}

/// The waiting messages as one prompt, answered as a reply to the first of
/// them. None when nothing is waiting.
pub(crate) fn merge_queued(
    queued: Vec<(Option<String>, String)>,
) -> Option<(Option<String>, String)> {
    let reply_to = queued.first()?.0.clone();
    let prompt = queued
        .into_iter()
        .map(|(_, text)| text)
        .collect::<Vec<_>>()
        .join("\n\n");
    Some((reply_to, prompt))
}

pub(super) async fn stop_turn(bridge: &Bridge, chat: &ChatKey, outbox: &Outbox) {
    let (queued, agent) = bridge.chat(chat, |state| (state.stop(), state.agent.clone()));
    let Some(queued) = queued else {
        outbox.text("Nothing to stop.").await;
        return;
    };
    // The agent process outlives the turn, so it has to be told as well, or
    // it keeps working on a message nobody is reading.
    if let Some(agent) = agent {
        agent.cancel().await;
    }
    let text = match queued {
        0 => "Stopped.".to_string(),
        1 => "Stopped, and dropped the message waiting behind it.".to_string(),
        n => format!("Stopped, and dropped the {n} messages waiting behind it."),
    };
    outbox.text(&text).await;
}

async fn drive_turn(
    bridge: &Arc<Bridge>,
    chat: &ChatKey,
    outbox: &Outbox,
    mut events: mpsc::Receiver<TurnEvent>,
    turn_task: task::JoinHandle<Result<TurnOutcome>>,
) -> Result<TurnOutcome> {
    let typing = outbox.agent.clone().map(|agent| {
        let bridge = Arc::clone(bridge);
        let chat = chat.clone();
        tokio::spawn(async move {
            loop {
                let waiting = bridge.chat(&chat, |state| state.pending.is_some());
                // A platform without typing says so once; asking again every
                // few seconds would only spend the rate limit.
                if !waiting
                    && bridge
                        .api
                        .typing(&chat.space, &agent, Typing::Started)
                        .await
                        .is_err()
                {
                    break;
                }
                tokio::time::sleep(TYPING_EVERY).await;
            }
        })
    });

    let policy = photo_policy(bridge, chat);
    let mut held_shot: Option<String> = None;
    while let Some(event) = events.recv().await {
        match event {
            TurnEvent::ToolCall {
                name, arguments, ..
            } => {
                eprintln!("[{}] -> {}", chat.slug(), console_tool(&name, &arguments));
            }
            TurnEvent::ToolResult { error, output, .. } => {
                let shots = shot_paths(&output);
                match policy.as_str() {
                    "on" => {
                        for path in shots {
                            outbox.photo(&path).await;
                        }
                    }
                    "off" => {}
                    _ => {
                        if let Some(path) = shots.into_iter().next_back() {
                            held_shot = Some(path);
                        }
                    }
                }
                if error {
                    eprintln!("[{}]   tool failed", chat.slug());
                }
            }
            TurnEvent::Text { .. } | TurnEvent::Thought { .. } => {}
            TurnEvent::ApprovalRequest {
                preview,
                scope,
                respond,
            } => {
                eprintln!(
                    "[{}] approval needed: {}",
                    chat.slug(),
                    console_text(&preview, 120)
                );
                // A reply only counts as the answer once every part of the
                // preview is out, so a yes never comes from someone who has
                // seen part of the request. Pending goes in before the closing
                // ask, so an answer that beats a slow or retried send of it
                // still lands.
                if !outbox
                    .verbatim(&approval_text(&preview, scope.as_deref()))
                    .await
                {
                    let _ = respond.send(Answer::Deny);
                    continue;
                }
                bridge.chat(chat, |state| {
                    state.pending = Some(Pending::Approval(respond))
                });
                if !outbox.verbatim(APPROVAL_ASK).await {
                    withdraw_pending(bridge, chat);
                }
            }
            TurnEvent::Question {
                header,
                question,
                options,
                respond,
            } => {
                eprintln!("[{}] ? {}", chat.slug(), console_text(&question, 120));
                let text = question_text(&header, &question, &options);
                bridge.chat(chat, |state| {
                    state.pending = Some(Pending::Question { options, respond })
                });
                if !outbox.verbatim(&text).await {
                    withdraw_pending(bridge, chat);
                }
            }
        }
    }
    let result = turn_task
        .await
        .unwrap_or_else(|e| Err(anyhow!("turn cancelled: {e}")));
    if let Some(typing) = typing {
        typing.abort();
        if let Some(agent) = &outbox.agent {
            let _ = bridge.api.typing(&chat.space, agent, Typing::Stopped).await;
        }
    }
    // The last screenshot is the evidence the reply leans on, so it goes out
    // however the turn ended.
    if let Some(path) = held_shot {
        outbox.photo(&path).await;
    }
    match &result {
        Ok(outcome) => eprintln!(
            "[{}] done: {}",
            chat.slug(),
            console_text(outcome.reply.trim(), 200)
        ),
        Err(e) => eprintln!("[{}] turn failed: {e:#}", chat.slug()),
    }
    result
}

/// A prompt that never reached the person is answered as a no, so the turn
/// does not wait on it and their next message is not taken as the answer.
fn withdraw_pending(bridge: &Bridge, chat: &ChatKey) {
    match bridge.chat(chat, |state| state.pending.take()) {
        Some(Pending::Approval(respond)) => {
            let _ = respond.send(Answer::Deny);
        }
        Some(Pending::Question { respond, .. }) => {
            let _ = respond.send(None);
        }
        None => {}
    }
}

pub(super) async fn finish_turn(
    bridge: &Bridge,
    chat: &ChatKey,
    turn: task::Id,
    outbox: &Outbox,
    reply_to: Option<&str>,
    result: Result<TurnOutcome>,
) {
    // Only the turn that is still registered clears the chat, so a finish
    // can never wipe out the state of a turn that started after it.
    let (stopped, latest) = bridge.chat(chat, |state| {
        let stopped = if state
            .running
            .as_ref()
            .is_some_and(|running| running.id() == turn)
        {
            state.running = None;
            state.pending = None;
            std::mem::take(&mut state.stopped)
        } else {
            false
        };
        (stopped, state.last_seen.clone())
    });
    // In a two-person chat an answer to the last thing said needs no quote;
    // one that lands after the person has moved on does.
    let quote = reply_to.filter(|id| latest.as_deref() != Some(*id));
    match result {
        Ok(outcome) => outbox.reply(&outcome.reply, quote).await,
        // /stop already confirmed; reporting the abort as a failure on top of
        // it would be a second, noisier message.
        Err(_) if stopped => {}
        Err(e) => {
            outbox
                .reply(
                    &format!("I couldn't finish that. Send it again to retry.\n\n{e:#}"),
                    quote,
                )
                .await
        }
    }
}

/// Watches `<data home>/aster/wakeups/<id>.json`, written by the phone; a
/// reminder becomes one turn in the chat that last spoke, a notice is only
/// posted.
pub(super) async fn watch_wakeups(bridge: Arc<Bridge>) {
    let root = match env::var_os("XDG_DATA_HOME").filter(|dir| !dir.is_empty()) {
        Some(dir) => PathBuf::from(dir),
        None => match env::var_os("HOME") {
            Some(home) => PathBuf::from(home).join(".local/share"),
            None => return,
        },
    };
    let dir = root.join("aster").join("wakeups");
    loop {
        tokio::time::sleep(WAKE_POLL).await;
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut files: Vec<PathBuf> = entries
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
            .collect();
        files.sort();
        for path in files {
            let wakeup = std::fs::read_to_string(&path)
                .ok()
                .and_then(|raw| parse_wakeup(&raw));
            let _ = std::fs::remove_file(&path);
            let Some(wakeup) = wakeup else {
                continue;
            };
            let Some(last) = bridge.latest_chat() else {
                tracing::warn!("wake-up with no chat to land in: {wakeup:?}");
                continue;
            };
            bridge.chat(&last.chat, |state| {
                state.agent_user.get_or_insert(last.agent.clone());
            });
            let outbox = bridge.outbox(&last.chat, None, "wake");
            match wakeup {
                Wakeup::Notice(text) => outbox.text(&text).await,
                Wakeup::Reminder(text) => {
                    // What lands in the chat is the errand in a line, its
                    // first sentence short enough to read at a glance, never
                    // the whole instruction the scheduler wrote to itself.
                    let errand = text
                        .split_once(". ")
                        .map(|(head, _)| head)
                        .unwrap_or(&text)
                        .trim();
                    outbox
                        .text(&format!("⏰ {}", truncate(errand, ERRAND_CHARS)))
                        .await;
                    start_turn(&bridge, &last.chat, None, REMINDER.replace("{text}", &text));
                }
            }
        }
    }
}
