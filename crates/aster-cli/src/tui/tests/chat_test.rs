use super::*;

fn chat_app(model: String) -> ChatApp {
    let (tx, _rx) = mpsc::channel(1);
    let (events_tx, _events_rx) = mpsc::channel(1);
    ChatApp::new(
        Mode::Plan,
        Effort::Low,
        false,
        model,
        SessionPermissions {
            plan: sync::Arc::new(Policy::permissive()),
            manual: sync::Arc::new(Policy::permissive()),
            auto: sync::Arc::new(Policy::permissive()),
            edit: sync::Arc::new(Policy::permissive()),
            grants: sync::Arc::new(Grants::default()),
            write_grants: sync::Arc::new(Grants::default()),
            credentials: sync::Arc::new(aster_policy::CommandGrants::default()),
        },
        tx,
        events_tx,
    )
}

fn pane() -> (BottomPane<AppEvent>, mpsc::UnboundedReceiver<AppEvent>) {
    let frames = super::super::terminal::FrameRequester::noop();
    let (tx, rx) = mpsc::unbounded_channel();
    (
        BottomPane::new(
            CHAT_COMMANDS,
            "hint",
            frames,
            tx,
            |answer, scope| AppEvent::ApprovalDecided { answer, scope },
            AppEvent::MentionQueried,
        ),
        rx,
    )
}

fn rendered(app: &ChatApp) -> String {
    app.queue
        .iter()
        .flatten()
        .map(|l| {
            l.spans
                .iter()
                .map(|s| s.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn command_model_switches_client_and_app() {
    let mut client = AiClient::new("http://localhost", "k", "openai/gpt-4o-mini");
    let mut app = chat_app(client.model.clone());
    let (mut p, _rx) = pane();
    app.handle_command("model anthropic/claude-sonnet-5", &mut client, &mut p);
    assert_eq!(client.model, "anthropic/claude-sonnet-5");
    assert_eq!(app.model, "anthropic/claude-sonnet-5");
}

#[test]
fn command_mode_with_name_switches_and_notes_the_model() {
    let mut client = AiClient::new("http://localhost", "k", "m1");
    let mut app = chat_app(client.model.clone());
    let (mut p, _rx) = pane();
    app.handle_command("mode manual", &mut client, &mut p);
    assert_eq!(app.mode, Mode::Manual);
    assert!(app.history.last().is_some_and(is_edit_note));

    app.handle_command("mode edit", &mut client, &mut p);
    assert_eq!(app.mode, Mode::Edit);
    assert_eq!(app.history.iter().filter(|m| is_edit_note(m)).count(), 1);
}

#[test]
fn command_mode_bare_opens_the_picker() {
    let mut client = AiClient::new("http://localhost", "k", "m1");
    let mut app = chat_app(client.model.clone());
    let (mut p, _rx) = pane();
    app.handle_command("mode", &mut client, &mut p);
    assert!(p.has_active_view());
}

#[test]
fn a_locked_run_cannot_leave_plan() {
    let mut client = AiClient::new("http://localhost", "k", "m1");
    let mut app = chat_app(client.model.clone());
    app.edits_locked = true;
    let (mut p, _rx) = pane();
    app.handle_command("mode edit", &mut client, &mut p);
    assert_eq!(app.mode, Mode::Plan);
    assert!(app.flash.unwrap().contains("edits are off"));
}

#[test]
fn mode_change_mid_turn_says_it_waits() {
    let mut client = AiClient::new("http://localhost", "k", "m1");
    let mut app = chat_app(client.model.clone());
    app.thinking = true;
    let (mut p, _rx) = pane();
    app.handle_command("mode manual", &mut client, &mut p);
    assert!(app.flash.unwrap().contains("next message"));
}

#[test]
fn approval_auto_approves_in_edit_mode() {
    let mut app = chat_app("m1".into());
    app.mode = Mode::Edit;
    let (mut p, _rx) = pane();
    let (respond, rx) = tokio::sync::oneshot::channel();
    app.on_approval_request(
        ApprovalRequest {
            markdown: None,
            preview: "edit a.rs".into(),
            scope: None,
            respond,
        },
        &mut p,
    );
    assert!(!p.has_active_view());
    assert_eq!(rx.blocking_recv(), Ok(Answer::Yes));
}

fn plan_request() -> (ApprovalRequest, tokio::sync::oneshot::Receiver<Answer>) {
    let (respond, rx) = tokio::sync::oneshot::channel();
    (
        ApprovalRequest {
            markdown: None,
            preview: "Approve this plan and start editing?\n\n[ ] ship it".into(),
            scope: None,
            respond,
        },
        rx,
    )
}

#[test]
fn a_plan_approval_asks_rather_than_passing_silently() {
    let mut app = chat_app("m1".into());
    app.mode = Mode::Plan;
    let (mut p, _rx) = pane();
    let (req, _answer) = plan_request();
    app.on_plan_approval_request(req, &mut p);
    assert!(p.has_active_view(), "the user must see the plan");
}

/// The turn-local edit gate dies with the turn, so approval has to land on
/// the session or the next message drops back to plan.
#[test]
fn an_approved_plan_promotes_the_session_not_just_the_turn() {
    let mut app = chat_app("m1".into());
    app.mode = Mode::Plan;
    app.edits_locked = false;
    let mut client = AiClient::new("http://localhost", "k", "m1");
    let (mut p, _rx) = pane();

    let (req, _answer) = plan_request();
    app.on_plan_approval_request(req, &mut p);
    app.on_app_event(
        AppEvent::ApprovalDecided {
            answer: Answer::Yes,
            scope: None,
        },
        &mut client,
    );

    assert_eq!(app.mode, Mode::Edit, "the footer and the next turn agree");
    assert!(app.mode.can_edit(), "the next submit() keeps edits on");
}

/// Yolo answered the plan silently before, so the user was never asked and the
/// tool came back with nothing to show for it.
#[test]
fn an_editable_session_is_asked_and_keeps_its_mode() {
    let mut app = chat_app("m1".into());
    app.mode = Mode::Yolo;
    app.edits_locked = false;
    let mut client = AiClient::new("http://localhost", "k", "m1");
    let (mut p, _rx) = pane();

    let (req, _answer) = plan_request();
    app.on_plan_approval_request(req, &mut p);
    assert!(p.has_active_view(), "the user must see the plan");

    app.on_app_event(
        AppEvent::ApprovalDecided {
            answer: Answer::Yes,
            scope: None,
        },
        &mut client,
    );

    assert_eq!(
        app.mode,
        Mode::Yolo,
        "approving a plan never narrows a mode"
    );
}

#[test]
fn a_rejected_plan_leaves_the_session_in_plan() {
    let mut app = chat_app("m1".into());
    app.mode = Mode::Plan;
    let mut client = AiClient::new("http://localhost", "k", "m1");
    let (mut p, _rx) = pane();

    let (req, _answer) = plan_request();
    app.on_plan_approval_request(req, &mut p);
    app.on_app_event(
        AppEvent::ApprovalDecided {
            answer: Answer::No,
            scope: None,
        },
        &mut client,
    );

    assert_eq!(app.mode, Mode::Plan);
}

/// A plain edit approval must not promote; only "always" and plans do.
#[test]
fn a_one_off_edit_approval_does_not_promote_the_session() {
    let mut app = chat_app("m1".into());
    app.mode = Mode::Manual;
    let mut client = AiClient::new("http://localhost", "k", "m1");
    app.on_app_event(
        AppEvent::ApprovalDecided {
            answer: Answer::Yes,
            scope: None,
        },
        &mut client,
    );
    assert_eq!(app.mode, Mode::Manual);
}

#[test]
fn a_locked_run_cannot_be_promoted_by_approving_a_plan() {
    let mut app = chat_app("m1".into());
    app.mode = Mode::Plan;
    app.edits_locked = true;
    let mut client = AiClient::new("http://localhost", "k", "m1");
    let (mut p, _rx) = pane();

    let (req, _answer) = plan_request();
    app.on_plan_approval_request(req, &mut p);
    app.on_app_event(
        AppEvent::ApprovalDecided {
            answer: Answer::Yes,
            scope: None,
        },
        &mut client,
    );

    assert_eq!(app.mode, Mode::Plan, "a read-only run stays read-only");
}

#[test]
fn approval_always_promotes_the_session_to_edit() {
    let mut app = chat_app("m1".into());
    app.mode = Mode::Manual;
    let mut client = AiClient::new("http://localhost", "k", "m1");
    let (mut p, _rx) = pane();
    app.on_app_event(
        AppEvent::ApprovalDecided {
            answer: Answer::Always,
            scope: None,
        },
        &mut client,
    );
    assert_eq!(app.mode, Mode::Edit);

    // The next request needs no prompt at all.
    let (respond, rx) = tokio::sync::oneshot::channel();
    app.on_approval_request(
        ApprovalRequest {
            markdown: None,
            preview: "edit b.rs".into(),
            scope: None,
            respond,
        },
        &mut p,
    );
    assert!(!p.has_active_view());
    assert_eq!(rx.blocking_recv(), Ok(Answer::Yes));
}

#[test]
fn approval_always_stays_locked_when_permissions_deny() {
    let mut app = chat_app("m1".into());
    app.edits_locked = true;
    let mut client = AiClient::new("http://localhost", "k", "m1");
    app.on_app_event(
        AppEvent::ApprovalDecided {
            answer: Answer::Always,
            scope: None,
        },
        &mut client,
    );
    assert_eq!(app.mode, Mode::Plan);
}

#[test]
fn streamed_text_is_emitted_a_line_at_a_time() {
    let mut app = chat_app("m1".into());
    app.on_turn_event(TurnEvent::Token("hello ".into()));
    assert!(app.queue.is_empty(), "an unfinished line stays buffered");
    app.on_turn_event(TurnEvent::Token("there\n".into()));
    assert!(rendered(&app).contains("hello there"));
}

#[test]
fn consecutive_reads_stream_into_one_explored_cell() {
    let mut app = chat_app("m1".into());
    for (i, path) in ["a.rs", "b.rs"].iter().enumerate() {
        let args = format!("{{\"path\":\"{path}\"}}");
        app.on_turn_event(TurnEvent::ToolCall {
            id: i.to_string(),
            name: "read_file".into(),
            args: args.clone(),
        });
        app.on_turn_event(TurnEvent::ToolResult {
            id: i.to_string(),
            result: "contents".into(),
            error: false,
        });
    }
    assert!(
        !app.queue.is_empty(),
        "each read prints as it lands, not once the group closes"
    );

    app.on_turn_event(TurnEvent::Token("done\n".into()));
    let out = rendered(&app);
    assert!(out.contains("Explored"), "{out}");
    assert_eq!(out.matches("Read ").count(), 2);
    assert_eq!(out.matches("Explored").count(), 1);
}

#[test]
fn an_edit_renders_as_a_counted_patch() {
    let mut app = chat_app("m1".into());
    app.on_turn_event(TurnEvent::ToolCall {
        id: "1".into(),
        name: "edit_file".into(),
        args: "{\"path\":\"src/lib.rs\"}".into(),
    });
    app.on_turn_event(TurnEvent::ToolResult {
        id: "1".into(),
        result: "edited src/lib.rs:\n- old\n+ new\n".into(),
        error: false,
    });
    let out = rendered(&app);
    assert!(out.contains("Edited"), "{out}");
    assert!(out.contains("src/lib.rs"));
    assert!(out.contains("+1 −1"), "{out}");
}

#[test]
fn a_swarm_renders_status_lines_then_the_curated_report() {
    let mut app = chat_app("m1".into());
    app.on_turn_event(TurnEvent::ToolCall {
        id: "c1".into(),
        name: "agent".into(),
        args: "{\"tasks\":[{\"agent\":\"explorer\",\"task\":\"find\"}]}".into(),
    });
    app.on_turn_event(TurnEvent::AgentStatus {
        call_id: "c1".into(),
        agent: "explorer".into(),
        status: "running".into(),
        report: None,
        error: None,
        done: 0,
        total: 1,
    });
    app.on_turn_event(TurnEvent::AgentStatus {
        call_id: "c1".into(),
        agent: "explorer".into(),
        status: "done".into(),
        report: Some("Found: auth lives in src/auth.rs".into()),
        error: None,
        done: 1,
        total: 1,
    });
    app.on_turn_event(TurnEvent::ToolResult {
        id: "c1".into(),
        result: "[{\"agent\":\"explorer\",\"report\":\"...\"}]".into(),
        error: false,
    });
    let out = rendered(&app);
    assert!(out.contains("agent explorer: done"), "{out}");
    assert!(out.contains("✔ explorer done"), "{out}");
    assert!(out.contains("Found: auth lives in src/auth.rs"), "{out}");
    assert!(!out.contains("agent ×"), "{out}");
}

#[test]
fn a_failing_tool_shows_its_output_instead_of_being_collapsed() {
    let mut app = chat_app("m1".into());
    app.on_turn_event(TurnEvent::ToolCall {
        id: "1".into(),
        name: "read_file".into(),
        args: "{\"path\":\"missing.rs\"}".into(),
    });
    app.on_turn_event(TurnEvent::ToolResult {
        id: "1".into(),
        result: "no such file".into(),
        error: true,
    });
    assert!(rendered(&app).contains("no such file"));
}

#[test]
fn a_missing_path_is_marked_on_the_explored_line_not_raised_as_an_error() {
    let mut app = chat_app("m1".into());
    app.on_turn_event(TurnEvent::ToolCall {
        id: "1".into(),
        name: "read_file".into(),
        args: "{\"path\":\"crates/ui/src/chat.rs\"}".into(),
    });
    app.on_turn_event(TurnEvent::ToolResult {
        id: "1".into(),
        result: "note: crates/ui/src/chat.rs does not exist. Nearest paths:\n  a.rs".into(),
        error: false,
    });
    app.on_turn_event(TurnEvent::Token("done\n".into()));

    let out = rendered(&app);
    assert!(out.contains("Explored"), "{out}");
    assert!(out.contains("(not found)"), "{out}");
    assert!(!out.contains("Nearest paths"), "{out}");
}

#[test]
fn a_command_step_names_the_command_it_ran() {
    assert_eq!(
        step_label(
            "run_command",
            r#"{"command":"cargo","args":["test","--all"]}"#
        ),
        "Ran cargo test --all"
    );
    assert_eq!(
        step_label("run_command", r#"{"command":"cargo"}"#),
        "Ran cargo"
    );
}

#[test]
fn a_command_step_prefers_the_models_own_summary() {
    assert_eq!(
        step_label(
            "run_command",
            r#"{"command":"bun","args":["run","build"],"description":"Rebuild the webview bundle"}"#
        ),
        "Rebuild the webview bundle"
    );
}

#[test]
fn a_half_streamed_command_label_does_not_invent_a_name() {
    assert_eq!(
        step_label("run_command", r#"{"args":["test"]}"#),
        "Ran a command"
    );
    assert_eq!(
        step_label("run_command", r#"{"command":"car"#),
        "Ran a command"
    );
}

#[test]
fn yolo_asks_before_it_switches() {
    let mut app = chat_app("m1".into());
    app.mode = Mode::Edit;
    let mut client = AiClient::new("http://localhost", "k", "m1");
    let (mut p, _rx) = pane();

    app.handle_command("yolo", &mut client, &mut p);
    assert_eq!(app.mode, Mode::Edit, "asking does not switch");
    assert!(
        p.has_active_view(),
        "the prompt is a pane view, not a flash"
    );
    assert!(app.flash.is_none(), "{:?}", app.flash);

    app.on_app_event(AppEvent::YoloConfirmed, &mut client);
    assert_eq!(app.mode, Mode::Yolo);
    assert!(app.takeover.is_some(), "the switch plays the takeover");

    app.finish_takeover();
    assert!(app.takeover.is_none());
    assert!(
        app.clear_requested,
        "the screen is repainted in the new palette"
    );
    let out = rendered(&app);
    assert!(out.contains("aster"), "the header comes back: {out}");
    assert!(out.contains("YOLO mode ON"), "{out}");

    theme::set(theme::Theme::DEFAULT);
}

#[test]
fn declining_yolo_leaves_the_mode_alone() {
    let mut app = chat_app("m1".into());
    app.mode = Mode::Edit;
    let mut client = AiClient::new("http://localhost", "k", "m1");
    let (mut p, _rx) = pane();

    app.handle_command("yolo", &mut client, &mut p);
    app.on_app_event(AppEvent::SetMode(Mode::Edit), &mut client);
    assert_eq!(app.mode, Mode::Edit);
}

#[test]
fn a_command_shows_its_command_and_then_its_output() {
    let mut app = chat_app("m1".into());
    app.on_turn_event(TurnEvent::ToolCall {
        id: "1".into(),
        name: "run_command".into(),
        args: r#"{"command":"cargo","args":["test"]}"#.into(),
    });
    app.on_turn_event(TurnEvent::ToolResult {
        id: "1".into(),
        result: "test result: ok. 29 passed".into(),
        error: false,
    });

    let out = rendered(&app);
    assert!(out.contains("cargo test"), "the command is named: {out}");
    assert!(out.contains("29 passed"), "the output follows it: {out}");
    assert!(
        !out.contains("Explored"),
        "a command is not collapsed away as exploration: {out}"
    );
}

#[test]
fn a_quiet_endpoint_still_renders_its_reply() {
    let mut app = chat_app("m1".into());
    app.finish_turn("the whole answer", &[], None);
    assert!(rendered(&app).contains("the whole answer"));
}

#[test]
fn blank_only_tokens_render_nothing() {
    let mut app = chat_app("m1".into());
    app.on_turn_event(TurnEvent::Token("\n\n\n".into()));
    assert!(app.queue.is_empty(), "{:?}", rendered(&app));
    app.end_message();
    assert!(app.queue.is_empty(), "{:?}", rendered(&app));
}

#[test]
fn blank_lines_inside_a_message_survive() {
    let mut app = chat_app("m1".into());
    app.on_turn_event(TurnEvent::Token("first\n\nsecond\n".into()));
    let out = rendered(&app);
    assert!(out.contains("first"));
    assert!(out.contains("second"));
    assert_eq!(app.pending_blanks, 0);
}

#[test]
fn trailing_blank_lines_are_dropped_at_message_end() {
    let mut app = chat_app("m1".into());
    app.on_turn_event(TurnEvent::Token("done\n\n\n".into()));
    app.end_message();
    let rows: Vec<Line<'static>> = app.queue.iter().flatten().cloned().collect();
    let last = rows
        .last()
        .map(|l| {
            l.spans
                .iter()
                .map(|s| s.content.as_ref())
                .collect::<String>()
        })
        .unwrap_or_default();
    assert!(last.contains("done"), "{last:?}");
}

#[test]
fn a_streamed_reply_is_not_rendered_twice() {
    let mut app = chat_app("m1".into());
    app.on_turn_event(TurnEvent::Token("the whole answer\n".into()));
    app.finish_turn("the whole answer", &[], None);
    assert_eq!(rendered(&app).matches("the whole answer").count(), 1);
}

#[test]
fn a_failed_turn_drops_the_unanswered_question() {
    let mut app = chat_app("m1".into());
    app.history.push(ChatMessage {
        role: "user".into(),
        content: "hi".into(),
    });
    app.fail_turn("provider is down");
    assert!(app.history.is_empty());
    assert!(rendered(&app).contains("provider is down"));
}

#[test]
fn command_unknown_is_reported() {
    let mut client = AiClient::new("http://localhost", "k", "m1");
    let mut app = chat_app(client.model.clone());
    let (mut p, _rx) = pane();
    app.handle_command("bogus", &mut client, &mut p);
    assert!(rendered(&app).contains("unknown command"));
}

#[test]
fn resume_seeds_history_from_prior_session() {
    let home = tempfile::tempdir().unwrap();
    let store = Store::open(home.path()).unwrap();
    let repo = std::path::Path::new("/tmp/aster-resume-repo");
    {
        let mut w = store.new_session(repo, repo, Some("m".into())).unwrap();
        w.append_message(MessageEvent::user("hello")).unwrap();
        w.append_message(MessageEvent::assistant(Some("hi there".into()), vec![]))
            .unwrap();
    }

    let (recorder, messages) = resume_or_new(&store, repo, &Resume::Latest)
        .unwrap()
        .unwrap();
    assert_eq!(messages.len(), 2);

    let mut app = chat_app("m".into());
    app.store = Some(store);
    app.repo_root = repo.to_path_buf();
    app.recorder = Some(recorder);
    app.load_history(messages);
    assert_eq!(app.history.len(), 2);
    assert!(rendered(&app).contains("hi there"));
}

/// A picker cannot be shown before the UI exists, and opening a transcript
/// early is what leaves the stray empty sessions `--continue` trips over.
#[test]
fn pick_opens_no_session_up_front() {
    let home = tempfile::tempdir().unwrap();
    let store = Store::open(home.path()).unwrap();
    let repo = std::path::Path::new("/tmp/aster-pick-repo");

    assert!(
        resume_or_new(&store, repo, &Resume::Pick)
            .unwrap()
            .is_none()
    );
    assert!(store.list_sessions(repo).unwrap().is_empty());
}

#[test]
fn resume_by_id_reopens_that_session() {
    let home = tempfile::tempdir().unwrap();
    let store = Store::open(home.path()).unwrap();
    let repo = std::path::Path::new("/tmp/aster-by-id-repo");
    let id = {
        let mut w = store.new_session(repo, repo, Some("m".into())).unwrap();
        w.append_message(MessageEvent::user("the first one"))
            .unwrap();
        w.meta().id.clone()
    };
    // A newer session, so "latest" and "this id" disagree.
    {
        let mut w = store.new_session(repo, repo, Some("m".into())).unwrap();
        w.append_message(MessageEvent::user("the second one"))
            .unwrap();
    }

    let (_, messages) = resume_or_new(&store, repo, &Resume::Id(id))
        .unwrap()
        .unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].content.text(), "the first one");
}

#[test]
fn resume_by_unknown_id_is_an_error_not_a_new_session() {
    let home = tempfile::tempdir().unwrap();
    let store = Store::open(home.path()).unwrap();
    let repo = std::path::Path::new("/tmp/aster-bad-id-repo");

    let Err(err) = resume_or_new(&store, repo, &Resume::Id("nope".into())) else {
        panic!("an unknown id cannot be resumed");
    };
    assert!(err.to_string().contains("nope"), "{err}");
    assert!(store.list_sessions(repo).unwrap().is_empty());
}

#[test]
fn the_session_picker_skips_empty_transcripts() {
    let home = tempfile::tempdir().unwrap();
    let store = Store::open(home.path()).unwrap();
    let repo = std::path::Path::new("/tmp/aster-picker-repo");
    store.new_session(repo, repo, Some("m".into())).unwrap();
    {
        let mut w = store.new_session(repo, repo, Some("m".into())).unwrap();
        w.append_message(MessageEvent::user("real work")).unwrap();
    }

    let mut app = chat_app("m".into());
    app.store = Some(store);
    app.repo_root = repo.to_path_buf();
    let (mut p, _rx) = pane();
    app.open_session_picker(&mut p);

    assert!(p.has_active_view(), "the one real session is offered");
    assert!(
        rendered(&app).contains("real work") || !rendered(&app).contains("no saved sessions"),
        "{}",
        rendered(&app)
    );
}

#[test]
fn the_session_picker_says_so_when_there_is_nothing_to_resume() {
    let home = tempfile::tempdir().unwrap();
    let store = Store::open(home.path()).unwrap();
    let repo = std::path::Path::new("/tmp/aster-empty-picker-repo");
    store.new_session(repo, repo, Some("m".into())).unwrap();

    let mut app = chat_app("m".into());
    app.store = Some(store);
    app.repo_root = repo.to_path_buf();
    let (mut p, _rx) = pane();
    app.open_session_picker(&mut p);

    assert!(!p.has_active_view(), "an empty list is not a picker");
    assert!(
        rendered(&app).contains("no saved sessions"),
        "{}",
        rendered(&app)
    );
}

#[test]
fn picking_a_session_seeds_its_history_and_reopens_its_transcript() {
    let home = tempfile::tempdir().unwrap();
    let store = Store::open(home.path()).unwrap();
    let repo = std::path::Path::new("/tmp/aster-adopt-repo");
    let id = {
        let mut w = store.new_session(repo, repo, Some("m".into())).unwrap();
        w.append_message(MessageEvent::user("earlier question"))
            .unwrap();
        w.append_message(MessageEvent::assistant(
            Some("earlier answer".into()),
            vec![],
        ))
        .unwrap();
        w.meta().id.clone()
    };

    let mut app = chat_app("m".into());
    app.store = Some(store);
    app.repo_root = repo.to_path_buf();
    let mut client = AiClient::new("http://localhost", "k", "m");
    app.on_app_event(AppEvent::SessionPicked(id), &mut client);

    assert_eq!(app.history.len(), 2);
    assert!(app.recorder.is_some(), "later turns append to that session");
    assert!(
        rendered(&app).contains("earlier answer"),
        "{}",
        rendered(&app)
    );
}

#[test]
fn record_user_persists_turn() {
    let home = tempfile::tempdir().unwrap();
    let store = Store::open(home.path()).unwrap();
    let repo = std::path::Path::new("/tmp/aster-record-repo");

    let mut app = chat_app("m".into());
    app.store = Some(store.clone());
    app.repo_root = repo.to_path_buf();
    app.start_new_session();
    app.record_user("remember me");

    let latest = store.latest(repo).unwrap().unwrap();
    let persisted = latest.events.iter().any(|e| {
        matches!(e, aster_persist::TranscriptEvent::Message(m)
            if m.role == "user" && m.content.as_deref() == Some("remember me"))
    });
    assert!(persisted);
}

#[test]
fn a_submit_before_mcp_connects_is_held_not_run() {
    let client = AiClient::new("http://localhost", "k", "m1");
    let mut app = chat_app(client.model.clone());
    app.mcp_pending = true;

    let turn = app.submit_or_hold("go", &[], &client, std::path::Path::new("/tmp"));

    assert!(turn.is_none());
    assert_eq!(
        app.held_submit.as_ref().map(|(t, _)| t.as_str()),
        Some("go")
    );
    assert!(app.history.is_empty());
}

#[tokio::test]
async fn a_submit_after_mcp_connects_runs_straight_away() {
    let client = AiClient::new("http://localhost", "k", "m1");
    let mut app = chat_app(client.model.clone());

    let turn = app.submit_or_hold("go", &[], &client, std::path::Path::new("/tmp"));

    assert!(turn.is_some());
    assert!(app.held_submit.is_none());
    assert!(
        app.history
            .iter()
            .any(|m| m.role == "user" && m.content.text() == "go")
    );
    turn.unwrap().abort();
}
