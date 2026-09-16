use super::{
    Activity, Api, LearnReport, LearnScore, TurnEnd, Wakeup, extract_gifs, file_url,
    incoming_prompt, is_stop_request, learn_error, link_buttons, mirror_url_for, parse_wakeup,
    photo_file_id, photo_prompt, pick_mirror_ip, plain_text, render_learned, reports_no_change,
    shot_paths, stranger_reply, streams_photos, tool_line, truncate, unwrap_result,
};
use serde_json::json;

/// A card's photo flag, which these tests never exercise.
fn asked() -> std::sync::Arc<std::sync::atomic::AtomicBool> {
    std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false))
}

#[test]
fn extract_gifs_removes_bare_url_lines() {
    let reply = "Here you go!\nhttps://media.giphy.com/media/abc/giphy.gif";
    let (text, gifs) = extract_gifs(reply);
    assert_eq!(text, "Here you go!");
    assert_eq!(gifs, vec!["https://media.giphy.com/media/abc/giphy.gif"]);
}

#[test]
fn extract_gifs_unwraps_markdown_images() {
    let reply = "![party](https://media.tenor.com/xyz/party.gif)";
    let (text, gifs) = extract_gifs(reply);
    assert!(text.is_empty());
    assert_eq!(gifs, vec!["https://media.tenor.com/xyz/party.gif"]);
}

#[test]
fn extract_gifs_keeps_inline_mentions_in_text() {
    let reply = "see https://x.com/a.gif for the vibe";
    let (text, gifs) = extract_gifs(reply);
    assert_eq!(text, reply);
    assert_eq!(gifs, vec!["https://x.com/a.gif"]);
}

#[test]
fn extract_gifs_ignores_plain_replies() {
    let (text, gifs) = extract_gifs("no media here, just https://docs.rs");
    assert_eq!(text, "no media here, just https://docs.rs");
    assert!(gifs.is_empty());
}

#[test]
fn tool_line_labels_known_tools() {
    let line = tool_line("read_file", r#"{"path":"src/main.rs"}"#);
    assert_eq!(line, "📖 <b>Read</b> <code>main.rs</code>");
}

#[test]
fn tool_line_truncates_long_commands() {
    let command = format!(r#"{{"command":"{}"}}"#, "x".repeat(200));
    let line = tool_line("run_command", &command);
    assert!(line.len() < 140);
    assert!(line.contains('…'));
}

#[test]
fn tool_line_escapes_html_in_arguments() {
    let line = tool_line("read_file", r#"{"path":"a<b>.rs"}"#);
    assert!(line.contains("a&lt;b&gt;.rs"));
}

#[test]
fn approval_subject_unwraps_the_run_preview() {
    assert_eq!(super::approval_subject("run `git status`"), "git status");
}

#[test]
fn approval_subject_keeps_trailing_notes() {
    assert_eq!(
        super::approval_subject("run `rm -rf dist` (risky command)"),
        "rm -rf dist (risky command)"
    );
}

#[test]
fn approval_subject_passes_through_edit_previews() {
    assert_eq!(
        super::approval_subject("edit src/lib.rs (protected path)"),
        "edit src/lib.rs (protected path)"
    );
}

#[test]
fn plan_message_marks_the_step_in_flight() {
    let args = r#"{"steps":[
        {"label":"read the code","status":"done"},
        {"label":"write the fix","status":"in_progress"},
        {"label":"run tests","status":"pending"}]}"#;
    let plan = super::plan_message(args).expect("a plan");
    assert!(plan.contains("✅ read the code"));
    assert!(plan.contains("▶️ <b>write the fix</b>"));
    assert!(plan.contains("▫️ run tests"));
    assert!(plan.contains("1/3 done"));
}

#[test]
fn plan_message_is_none_without_steps() {
    assert!(super::plan_message(r#"{"steps":[]}"#).is_none());
    assert!(super::plan_message("not json").is_none());
}

#[test]
fn tool_line_shortens_deep_paths() {
    let line = tool_line(
        "read_file",
        r#"{"path":"crates/aster-policy/src/grants.rs"}"#,
    );
    assert_eq!(line, "📖 <b>Read</b> <code>grants.rs</code>");
}

#[test]
fn tool_line_compresses_regex_alternations() {
    let line = tool_line(
        "search_files",
        r#"{"query":"request_approval|Answer::Always|fn allowed"}"#,
    );
    assert_eq!(
        line,
        "🔎 <b>Search</b> <code>request_approval +2 more</code>"
    );
}

#[test]
fn tool_line_falls_back_to_name() {
    assert_eq!(tool_line("mystery", "{}"), "⚙️ <b>mystery</b>");
}

#[test]
fn truncate_respects_char_boundaries() {
    let text = "é".repeat(50);
    let cut = truncate(&text, 41);
    assert!(cut.ends_with('…'));
    assert!(cut.len() <= 44);
}

#[test]
fn shot_paths_finds_every_screenshot_line() {
    let output = "receipt: ok\nshot /data/cache/screen-1.png (720x720, 44674 bytes)\nstdout: shot /data/cache/screen-2.png (100x100, 9 bytes)\nshot me later\n";
    assert_eq!(
        shot_paths(output),
        vec![
            "/data/cache/screen-1.png".to_string(),
            "/data/cache/screen-2.png".to_string(),
        ]
    );
    assert!(shot_paths("nothing here").is_empty());
}

#[test]
fn plain_text_strips_card_markup() {
    assert_eq!(
        plain_text("🖥 <b>Capture the board</b> <code>asterctl shot &lt;n&gt;</code>"),
        "🖥 Capture the board asterctl shot <n>"
    );
}

#[test]
fn sample_card() {
    let steps: [(&str, &str); 7] = [
        (
            r#"{"command":"asterctl","args":["open","spotify"],"description":"Open Spotify on the phone"}"#,
            "stdout:\nopening Spotify (com.spotify.music)\npkg=com.spotify.music elements=73\n",
        ),
        (
            r#"{"command":"asterctl","args":["wait","2048","20"],"description":"Wait for the 2048 board to load"}"#,
            "stdout:\nfound after 0.0s\npkg=com.android.chrome matches=1 for \"2048\"\n",
        ),
        (
            r#"{"command":"asterctl","args":["ocr"],"description":"Read the game board"}"#,
            "stdout:\nocr blocks=13\n  0 \"10:03 G o\" 57,25-204,49\n",
        ),
        (
            r#"{"command":"asterctl","args":["swipe","500,900","150,900","200"],"description":"Swipe the tiles left"}"#,
            "stdout:\nreceipt: posted (swipe)\nchanged: +14 -1 pkg=com.android.chrome after_ms=1377\n  0 [Button] 16,292-96,364 tap\n",
        ),
        (
            r#"{"command":"asterctl","args":["shot","40,560-680,1180"],"description":"Screenshot the board after the move"}"#,
            "stdout:\nshot /data/user/0/dev.aster.probe/cache/screen-1789074224651.png (640x620, 76722 bytes)\n",
        ),
        (
            r#"{"path":"/data/user/0/dev.aster.probe/cache/screen-1789074224651.png"}"#,
            "/data/data/dev.aster.probe/cache/screen-1789074224651.png is an image; it is attached below",
        ),
        (
            r#"{"command":"asterctl","args":["tap","360,800"],"description":"Tap the middle square"}"#,
            "stdout:\nreceipt: posted (tap at 360,800)\nchanged: +0 -0 pkg=com.android.chrome after_ms=1500\nwarning: nothing on screen changed\n",
        ),
    ];
    let mut activity = Activity::new(Api::new("123:test").unwrap(), 1, None, asked());
    for (i, (args, out)) in steps.iter().enumerate() {
        let tool = if i == 5 { "read_file" } else { "run_command" };
        activity.push(i.to_string(), tool_line(tool, args));
        activity.complete(&i.to_string(), false, out.to_string());
    }
    let text = activity.render("<b>Working…</b>");
    println!("{text}");
}

#[tokio::test]
async fn a_flush_inside_the_rate_gap_is_held_until_the_gap_passes() {
    let mut activity = Activity::new(Api::new("123:test").unwrap(), 1, None, asked());
    activity.push("1".into(), "🔎 <b>Look</b>".into());
    activity.message_id = Some(7);
    activity.last_flush = std::time::Instant::now();
    assert!(activity.due().is_none());

    activity.flush(false).await;
    let due = activity.due().expect("the held edit has a time to go out");
    assert!(due > std::time::Instant::now());
}

#[test]
fn an_incoming_message_carries_its_id_and_the_message_it_quotes() {
    let message = serde_json::json!({
        "message_id": 42,
        "text": "what about this one?",
        "reply_to_message": {
            "message_id": 40,
            "from": { "is_bot": true, "first_name": "Aster" },
            "text": "Two options: A or B."
        }
    });
    assert_eq!(
        incoming_prompt(&message, "what about this one?"),
        "[msg 42, replying to msg 40 from you: \"Two options: A or B.\"]\nwhat about this one?"
    );
    let plain = serde_json::json!({ "message_id": 43, "text": "hi" });
    assert_eq!(incoming_prompt(&plain, "hi"), "[msg 43]\nhi");
}

fn learned(outcome: &str, best: Option<usize>) -> LearnReport {
    LearnReport {
        task: Some("sudoku-speedrun".into()),
        score: LearnScore {
            rounds: 40,
            calls: 61,
        },
        best: best.map(|rounds| LearnScore { rounds, calls: 82 }),
        outcome: outcome.into(),
        reason: None,
    }
}

#[test]
fn a_learned_line_says_what_moved_and_stays_quiet_otherwise() {
    assert_eq!(
        render_learned(&learned("new", None)).unwrap(),
        "📚 <b>sudoku-speedrun</b> · 40 rounds, 61 calls · new skill"
    );
    assert_eq!(
        render_learned(&learned("improved", Some(59))).unwrap(),
        "📚 <b>sudoku-speedrun</b> · 40 rounds, 61 calls · beat 59 · procedure updated"
    );
    assert_eq!(
        render_learned(&learned("regressed", Some(30))).unwrap(),
        "📚 <b>sudoku-speedrun</b> · 40 rounds · best is 30 · lesson noted"
    );
    assert!(render_learned(&learned("skipped", None)).is_none());
    assert!(render_learned(&learned("already_learned", Some(1))).is_none());
    let mut no_task = learned("new", None);
    no_task.task = None;
    assert!(render_learned(&no_task).is_none());
}

fn failed_run(stdout: &str, stderr: &str) -> std::process::Output {
    let mut output = std::process::Command::new("sh")
        .args(["-c", "exit 1"])
        .output()
        .expect("sh runs");
    output.stdout = stdout.as_bytes().to_vec();
    output.stderr = stderr.as_bytes().to_vec();
    output
}

#[test]
fn learn_error_reads_the_json_failure_on_stdout() {
    let run = failed_run(r#"{"ok":false,"error":"401 from the provider"}"#, "");
    assert_eq!(learn_error(&run), "401 from the provider");
}

#[test]
fn learn_error_falls_back_to_stderr_then_exit_status() {
    assert_eq!(learn_error(&failed_run("", "panicked\n")), "panicked");
    assert_eq!(learn_error(&failed_run("", "")), "exit status: 1");
}

#[test]
fn a_url_written_as_code_still_gets_a_button() {
    let reply = "The mirror is live at `http://100.94.136.67:7071` — open it there.";

    let buttons = link_buttons(reply);

    assert_eq!(
        buttons,
        vec![(
            "Open 100.94.136.67:7071".to_string(),
            "http://100.94.136.67:7071".to_string()
        )]
    );
}

#[test]
fn a_labelled_link_keeps_its_label() {
    let reply = "Open the [live screen mirror](http://100.94.136.67:7071) from any device.";

    let buttons = link_buttons(reply);

    assert_eq!(
        buttons,
        vec![(
            "live screen mirror".to_string(),
            "http://100.94.136.67:7071".to_string()
        )]
    );
}

#[test]
fn the_same_link_twice_is_one_button() {
    let reply = "Mirror: http://x.dev:7071\nAgain: http://x.dev:7071";

    assert_eq!(link_buttons(reply).len(), 1);
}

#[test]
fn no_more_buttons_than_the_limit() {
    let reply = "http://a.dev http://b.dev http://c.dev http://d.dev http://e.dev";

    assert_eq!(link_buttons(reply).len(), 3);
}

#[test]
fn a_gif_is_sent_as_an_animation_not_a_button() {
    let reply = "https://media.giphy.com/media/abc/giphy.gif";

    assert!(link_buttons(reply).is_empty());
}

#[test]
fn prose_without_a_link_gets_no_buttons() {
    assert!(link_buttons("Done — the forwarder now survives restarts.").is_empty());
}

#[test]
fn a_photo_message_yields_a_staged_path() {
    let message = json!({
        "photo": [
            {"file_id": "small", "width": 320},
            {"file_id": "large", "width": 1280}
        ]
    });
    let path = photo_file_id(&message).unwrap();
    assert_eq!(path, "large");
}

#[test]
fn a_message_without_a_photo_yields_nothing() {
    assert_eq!(photo_file_id(&json!({"text": "hi"})), None);
}

#[test]
fn stop_on_its_own_ends_the_turn_however_it_is_put() {
    for said in [
        "stop",
        "STOP!",
        "stop stop stop",
        "just stop ffs",
        "please stop it now",
        "cancel",
    ] {
        assert!(is_stop_request(said), "{said} should stop the turn");
    }
}

#[test]
fn a_message_that_wants_something_else_is_for_the_agent() {
    for said in [
        "stop the server and restart it",
        "can you stop using tailwind here",
        "",
        "what are you doing exactly",
        "stop, then tell me what the last commit was",
    ] {
        assert!(!is_stop_request(said), "{said} should reach the agent");
    }
}

#[test]
fn a_file_url_puts_the_path_straight_after_the_token() {
    assert_eq!(
        file_url("https://api.telegram.org/bot123:test", "photos/file_4.jpg"),
        "https://api.telegram.org/file/bot123:test/photos/file_4.jpg",
    );
}

#[test]
fn a_photo_prompt_carries_the_caption() {
    let message = json!({"caption": "what is wrong with this chart"});
    let prompt = photo_prompt("/tmp/x.jpg", &message);
    assert_eq!(prompt, "@/tmp/x.jpg what is wrong with this chart");
}

#[test]
fn a_photo_without_a_caption_still_mentions_the_path() {
    let prompt = photo_prompt("/tmp/x.jpg", &json!({}));
    assert_eq!(prompt, "@/tmp/x.jpg");
}

#[test]
fn the_mirror_url_names_the_phone_not_the_phone_itself() {
    let url = mirror_url_for(Some("192.168.1.20".parse().expect("ip")), 7070);
    assert_eq!(url, "http://192.168.1.20:7070");
}

#[test]
fn the_mirror_url_refuses_loopback_and_falls_back_to_the_host() {
    let url = mirror_url_for(Some("127.0.0.1".parse().expect("ip")), 7070);
    assert!(!url.contains("127.0.0.1"), "{url}");
    assert!(url.starts_with("http://"), "{url}");
}

#[test]
fn the_mirror_url_survives_a_missing_address() {
    let url = mirror_url_for(None, 7070);
    assert!(!url.contains("127.0.0.1"), "{url}");
    assert!(url.starts_with("http://"), "{url}");
}

#[test]
fn the_mirror_url_prefers_tailscale_over_the_lan() {
    let tailscale = Some("100.94.136.67".parse().expect("ip"));
    let lan = Some("192.168.1.20".parse().expect("ip"));
    let url = mirror_url_for(pick_mirror_ip(tailscale, lan), 7070);
    assert_eq!(url, "http://100.94.136.67:7070");
}

#[test]
fn the_mirror_url_falls_back_to_the_lan_without_tailscale() {
    let lan = Some("192.168.1.20".parse().expect("ip"));
    let url = mirror_url_for(pick_mirror_ip(None, lan), 7070);
    assert_eq!(url, "http://192.168.1.20:7070");
}

#[test]
fn a_tap_that_moved_nothing_is_not_a_tick() {
    let mut activity = Activity::new(Api::new("123:test").unwrap(), 1, None, asked());
    activity.push("1".into(), "📱 <b>Tap</b> 15".into());
    activity.complete(
        "1",
        false,
        "receipt: posted (CREATE)\nchanged: +0 -0\nwarning: nothing on screen changed; treat as not done\n".into(),
    );
    activity.push("2".into(), "📱 <b>Tap</b> 16".into());
    activity.complete(
        "2",
        false,
        "receipt: posted (\"SIMs\")\nchanged: +5 -17\n".into(),
    );

    let card = activity.render("<b>Working…</b>");
    assert!(card.contains("⚠️"), "{card}");
    assert!(card.contains("✅"), "{card}");
}

#[test]
fn only_the_phrases_the_tools_print_count_as_no_change() {
    assert!(reports_no_change(
        "changed: +0 -0\nwarning: nothing on screen changed; treat as not done"
    ));
    assert!(reports_no_change(
        "receipt: refused (already at the down limit)"
    ));
    assert!(!reports_no_change(
        "receipt: posted (\"Open\")\nchanged: +4 -8"
    ));
    assert!(!reports_no_change("nothing to commit, working tree clean"));
}

/// The card used to call every screenshot "picture sent" because the tool
/// printed a path. Only the send knows, and it runs before the step completes.
#[test]
fn a_screenshot_step_claims_a_picture_only_when_one_went_out() {
    let shot = "shot /tmp/screen-1.png (1440x3040, 24210 bytes)".to_string();

    let mut held = Activity::new(Api::new("123:test").unwrap(), 1, None, asked());
    held.push("1".into(), "📱 <b>Shot</b>".into());
    held.complete("1", false, shot.clone());
    assert_eq!(held.lines[0].result, "screenshot taken");

    let mut sent = Activity::new(Api::new("123:test").unwrap(), 1, None, asked());
    sent.push("1".into(), "📱 <b>Shot</b>".into());
    sent.mark_sent("1");
    sent.complete("1", false, shot);
    assert_eq!(sent.lines[0].result, "picture sent");
}

#[test]
fn a_stranger_gets_no_answer_at_all() {
    assert_eq!(stranger_reply(&[8504978708], 42), None);
    assert_eq!(stranger_reply(&[1, 2, 3], 0), None);
}

/// Until somebody is allowed, the id is the one thing worth saying: it is how
/// the owner allows themselves.
#[test]
fn an_unclaimed_bot_hands_back_the_id_that_would_claim_it() {
    let reply = stranger_reply(&[], 8504978708).expect("an unclaimed bot answers");
    assert!(reply.contains("8504978708"), "{reply}");
}

/// The guess that used to gate this held every shot of "I want Barilla pasta"
/// and posted one at the end. Nobody is holding the phone: the pictures are
/// the only view of the work, so they go out unless the chat says otherwise.
#[test]
fn screenshots_go_out_as_they_are_taken() {
    assert!(streams_photos(None));
    assert!(streams_photos(Some("auto")));
    assert!(streams_photos(Some("on")));
}

#[test]
fn only_photos_off_holds_them_back() {
    assert!(!streams_photos(Some("off")));
}

/// A photo moves the card below it. The card left above must be remembered
/// so it can go, or every photo leaves another copy of the whole step list.
#[test]
fn a_card_moved_below_a_photo_remembers_the_one_it_replaces() {
    let mut activity = Activity::new(Api::new("123:test").unwrap(), 1, None, asked());
    activity.message_id = Some(7);

    activity.rehome();
    assert_eq!(activity.message_id, None);
    assert_eq!(activity.stale, Some(7));

    activity.rehome();
    assert_eq!(
        activity.stale,
        Some(7),
        "a second move keeps the old card to delete"
    );
}

/// The flood: a card whose id was never read posted a new message on every
/// edit. One failed post must silence the card, not start it over.
#[tokio::test]
async fn a_card_that_cannot_be_posted_is_never_posted_twice() {
    let mut activity = Activity::new(Api::new("123:test").unwrap(), 1, None, asked());
    activity.push("1".into(), "📱 <b>Tap</b>".into());

    activity.flush(true).await;
    assert!(
        activity.message_id.is_none(),
        "the send could not have worked"
    );
    assert!(activity.lost, "a card that cannot post gives up");

    activity.push("2".into(), "📱 <b>Tap</b>".into());
    activity.flush(true).await;
    assert!(activity.lost);
    // And it has nothing to finish, so the answer goes out on its own.
    activity.finish(TurnEnd::Done).await;
    assert!(activity.message_id.is_none(), "it stays quiet to the end");
}

/// `call` hands back the `result` object, not the envelope around it. Reading
/// a level too deep is what cost the card its message id.
/// Every chunk the agent says lands in one accumulated reply, so a turn that
/// narrated four times used to end by repeating all four as a single block.
/// What already went out is counted off; only the tail is still owed.
#[tokio::test]
async fn narration_already_sent_is_not_repeated_at_the_end() {
    let reply = "Let me check what is registered.Search works.Both work, no key needed.";
    let mut activity = Activity::new(Api::new("123:test").unwrap(), 1, None, asked());

    activity.say("Let me check what is registered.");
    activity.say_out().await;
    activity.say("Search works.");
    activity.say_out().await;
    activity.say("Both work, no key needed.");

    assert_eq!(&reply[activity.said..], "Both work, no key needed.");
}

/// Narration between steps used to be a message of its own, so one task was
/// ten notifications on a phone. It is counted off and left on the card, and
/// only the tail is still owed to the final reply.
#[tokio::test]
async fn narration_between_steps_stays_on_the_card() {
    let reply = "Store's open, adding two.Cart's right, checking out.Order's in, ETA 17:40.";
    let mut activity = Activity::new(Api::new("123:test").unwrap(), 1, None, asked());

    activity.say("Store's open, adding two.");
    activity.close_narration();
    activity.push("1".into(), "📱 <b>Tap</b>".into());
    activity.say("Cart's right, checking out.");
    activity.close_narration();
    assert!(activity.saying.is_empty(), "a closed block leaves the card");

    activity.say("Order's in, ETA 17:40.");
    assert_eq!(&reply[activity.said..], "Order's in, ETA 17:40.");
}

/// A model that emits only whitespace between tool calls posted blank lines
/// into the chat. Nothing goes out, but the offset still has to move or the
/// final reply resumes mid-sentence.
#[tokio::test]
async fn blank_narration_posts_nothing_and_still_counts() {
    let mut activity = Activity::new(Api::new("123:test").unwrap(), 1, None, asked());

    activity.say("\n\n");
    activity.say_out().await;

    assert_eq!(activity.said, 2);
    assert!(activity.saying.is_empty());
}

#[test]
fn a_call_returns_the_result_itself() {
    let response = json!({ "ok": true, "result": { "message_id": 7, "text": "hi" } });
    let result = unwrap_result("sendMessage", response).unwrap();
    assert_eq!(result.get("message_id").and_then(|v| v.as_i64()), Some(7));
    assert!(result.get("result").is_none());
}

#[test]
fn a_session_button_marks_the_one_the_chat_is_in() {
    let row = super::SessionRow {
        id: "vsc-mu035c38".into(),
        title: "Fix the thinking toggle".into(),
        turns: 3,
    };
    let label = super::session_label(&row, Some("vsc-mu035c38"));
    assert_eq!(label, "• Fix the thinking toggle · 3 turns");
    let other = super::session_label(&row, Some("vsc-other"));
    assert_eq!(other, "Fix the thinking toggle · 3 turns");
}

#[test]
fn a_session_with_no_title_falls_back_to_its_id() {
    let row = super::SessionRow {
        id: "vsc-mu034gqq".into(),
        title: String::new(),
        turns: 1,
    };
    assert_eq!(
        super::session_label(&row, None),
        "vsc-mu034gqq · 1 turn",
        "one turn is not turns"
    );
}

#[test]
fn every_command_telegram_is_told_about_is_one_it_accepts() {
    for command in super::commands() {
        assert!(
            command.name.len() <= 32
                && command
                    .name
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'),
            "{} is not a name Telegram takes",
            command.name
        );
        assert!(
            command.about.len() <= 256 && !command.about.is_empty(),
            "{} needs a description Telegram takes",
            command.name
        );
    }
    let mut names: Vec<&str> = super::commands().map(|c| c.name).collect();
    let listed = names.len();
    names.sort_unstable();
    names.dedup();
    assert_eq!(listed, names.len(), "a command is in the table twice");
}

#[test]
fn the_phone_build_keeps_the_phone_commands_and_drops_the_repo_ones() {
    let named = |name: &str| super::commands().any(|command| command.name == name);
    assert_eq!(named("mirror"), cfg!(target_os = "android"));
    assert_eq!(named("diff"), !cfg!(target_os = "android"));
    assert_eq!(named("repo"), !cfg!(target_os = "android"));
    assert!(named("bots"), "bots belong on every build");
    assert!(named("sessions") && named("memory") && named("learn"));
}

#[test]
fn a_session_list_flattens_the_title_it_shows() {
    let raw = r#"[{"id":"vsc-mu028ou0","created_at":"2026-09-13T17:00:54Z","model":"glm","turns":2,
        "title":"FIX THIS:\n\n\nwhat causes the repeat"},{"id":"vsc-x","turns":1,"title":""}]"#;
    let rows = super::parse_sessions(raw).expect("the CLI shape reads");
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].title, "FIX THIS: what causes the repeat");
    assert_eq!(rows[0].turns, 2);
    assert!(rows[1].title.is_empty(), "an untitled session keeps its id");
}

#[test]
fn a_session_list_that_is_not_json_is_a_reason_not_a_panic() {
    let err = super::parse_sessions("error: no such repo")
        .err()
        .expect("that is not a list");
    assert!(err.contains("unreadable session list"), "{err}");
}

#[test]
fn memory_shows_the_project_facts_and_the_blocks_by_name() {
    let stored = json!({
        "project": { "text": "# Project memory\n\n- the deploy needs the VPN\n- keys live in .env" },
        "blocks": [{ "name": "release", "description": "how a release goes out" }],
    });
    let text = super::memory_summary(&stored, "").expect("there is memory to show");
    assert!(text.contains("• the deploy needs the VPN"), "{text}");
    assert!(
        !text.contains("# Project memory"),
        "the heading is not a fact"
    );
    assert!(
        text.contains("<b>release</b> how a release goes out"),
        "{text}"
    );
}

#[test]
fn memory_filters_facts_and_blocks_by_what_was_asked_for() {
    let stored = json!({
        "project": { "text": "- the deploy needs the VPN\n- keys live in .env" },
        "blocks": [{ "name": "release", "description": "how a release goes out" }],
    });
    let text = super::memory_summary(&stored, "vpn").expect("one fact matches");
    assert!(text.contains("VPN"), "{text}");
    assert!(!text.contains("keys live"), "the others are left out");
    assert!(!text.contains("release"), "a block that misses is left out");
    assert!(
        super::memory_summary(&stored, "nothing here").is_none(),
        "a filter that matches nothing says so"
    );
}

#[test]
fn memory_with_nothing_in_it_is_none() {
    assert!(super::memory_summary(&json!({}), "").is_none());
    assert!(super::memory_summary(&serde_json::Value::Null, "").is_none());
}

#[test]
fn help_lists_every_command_this_build_offers() {
    let cfg = super::TelegramConfig {
        token: String::new(),
        allowed_users: vec![],
        bin: "aster".into(),
        repo_root: "/Users/me/projects/aster".into(),
        mode: "auto".into(),
    };
    let help = super::help(&cfg);
    assert!(
        help.contains("<code>/Users/me/projects/aster</code>"),
        "{help}"
    );
    for command in super::commands() {
        assert!(
            help.contains(&format!("/{} - {}", command.name, command.about)),
            "/{} is missing from help",
            command.name
        );
    }
    assert!(help.ends_with("Installed skills show up as /commands too."));
    assert!(!help.contains("\n\n\n"), "no gaps between the lines");
}

#[test]
fn a_trimmed_thought_starts_at_a_sentence() {
    // What the card used to show: the tail of the reasoning, cut mid-word.
    let cut = "…o, the sheet doesn't contain Summary. Conclusion: the sheet closed.";
    assert_eq!(super::from_sentence(cut), "Conclusion: the sheet closed.");
    // One whole sentence is left alone.
    let whole = "Checking the payment row now.";
    assert_eq!(super::from_sentence(whole), whole);
}

#[test]
fn a_reminder_reaches_the_chat_as_one_line() {
    let full = "Check the Glovo EatYum order (2x Yum Pasta Lite 2) for an assigned rider. \
                When one is assigned: run sh bin/estate-code.sh with their name, take the \
                gate code from the last line, and send it in the Glovo chat.";
    let line = super::errand(full);
    assert!(line.starts_with("Check the Glovo EatYum order"), "{line}");
    assert!(!line.contains("estate-code.sh"), "{line}");
    assert!(line.chars().count() <= 91, "{}", line.chars().count());
}

/// A notice from the phone is told, not acted on; a file without a kind is a
/// reminder, which is what every file the phone wrote before notices was.
#[test]
fn a_wake_file_is_a_reminder_unless_it_says_notice() {
    assert_eq!(
        parse_wakeup(r#"{"text":"check the install","at":1}"#),
        Some(Wakeup::Reminder("check the install".into()))
    );
    assert_eq!(
        parse_wakeup(r#"{"kind":"notice","text":"🔋 Battery at 20%","at":1}"#),
        Some(Wakeup::Notice("🔋 Battery at 20%".into()))
    );
    assert_eq!(parse_wakeup(r#"{"kind":"notice"}"#), None);
    assert_eq!(parse_wakeup("not json"), None);
}

#[test]
fn alerts_reads_the_phone_settings() {
    let raw = "battery=on levels=20,10\napps=2\n  Google Messages  (com.google.android.apps.messaging)\n  WhatsApp  (com.whatsapp)\n";
    let settings = super::alerts::parse_alerts(raw).unwrap();
    assert!(settings.battery);
    assert_eq!(settings.levels, vec![20, 10]);
    assert_eq!(
        settings.apps,
        vec![
            (
                "Google Messages".to_string(),
                "com.google.android.apps.messaging".to_string()
            ),
            ("WhatsApp".to_string(), "com.whatsapp".to_string()),
        ]
    );
    assert_eq!(
        super::alerts::parse_alerts("error: no app matching \"foo\"; try `apps` to list them\n"),
        Err("no app matching \"foo\"; try `apps` to list them".to_string())
    );
}

/// Every forwarded app gets its own stop button, and the battery button offers
/// the opposite of what is set.
#[test]
fn alerts_card_offers_the_opposite_toggle_and_a_stop_per_app() {
    let settings = super::alerts::AlertSettings {
        battery: false,
        levels: vec![30, 15],
        apps: vec![("WhatsApp".into(), "com.whatsapp".into())],
    };
    let (text, keyboard) = super::alerts::render_alerts(&settings);
    assert!(text.contains("Battery warnings: off"));
    assert!(text.contains("Notifications sent here: WhatsApp"));
    assert_eq!(keyboard[0][0]["callback_data"], "L:b:on");
    assert_eq!(keyboard[1][0]["callback_data"], "L:r:com.whatsapp");
    assert_eq!(keyboard[1][0]["text"], "Stop WhatsApp");

    let on = super::alerts::AlertSettings {
        battery: true,
        ..settings
    };
    assert!(
        super::alerts::render_alerts(&on)
            .0
            .contains("on, at 30% and 15%")
    );
}
