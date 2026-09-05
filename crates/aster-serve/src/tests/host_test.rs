use super::*;

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;

use tokio::sync::broadcast::error::TryRecvError;

fn state(root: PathBuf) -> Arc<AppState> {
    Arc::new(AppState::new(
        root,
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 4187),
        None,
    ))
}

#[tokio::test]
async fn searching_files_answers_the_request_that_asked() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("guard.rs"), "fn main() {}").expect("file");
    let state = state(dir.path().to_path_buf());
    let instance = state.instance_for(&json!({})).await;
    let mut events = instance.events.subscribe();

    handle(
        &state,
        &json!({ "type": "searchFiles", "query": "guard", "requestId": "r1" }),
    )
    .await
    .expect("handled");

    let posted: Value = serde_json::from_str(&events.try_recv().expect("a message")).expect("json");
    assert_eq!(posted["type"], "fileResults");
    assert_eq!(posted["requestId"], "r1");
    assert_eq!(posted["paths"][0], "guard.rs");
}

#[tokio::test]
async fn answering_with_no_turn_running_says_so() {
    let state = state(PathBuf::from("."));
    let answered = handle(&state, &json!({ "type": "approval", "allow": true })).await;
    assert_eq!(answered, Err("no turn is running".to_string()));
}

#[tokio::test]
async fn a_message_only_an_editor_could_serve_is_let_through_quietly() {
    let state = state(PathBuf::from("."));
    let instance = state.instance_for(&json!({})).await;
    let mut events = instance.events.subscribe();
    for message in [
        json!({ "type": "openExternal", "url": "https://example.test" }),
        json!({ "type": "runCommand", "command": "aster.newConversation" }),
        json!({ "type": "somethingNewer" }),
    ] {
        handle(&state, &message).await.expect("handled");
    }
    assert_eq!(events.try_recv().unwrap_err(), TryRecvError::Empty);
}

#[tokio::test]
async fn dropped_paths_come_back_as_mentions() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("notes.md"), "x").expect("file");
    let state = state(dir.path().to_path_buf());
    let instance = state.instance_for(&json!({})).await;
    let mut events = instance.events.subscribe();

    let uri = format!("file://{}/notes.md", dir.path().display());
    handle(&state, &json!({ "type": "dropFiles", "uris": [uri] }))
        .await
        .expect("handled");

    let posted: Value = serde_json::from_str(&events.try_recv().expect("a message")).expect("json");
    assert_eq!(
        posted,
        json!({
            "type": "insertMention",
            "text": "@notes.md",
            "mentions": ["@notes.md"],
        })
    );
}

#[tokio::test]
async fn nothing_droppable_posts_nothing() {
    let state = state(PathBuf::from("."));
    let instance = state.instance_for(&json!({})).await;
    let mut events = instance.events.subscribe();
    handle(
        &state,
        &json!({ "type": "dropFiles", "uris": ["https://example.test/x"] }),
    )
    .await
    .expect("handled");
    assert_eq!(events.try_recv().unwrap_err(), TryRecvError::Empty);
}

#[tokio::test]
async fn run_state_tells_every_tab_that_nothing_is_running() {
    let state = state(PathBuf::from("."));
    let instance = state.instance_for(&json!({})).await;
    let mut events = instance.events.subscribe();
    instance.post_run_state().await;
    let posted: Value = serde_json::from_str(&events.try_recv().expect("a message")).expect("json");
    assert_eq!(
        posted,
        json!({ "type": "runState", "chat": false, "review": false })
    );
}
