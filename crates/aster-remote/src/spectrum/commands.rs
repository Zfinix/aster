//! Slash commands a chat can send instead of a message for the agent.

use std::sync::Arc;

use super::ChatKey;
use super::state::{Bridge, ChatState};
use super::turn::{start_turn, stop_turn};
use crate::channel::{MODES, skill_prompt, workspace_name};

/// What happens to screenshots: `end` sends the last one with the reply, `on`
/// sends each as it is taken, `off` keeps them all back.
const PHOTOS: &[&str] = &["end", "on", "off"];

pub(super) async fn handle_command(
    bridge: &Arc<Bridge>,
    chat: &ChatKey,
    id: &str,
    name: &str,
    arg: &str,
) {
    let outbox = bridge.outbox(chat, Some(id), "cmd");
    match name {
        "start" | "help" => {
            let text = format!(
                "Aster remote control over chat.\n\
                 Send a message to run the agent on {}.\n\
                 Approvals arrive as questions; answer yes, always, or no.\n\n\
                 /new starts a fresh conversation\n\
                 /stop cancels the running task\n\
                 /mode sets how the agent acts (plan, manual, auto, edit, yolo)\n\
                 /photos sets which screenshots come back (end, on, off)\n\
                 /status shows the mode and workspace\n\
                 /skills lists installed skills\n\
                 /help shows this message",
                workspace_name(&bridge.cfg.repo_root)
            );
            outbox.text(&text).await;
        }
        "new" | "clear" => {
            let agent = bridge.chat(chat, |state| {
                state.session = None;
                state.agent.clone()
            });
            if let Some(agent) = agent {
                agent.reset_session();
            }
            bridge.save_chat(chat);
            outbox.text("Started a fresh conversation.").await;
        }
        "stop" => stop_turn(bridge, chat, &outbox).await,
        "mode" => {
            let reply = set_choice(bridge, chat, arg, MODES, |state| &mut state.mode)
                .unwrap_or_else(|| {
                    let current = bridge
                        .chat(chat, |state| state.mode.clone())
                        .unwrap_or_else(|| bridge.cfg.mode.clone());
                    format!(
                        "Mode is {current}. Reply /mode with one of: {}.",
                        MODES.join(", ")
                    )
                });
            outbox.text(&reply).await;
        }
        "photos" => {
            let reply = set_choice(bridge, chat, arg, PHOTOS, |state| &mut state.photos)
                .unwrap_or_else(|| {
                    let current = photo_policy(bridge, chat);
                    format!(
                        "Screenshots are {current}. Reply /photos end to get the last one with each reply, on for every one, or off for none."
                    )
                });
            outbox.text(&reply).await;
        }
        "status" => {
            let (mode, busy) =
                bridge.chat(chat, |state| (state.mode.clone(), state.running.is_some()));
            let text = format!(
                "Workspace: {}\nMode: {}\nScreenshots: {}\nState: {}",
                workspace_name(&bridge.cfg.repo_root),
                mode.unwrap_or_else(|| bridge.cfg.mode.clone()),
                photo_policy(bridge, chat),
                if busy { "working" } else { "idle" }
            );
            outbox.text(&text).await;
        }
        "skills" => {
            let mut names: Vec<&String> = bridge.skills.keys().collect();
            names.sort();
            let text = if names.is_empty() {
                "No skills are installed.".to_string()
            } else {
                let names: Vec<String> = names.iter().map(|name| format!("/{name}")).collect();
                format!("Installed skills: {}", names.join(", "))
            };
            outbox.text(&text).await;
        }
        other => match bridge.skills.get(other) {
            Some(skill) => start_turn(bridge, chat, Some(id.to_string()), skill_prompt(skill, arg)),
            None => {
                outbox
                    .text("Unknown command. /help lists what I know.")
                    .await
            }
        },
    }
}

/// Set one of a chat's saved choices. None when no value was given, so the
/// caller can say what it is now instead.
fn set_choice(
    bridge: &Bridge,
    chat: &ChatKey,
    arg: &str,
    allowed: &[&str],
    slot: impl FnOnce(&mut ChatState) -> &mut Option<String>,
) -> Option<String> {
    let arg = arg.trim().to_lowercase();
    if arg.is_empty() {
        return None;
    }
    if !allowed.contains(&arg.as_str()) {
        return Some(format!("Expected one of: {}.", allowed.join(", ")));
    }
    bridge.chat(chat, |state| *slot(state) = Some(arg.clone()));
    bridge.save_chat(chat);
    Some(format!("Set to {arg}."))
}

pub(super) fn photo_policy(bridge: &Bridge, chat: &ChatKey) -> String {
    bridge
        .chat(chat, |state| state.photos.clone())
        .unwrap_or_else(|| PHOTOS[0].to_string())
}
