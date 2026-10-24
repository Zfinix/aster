use serde_json::{Value, json};

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::{
    Admission, Api, ApiError, Bridge, CHUNK_LIMIT, CURSOR_OVERLAP_MS, ChatKey, ChatState, Cursor,
    Inbound, LastChat, Part, SpectrumConfig, approval_text, dispatch, extension_for, format_time,
    is_allowed, merge_queued, normalize_handle, page_ids, parse_approval, parse_inbound,
    parse_time, plain_chunks, problem_code, question_text, related_body, resolve_choice,
    retry_after, shot_paths, stranger_reply, verbatim_chunks,
};
use crate::bridge::Answer;

const AGENT: &str = "pho_usr_00000000000000000000000001";
const HUMAN: &str = "pho_usr_00000000000000000000000002";
const SPACE: &str = "pho_spc_00000000000000000000000001";
const PROJECT: &str = "pho_prj_00000000000000000000000001";

fn user(id: &str, handle: &str, kind: &str, platform: &str) -> Value {
    json!({
        "details": {},
        "handle": handle,
        "id": id,
        "kind": kind,
        "metadata": {},
        "platform": platform,
        "projectId": PROJECT,
    })
}

/// A `ReceivedImessageMessage` (or its WhatsApp or Telegram twin) shaped like
/// the OpenAPI spec's own message examples.
fn received(platform: &str, content: Value) -> Value {
    json!({
        "content": content,
        "createdAt": "2026-10-22T18:00:00.250Z",
        "id": "pho_msg_00000000000000000000000001",
        "metadata": { "displayName": "Ada" },
        "platform": platform,
        "projectId": PROJECT,
        "receivedBy": user(AGENT, "+14155550100", "agent", platform),
        "sentBy": user(HUMAN, "+14155550123", "human", platform),
        "space": {
            "details": {},
            "id": SPACE,
            "kind": "direct",
            "metadata": {},
            "participantIds": [AGENT, HUMAN],
            "platform": platform,
            "projectId": PROJECT,
        },
        "status": "received",
        "timestamps": { "receivedAt": "2026-10-22T18:00:00.000Z" },
        "updatedAt": "2026-10-22T18:00:00.250Z",
    })
}

fn text(body: &str) -> Value {
    json!({ "type": "text", "format": "plain", "text": body })
}

fn all() -> Vec<String> {
    vec!["imessage".into(), "whatsapp".into(), "telegram".into()]
}

fn chat() -> ChatKey {
    ChatKey {
        platform: "imessage".into(),
        space: SPACE.into(),
    }
}

#[test]
fn parse_inbound_reads_a_direct_text() {
    let inbound = parse_inbound(&received("imessage", text("  ping  ")), &all()).unwrap();
    assert_eq!(
        inbound,
        Inbound {
            id: "pho_msg_00000000000000000000000001".into(),
            chat: chat(),
            agent: AGENT.into(),
            sender: "+14155550123".into(),
            parts: vec![Part::Text("ping".into())],
        }
    );
}

#[test]
fn parse_inbound_reads_an_attachment() {
    let content =
        json!({ "type": "attachment", "attachmentId": "pho_att_00000000000000000000000001" });
    let inbound = parse_inbound(&received("whatsapp", content), &all()).unwrap();
    assert_eq!(
        inbound.parts,
        vec![Part::Attachment(
            "pho_att_00000000000000000000000001".into()
        )]
    );
    assert_eq!(inbound.chat.platform, "whatsapp");
}

#[test]
fn parse_inbound_flattens_a_group_in_order() {
    let content = json!({
        "type": "group",
        "parts": [
            { "type": "attachment", "attachmentId": "pho_att_00000000000000000000000001" },
            text("what is this?"),
            { "type": "message.read" },
        ],
    });
    let inbound = parse_inbound(&received("imessage", content), &all()).unwrap();
    assert_eq!(
        inbound.parts,
        vec![
            Part::Attachment("pho_att_00000000000000000000000001".into()),
            Part::Text("what is this?".into()),
        ]
    );
}

#[test]
fn parse_inbound_skips_receipts_and_typing() {
    for content in [
        json!({ "type": "message.read" }),
        json!({ "type": "space.typing.update", "typing": true }),
        text("   "),
    ] {
        assert!(parse_inbound(&received("imessage", content), &all()).is_none());
    }
}

#[test]
fn parse_inbound_skips_the_agents_own_sends() {
    let mut message = received("imessage", text("hello"));
    message["status"] = json!("dispatched");
    assert!(parse_inbound(&message, &all()).is_none());

    let mut message = received("imessage", text("hello"));
    message["sentBy"]["kind"] = json!("agent");
    assert!(parse_inbound(&message, &all()).is_none());

    let mut message = received("imessage", text("hello"));
    message["sentBy"]["id"] = json!(AGENT);
    assert!(parse_inbound(&message, &all()).is_none());
}

#[test]
fn parse_inbound_skips_group_chats() {
    let mut message = received("telegram", text("hello"));
    message["space"]["kind"] = json!("group");
    assert!(parse_inbound(&message, &all()).is_none());
}

#[test]
fn parse_inbound_needs_a_receiving_agent() {
    let mut message = received("imessage", text("hello"));
    message.as_object_mut().unwrap().remove("receivedBy");
    assert!(parse_inbound(&message, &all()).is_none());

    let mut message = received("imessage", text("hello"));
    message["receivedBy"]["kind"] = json!("human");
    assert!(parse_inbound(&message, &all()).is_none());
}

#[test]
fn parse_inbound_skips_platforms_not_answered() {
    let message = received("sms", text("hello"));
    assert!(parse_inbound(&message, &all()).is_none());
    assert!(parse_inbound(&message, &["sms".to_string()]).is_some());
}

#[test]
fn chat_key_keeps_platforms_apart() {
    let imessage = parse_inbound(&received("imessage", text("a")), &all()).unwrap();
    let telegram = parse_inbound(&received("telegram", text("a")), &all()).unwrap();
    assert_ne!(imessage.chat, telegram.chat);
    assert_ne!(imessage.chat.slug(), telegram.chat.slug());
}

#[test]
fn chat_key_slug_cannot_climb_out() {
    let key = ChatKey {
        platform: "imessage".into(),
        space: "../../etc/passwd".into(),
    };
    let slug = key.slug();
    assert!(!slug.contains('/') && !slug.contains(".."), "{slug}");
    assert_eq!(chat().slug(), format!("imessage-{SPACE}"));
}

#[test]
fn is_allowed_matches_phone_numbers_by_digits() {
    let allowed = vec!["+1 (415) 555-0123".to_string()];
    assert!(is_allowed(&allowed, "imessage", "+14155550123"));
    assert!(is_allowed(&allowed, "whatsapp", "14155550123"));
    assert!(!is_allowed(&allowed, "imessage", "+14155550124"));
}

#[test]
fn is_allowed_matches_emails_without_case() {
    let allowed = vec!["Ada@Example.com".to_string()];
    assert!(is_allowed(&allowed, "imessage", "ada@example.com"));
}

#[test]
fn is_allowed_honours_a_platform_scope() {
    let allowed = vec!["telegram:123456".to_string()];
    assert!(is_allowed(&allowed, "telegram", "123456"));
    assert!(!is_allowed(&allowed, "whatsapp", "123456"));
}

#[test]
fn is_allowed_denies_by_default() {
    assert!(!is_allowed(&[], "imessage", "+14155550123"));
    assert!(!is_allowed(&["".to_string()], "imessage", ""));
}

#[test]
fn is_allowed_keeps_telegram_chat_handles_whole() {
    assert_eq!(normalize_handle("chat:-100123"), "chat:-100123");
    assert!(is_allowed(
        &["chat:-100123".to_string()],
        "telegram",
        "chat:-100123"
    ));
}

#[test]
fn stranger_reply_only_while_nobody_is_allowed() {
    let reply = stranger_reply(&[], "+14155550123").unwrap();
    assert!(reply.contains("--sender +14155550123"), "{reply}");
    assert!(stranger_reply(&["+1".to_string()], "+14155550123").is_none());
}

#[test]
fn time_round_trips_in_utc() {
    let ms = parse_time("2026-10-22T18:00:00.250Z").unwrap();
    assert_eq!(format_time(ms), "2026-10-22T18:00:00.250Z");
    assert_eq!(parse_time("1970-01-01T00:00:00Z"), Some(0));
    assert_eq!(format_time(0), "1970-01-01T00:00:00.000Z");
}

#[test]
fn time_applies_offsets_and_fractions() {
    let utc = parse_time("2026-10-22T18:00:00Z").unwrap();
    assert_eq!(parse_time("2026-10-22T19:30:00+01:30"), Some(utc));
    assert_eq!(parse_time("2026-10-22T13:00:00-05:00"), Some(utc));
    assert_eq!(parse_time("2026-10-22T18:00:00.5Z"), Some(utc + 500));
    assert_eq!(parse_time("2026-10-22T18:00:00.123456Z"), Some(utc + 123));
}

#[test]
fn time_handles_leap_days() {
    let leap = parse_time("2024-02-29T00:00:00Z").unwrap();
    assert_eq!(format_time(leap), "2024-02-29T00:00:00.000Z");
    assert_eq!(
        parse_time("2024-03-01T00:00:00Z").unwrap() - leap,
        86_400_000
    );
}

#[test]
fn time_rejects_what_is_not_a_timestamp() {
    for bad in [
        "",
        "yesterday",
        "2026-13-01T00:00:00Z",
        "2026-10-22T25:00:00Z",
        "2026-10-22T18:00:00.ab",
    ] {
        assert!(parse_time(bad).is_none(), "{bad}");
    }
}

#[test]
fn cursor_admits_each_id_once() {
    let mut cursor = Cursor::default();
    assert!(cursor.admit("a", 1_000));
    assert!(!cursor.admit("a", 1_000));
    assert!(cursor.admit("b", 2_000));
    assert_eq!(cursor.since, 2_000);
}

#[test]
fn cursor_reads_back_over_the_overlap() {
    let mut cursor = Cursor::default();
    let at = parse_time("2026-10-22T18:00:00Z").unwrap();
    cursor.admit("a", at);
    assert_eq!(cursor.query_since(), "2026-10-22T17:59:00.000Z");
}

#[test]
fn cursor_never_queries_before_the_epoch() {
    assert_eq!(Cursor::default().query_since(), "1970-01-01T00:00:00.000Z");
}

#[test]
fn cursor_forgets_ids_older_than_the_overlap() {
    let mut cursor = Cursor::default();
    cursor.admit("old", 0);
    cursor.admit("new", CURSOR_OVERLAP_MS + 1);
    assert!(!cursor.seen.contains_key("old"));
    assert!(cursor.seen.contains_key("new"));
}

#[test]
fn cursor_late_message_is_still_admitted() {
    let mut cursor = Cursor::default();
    cursor.admit("newer", 10_000);
    assert!(cursor.admit("older", 9_000));
    assert_eq!(cursor.since, 10_000);
}

#[test]
fn cursor_survives_a_save_and_load() {
    let mut cursor = Cursor::default();
    cursor.admit("pho_msg_a", parse_time("2026-10-22T18:00:00.250Z").unwrap());
    cursor.admit("pho_msg_b", parse_time("2026-10-22T18:00:01Z").unwrap());
    cursor.last = Some(LastChat {
        chat: chat(),
        agent: AGENT.into(),
    });
    let saved: Value = serde_json::from_str(&cursor.to_json().to_string()).unwrap();
    assert_eq!(Cursor::from_json(&saved), Some(cursor));
}

#[test]
fn cursor_rejects_a_damaged_file() {
    assert!(Cursor::from_json(&json!({ "since": "not a time" })).is_none());
    assert!(Cursor::from_json(&json!({})).is_none());
}

#[test]
fn page_ids_reads_ids_and_times() {
    let page = json!({
        "items": [received("imessage", text("a")), { "id": "broken" }],
        "nextPageToken": "abc.def",
    });
    assert_eq!(
        page_ids(&page),
        vec![(
            "pho_msg_00000000000000000000000001".to_string(),
            parse_time("2026-10-22T18:00:00.250Z").unwrap()
        )]
    );
}

#[test]
fn plain_chunks_respect_the_limit_and_keep_every_line() {
    let text = "one\ntwo\nthree";
    let chunks = plain_chunks(text, 8);
    assert_eq!(chunks.join("\n"), text);
    assert!(chunks.iter().all(|c| c.len() <= 8));
}

#[test]
fn plain_chunks_cut_a_long_line_at_a_space() {
    let chunks = plain_chunks("alpha beta gamma delta", 11);
    assert_eq!(chunks, vec!["alpha beta", "gamma delta"]);
}

#[test]
fn plain_chunks_cut_on_char_boundaries() {
    let text = "é".repeat(10);
    let chunks = plain_chunks(&text, 5);
    assert!(chunks.iter().all(|c| c.len() <= 5));
    assert_eq!(chunks.concat(), text);
}

#[test]
fn plain_chunks_drop_markdown_markers() {
    let chunks = plain_chunks("## Done\n**All** set.\n```\nls\n```", 3500);
    assert_eq!(chunks, vec!["Done\nAll set.\nls"]);
}

#[test]
fn plain_chunks_keep_hashtags_and_identifiers() {
    let chunks = plain_chunks("#1 priority\n__init__", 3500);
    assert_eq!(chunks, vec!["#1 priority\n__init__"]);
}

#[test]
fn plain_chunks_of_nothing_is_no_message() {
    assert!(plain_chunks("  \n\n", 3500).is_empty());
}

#[test]
fn plain_chunks_split_a_long_reply_into_several() {
    let line = "x".repeat(100);
    let reply = vec![line.as_str(); 80].join("\n");
    let chunks = plain_chunks(&reply, 3500);
    assert_eq!(chunks.len(), 3);
    assert!(chunks.iter().all(|c| c.len() <= 3500));
}

#[test]
fn verbatim_chunks_keep_an_approval_exactly_as_written() {
    let preview =
        "run `bash -c \"echo listing files\n```touch X/pwned`\"`\n## step\n2**8 is **256**";
    let text = approval_text(preview, None);
    assert_eq!(verbatim_chunks(&text, CHUNK_LIMIT), vec![text.clone()]);
    assert_ne!(plain_chunks(&text, CHUNK_LIMIT), vec![text]);
}

#[test]
fn verbatim_chunks_still_respect_the_limit() {
    let chunks = verbatim_chunks("```\nalpha beta gamma delta", 11);
    assert_eq!(chunks, vec!["```", "alpha beta", "gamma delta"]);
}

#[test]
fn approval_text_names_the_three_answers() {
    let text = approval_text("run_command: rm -rf build", Some("this repo"));
    assert!(text.contains("rm -rf build"));
    assert!(text.contains("Scope: this repo"));
    assert!(text.ends_with("Reply yes, always, or no."));
}

#[test]
fn parse_approval_reads_the_three_answers() {
    assert!(matches!(parse_approval("Yes"), Some(Answer::Allow)));
    assert!(matches!(parse_approval(" ok. "), Some(Answer::Allow)));
    assert!(matches!(
        parse_approval("always"),
        Some(Answer::AlwaysAllow)
    ));
    assert!(matches!(parse_approval("No!"), Some(Answer::Deny)));
}

#[test]
fn parse_approval_leaves_other_text_to_the_agent() {
    assert!(parse_approval("use the other folder instead").is_none());
    assert!(parse_approval("stop").is_none());
}

#[test]
fn question_text_numbers_the_options() {
    let options = vec!["Tesco".to_string(), "Sainsbury's".to_string()];
    let text = question_text("Shop", "Which one?", &options);
    assert_eq!(
        text,
        "Shop\nWhich one?\n1. Tesco\n2. Sainsbury's\nReply with a number or your own answer, or skip."
    );
}

#[test]
fn resolve_choice_maps_numbers_and_names() {
    let options = vec!["Tesco".to_string(), "Sainsbury's".to_string()];
    assert_eq!(
        resolve_choice("2", &options).as_deref(),
        Some("Sainsbury's")
    );
    assert_eq!(resolve_choice("1.", &options).as_deref(), Some("Tesco"));
    assert_eq!(resolve_choice("tesco", &options).as_deref(), Some("Tesco"));
}

#[test]
fn resolve_choice_keeps_a_custom_answer() {
    let options = vec!["Tesco".to_string()];
    assert_eq!(resolve_choice("Aldi", &options).as_deref(), Some("Aldi"));
    assert_eq!(resolve_choice("3", &options).as_deref(), Some("3"));
}

#[test]
fn resolve_choice_skip_is_no_answer() {
    assert!(resolve_choice("Skip", &["a".to_string()]).is_none());
}

#[test]
fn shot_paths_finds_asterctl_screenshots() {
    let output = "shot /tmp/aster-shot-1.png (1080x2400, 52311 bytes)\nexit code: 0";
    assert_eq!(shot_paths(output), vec!["/tmp/aster-shot-1.png"]);
    assert!(shot_paths("nothing to see").is_empty());
}

#[test]
fn related_body_matches_photons_upload_layout() {
    let metadata = json!({ "filename": "hello.txt", "contentType": "text/plain", "sizeBytes": 19 });
    let body = related_body(
        "photon-postman-example",
        &metadata,
        "text/plain",
        b"Hello from Photon.\n",
    );
    let expected = format!(
        "--photon-postman-example\r\nContent-Type: application/json\r\n\r\n{metadata}\r\n\
         --photon-postman-example\r\nContent-Type: text/plain\r\n\r\n\
         Hello from Photon.\n\r\n--photon-postman-example--\r\n"
    );
    assert_eq!(String::from_utf8(body).unwrap(), expected);
}

#[test]
fn extension_for_reads_the_media_type() {
    assert_eq!(extension_for("image/jpeg"), "jpg");
    assert_eq!(extension_for("image/PNG; charset=binary"), "png");
    assert_eq!(extension_for("application/x-unknown"), "bin");
}

#[test]
fn retry_after_reads_seconds_and_caps_them() {
    assert_eq!(
        retry_after(Some("5")),
        Some(std::time::Duration::from_secs(5))
    );
    assert_eq!(
        retry_after(Some("3600")),
        Some(std::time::Duration::from_secs(60))
    );
    assert_eq!(retry_after(Some("Wed, 21 Oct 2026 07:28:00 GMT")), None);
    assert_eq!(retry_after(None), None);
}

#[test]
fn problem_code_reads_rfc_9457_bodies() {
    let body =
        r#"{"type":"about:blank","title":"Too Many Requests","status":429,"code":"RATE_LIMITED"}"#;
    assert_eq!(problem_code(body), "RATE_LIMITED");
    assert_eq!(problem_code("<html>"), "");
}

#[test]
fn api_error_says_what_failed_without_secrets() {
    let error = ApiError {
        op: "sendMessage",
        status: 403,
        code: "INSUFFICIENT_SCOPE".into(),
        retry_after: None,
    };
    assert_eq!(
        error.to_string(),
        "photon sendMessage failed: HTTP 403 INSUFFICIENT_SCOPE"
    );
}

#[test]
fn merge_queued_answers_the_first_message() {
    assert!(merge_queued(Vec::new()).is_none());
    let merged = merge_queued(vec![
        (Some("pho_msg_a".into()), "first".into()),
        (Some("pho_msg_b".into()), "second".into()),
    ]);
    assert_eq!(
        merged,
        Some((Some("pho_msg_a".to_string()), "first\n\nsecond".to_string()))
    );
}

#[tokio::test]
async fn a_stopped_turn_holds_its_place_until_it_finishes() {
    let mut state = ChatState::default();
    assert!(state.stop().is_none());
    let turn = tokio::spawn(std::future::pending::<()>());
    state.running = Some(turn.abort_handle());
    state.queued.push((None, "waiting".into()));

    assert_eq!(state.stop(), Some(1));
    assert!(state.stopped);
    assert!(state.running.is_some());
    assert!(turn.await.unwrap_err().is_cancelled());

    assert!(matches!(
        state.admit(Some("pho_msg_b"), "next job"),
        Admission::Queued
    ));
    assert_eq!(
        state.queued,
        vec![(Some("pho_msg_b".to_string()), "next job".to_string())]
    );
    assert!(matches!(state.admit(None, "stop"), Admission::Busy));

    state.running = None;
    assert!(matches!(
        state.admit(None, "after"),
        Admission::Start { .. }
    ));
    assert!(!state.stopped);
}

#[tokio::test]
async fn a_running_turn_takes_messages_as_asides() {
    let mut state = ChatState::default();
    let turn = tokio::spawn(std::future::pending::<()>());
    state.running = Some(turn.abort_handle());
    assert!(matches!(state.admit(None, "also this"), Admission::Busy));
    assert!(state.queued.is_empty());
    turn.abort();
}

#[tokio::test]
async fn dispatch_workers_go_away_once_their_chat_is_quiet() {
    let bridge = Arc::new(Bridge {
        api: Api::new("http://127.0.0.1:9", PROJECT, "pho_sk_test").unwrap(),
        cfg: SpectrumConfig {
            api_key: "pho_sk_test".into(),
            project_id: PROJECT.into(),
            api_base: "http://127.0.0.1:9".into(),
            platforms: all(),
            allowed_senders: vec!["+15550000000".into()],
            bin: PathBuf::from("aster"),
            repo_root: PathBuf::from("."),
            mode: "manual".into(),
        },
        skills: Arc::new(HashMap::new()),
        chats: Mutex::new(HashMap::new()),
        cursor: Mutex::new(Cursor::default()),
        cursor_path: None,
        told: Mutex::new(HashSet::new()),
        workers: Mutex::new(HashMap::new()),
    });
    for n in 0..3 {
        dispatch(
            &bridge,
            Inbound {
                id: format!("pho_msg_{n}"),
                chat: chat(),
                agent: AGENT.into(),
                sender: "+14155550123".into(),
                parts: vec![Part::Text("hello".into())],
            },
        );
    }
    for _ in 0..100 {
        if bridge.workers.lock().unwrap().is_empty() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("the chat's worker never finished");
}
