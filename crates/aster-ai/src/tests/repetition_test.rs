use super::*;
use crate::AiClient;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn degenerate() -> String {
    "I'm committing. ".repeat(60)
}

#[test]
fn repeated_prose_trips() {
    let mut guard = RepetitionGuard::default();
    let out = degenerate();
    let mut tripped = false;
    for delta in out.as_bytes().chunks(11) {
        if guard.feed(std::str::from_utf8(delta).unwrap()) {
            tripped = true;
            break;
        }
    }
    assert!(tripped);
    assert!(is_degenerate(&out));
}

#[test]
fn varied_prose_does_not_trip() {
    let mut guard = RepetitionGuard::default();
    let text = "The model is reviewing this function, so give it the whole file. \
                Maybe the callers too. Maybe the tests. Every addition felt like \
                diligence, and past a small threshold every addition made the output \
                worse. The model does not read context the way you hope."
        .repeat(2);
    for delta in text.as_bytes().chunks(13) {
        assert!(!guard.feed(std::str::from_utf8(delta).unwrap()));
    }
    assert!(!is_degenerate(&text));
}

#[test]
fn separator_wall_does_not_trip() {
    let mut guard = RepetitionGuard::default();
    let wall = "--------".repeat(80);
    for delta in wall.as_bytes().chunks(19) {
        assert!(!guard.feed(std::str::from_utf8(delta).unwrap()));
    }
    assert!(!is_degenerate(&wall));
}

#[test]
fn chunks_split_across_deltas_still_trip() {
    let mut guard = RepetitionGuard::default();
    let out = degenerate();
    // Odd, uneven delta sizes so period boundaries land mid-delta.
    let mut tripped = false;
    for delta in out.as_bytes().chunks(7) {
        if guard.feed(std::str::from_utf8(delta).unwrap()) {
            tripped = true;
            break;
        }
    }
    assert!(tripped);
}

#[test]
fn short_text_does_not_trip() {
    assert!(!is_degenerate("a short reply"));
}

#[test]
fn marker_downcasts() {
    let e = anyhow::Error::new(DegenerateOutput)
        .context("the model's reply degenerated into repeated text");
    assert!(e.downcast_ref::<DegenerateOutput>().is_some());
}

#[tokio::test]
async fn degenerate_stream_is_cut_off_mid_turn() {
    let server = MockServer::start().await;
    let mut sse = String::new();
    for _ in 0..40 {
        sse.push_str(&format!(
            "data: {}\n\n",
            json!({ "choices": [{ "delta": { "content": "I'm committing. " } }] })
        ));
    }
    sse.push_str("data: [DONE]\n\n");
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_string(sse))
        .mount(&server)
        .await;

    let client = AiClient::new(server.uri(), "test-key", "mock-model");
    let err = client
        .complete_tools_stream_with("mock-model", vec![], vec![], 0.0, |_| {})
        .await
        .unwrap_err();
    assert!(
        err.downcast_ref::<DegenerateOutput>().is_some(),
        "unexpected error: {err:#}"
    );
}

#[tokio::test]
async fn degenerate_non_streaming_reply_is_rejected() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{ "message": { "role": "assistant", "content": degenerate() } }],
            "usage": { "prompt_tokens": 10, "completion_tokens": 10 }
        })))
        .mount(&server)
        .await;

    let client = AiClient::new(server.uri(), "test-key", "mock-model");
    let err = client
        .complete_tools_with("mock-model", vec![], vec![], 0.0)
        .await
        .unwrap_err();
    assert!(
        err.downcast_ref::<DegenerateOutput>().is_some(),
        "unexpected error: {err:#}"
    );
}

/// The buffer is trimmed at a byte offset computed from its length, so a
/// multi-byte character straddling that offset used to panic the streaming
/// task: "end byte index 216 is not a char boundary; it is inside '├'".
#[test]
fn a_multibyte_character_on_the_trim_boundary_does_not_panic() {
    // A directory tree is the everyday case: the glyphs are three bytes each.
    let tree = "├─ crates/aster-ai\n│  └─ src/lib.rs\n";
    for chunk in 1..=8 {
        let mut guard = RepetitionGuard::default();
        let text = tree.repeat(40);
        let mut cut = 0;
        while cut < text.len() {
            let mut end = (cut + chunk).min(text.len());
            while !text.is_char_boundary(end) {
                end += 1;
            }
            guard.feed(&text[cut..end]);
            cut = end;
        }
    }
}

#[test]
fn emoji_and_cjk_survive_the_window_split() {
    let mut guard = RepetitionGuard::default();
    for delta in ["🙂".repeat(200), "日本語のテキスト".repeat(50)] {
        guard.feed(&delta);
    }
    // Nothing to assert beyond not panicking: the guard may or may not trip on
    // repeated emoji, and either verdict is fine.
    assert!(!is_degenerate("🙂 one 🙂 two 🙂 three"));
}

#[test]
fn a_repeated_tree_still_trips_the_guard() {
    // Boundary rounding must not cost the guard its job.
    let out = "├─ same line every time\n".repeat(60);
    assert!(is_degenerate(&out));
}
