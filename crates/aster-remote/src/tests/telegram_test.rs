use super::{
    Activity, Api, LearnReport, LearnScore, extract_gifs, incoming_prompt, learn_error,
    link_buttons, mirror_url_for, photo_file_id, photo_prompt, pick_mirror_ip, plain_text,
    render_learned, shot_paths, tool_line, truncate,
};
use serde_json::json;

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
    let mut activity = Activity::new(Api::new("123:test").unwrap(), 1, 1);
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
    let mut activity = Activity::new(Api::new("123:test").unwrap(), 1, 1);
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
