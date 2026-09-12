use super::*;

fn aster_bin() -> PathBuf {
    std::env::var_os("ASTER_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/debug/aster")
        })
}

/// A real `aster acp` child, a session, one prompt, and the events it yields.
/// Needs a built binary and a configured provider, so it runs on request:
/// `cargo test -p aster-remote -- --ignored acp_agent`.
#[tokio::test]
#[ignore]
async fn acp_agent_runs_a_turn_and_streams_its_text() {
    let repo_root = std::env::temp_dir().join(format!("aster-bridge-{}", std::process::id()));
    std::fs::create_dir_all(&repo_root).unwrap();
    let turn = Turn {
        bin: aster_bin(),
        repo_root: repo_root.clone(),
        session: "test".into(),
        mode: "auto".into(),
        model: None,
        effort: None,
        extra_env: Vec::new(),
    };
    let agent = Agent::spawn(&turn).await.expect("spawn aster acp");
    let session = agent
        .ensure_session(&repo_root, Some("no-such-session"))
        .await
        .expect("a new session when the wanted one is missing");
    assert!(!session.is_empty());
    agent.configure(&session, &turn).await;

    let (tx, mut rx) = mpsc::channel(32);
    let outcome = agent
        .prompt(&session, "Reply with exactly one word: pong", &tx)
        .await
        .expect("prompt");
    drop(tx);
    let mut saw_text = false;
    while let Some(event) = rx.recv().await {
        if let TurnEvent::Text { .. } = event {
            saw_text = true;
        }
    }
    assert!(saw_text, "no text chunks arrived");
    assert!(
        outcome.reply.to_lowercase().contains("pong"),
        "reply was {:?}",
        outcome.reply
    );

    // The same process answers a second prompt without a new session.
    let (tx, _rx) = mpsc::channel(32);
    let again = agent
        .prompt(&session, "And once more: pong", &tx)
        .await
        .expect("second prompt");
    assert!(again.reply.to_lowercase().contains("pong"));
    assert!(agent.is_alive());
    let _ = std::fs::remove_dir_all(&repo_root);
}

#[test]
fn content_text_reads_blocks_and_lists() {
    assert_eq!(content_text(&json!({ "type": "text", "text": "hi" })), "hi");
    assert_eq!(
        content_text(&json!([
            { "type": "content", "content": { "type": "text", "text": "a" } },
            { "type": "text", "text": "b" }
        ])),
        "ab"
    );
    assert_eq!(content_text(&json!(null)), "");
}

#[test]
fn excerpt_caps_long_output() {
    let long = "x".repeat(OUTPUT_EXCERPT + 5);
    let cut = excerpt(&long);
    assert_eq!(cut.chars().count(), OUTPUT_EXCERPT + 1);
    assert!(cut.ends_with('…'));
    assert_eq!(excerpt("  short  "), "short");
}
