use super::*;
use reqwest::StatusCode;

#[test]
fn format_api_error_prefers_openrouter_upstream_message() {
    let body = r#"{"error":{"message":"Provider returned error","code":429,"metadata":{"raw":"kimi is temporarily rate-limited upstream. Please retry shortly.","provider_name":"Moonshot AI"}}}"#;
    let msg = format_api_error(StatusCode::TOO_MANY_REQUESTS, body);
    assert_eq!(
        msg,
        "rate limited (429): kimi is temporarily rate-limited upstream. Please retry shortly."
    );
}

#[test]
fn format_api_error_falls_back_to_error_message() {
    let body = r#"{"error":{"message":"invalid model id"}}"#;
    assert_eq!(
        format_api_error(StatusCode::BAD_REQUEST, body),
        "bad request (400): invalid model id"
    );
}

#[test]
fn format_api_error_labels_auth_failures() {
    let msg = format_api_error(StatusCode::UNAUTHORIZED, "");
    assert_eq!(msg, "authentication failed (check your API key) (401)");
}

#[test]
fn format_api_error_uses_raw_body_when_not_json() {
    let msg = format_api_error(StatusCode::INTERNAL_SERVER_ERROR, "upstream down");
    assert_eq!(msg, "provider error (500): upstream down");
}

#[test]
fn a_catalog_entry_without_an_architecture_says_nothing_about_images() {
    let entry: ModelEntry = serde_json::from_str(r#"{"id":"local/model"}"#).unwrap();
    assert_eq!(ModelInfo::from(entry).takes_images, None);
}

#[test]
fn a_catalog_entry_reports_the_modalities_it_declares() {
    let body = r#"{"id":"openai/gpt-4o","architecture":{"input_modalities":["text","image"]}}"#;
    let entry: ModelEntry = serde_json::from_str(body).unwrap();
    assert_eq!(ModelInfo::from(entry).takes_images, Some(true));

    let body = r#"{"id":"deepseek/deepseek-chat","architecture":{"input_modalities":["text"]}}"#;
    let entry: ModelEntry = serde_json::from_str(body).unwrap();
    assert_eq!(ModelInfo::from(entry).takes_images, Some(false));
}

#[test]
fn an_empty_modality_list_is_a_declaration_of_nothing() {
    let body = r#"{"id":"x/y","architecture":{"input_modalities":[]}}"#;
    let entry: ModelEntry = serde_json::from_str(body).unwrap();
    assert_eq!(ModelInfo::from(entry).takes_images, None);
}

#[test]
fn only_an_error_that_names_the_images_is_worth_retrying_without_them() {
    assert!(rejected_images(&anyhow::anyhow!(
        "bad request (400): image exceeds 5 MB maximum"
    )));
    assert!(!rejected_images(&anyhow::anyhow!(
        "rate limited (429): slow down"
    )));
}

fn detail(kind: &str, index: u32) -> ReasoningDetail {
    ReasoningDetail {
        kind: kind.into(),
        format: Some("anthropic-claude-v1".into()),
        text: None,
        summary: None,
        data: None,
        signature: None,
        id: None,
        index: Some(index),
    }
}

#[test]
fn reasoning_fragments_without_index_merge_into_last_block() {
    let mut out = Vec::new();
    let kind = |text: &str, index: Option<u32>| ReasoningDetail {
        kind: "reasoning.text".into(),
        format: Some("anthropic-claude-v1".into()),
        text: Some(text.into()),
        summary: None,
        data: None,
        signature: None,
        id: None,
        index,
    };

    merge_reasoning(&mut out, kind("first block ", Some(0)));

    merge_reasoning(&mut out, kind("fragment a ", None));
    merge_reasoning(&mut out, kind("fragment b", None));

    assert_eq!(out.len(), 2, "indexed block and unindexed merged block");
    assert_eq!(out[0].text.as_deref(), Some("first block "));
    assert_eq!(out[1].text.as_deref(), Some("fragment a fragment b"));
}

#[test]
fn streamed_reasoning_fragments_sharing_an_index_rejoin_in_order() {
    let mut out = Vec::new();
    for chunk in ["the ", "config ", "is inert"] {
        merge_reasoning(
            &mut out,
            ReasoningDetail {
                text: Some(chunk.into()),
                ..detail("reasoning.text", 0)
            },
        );
    }
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].text.as_deref(), Some("the config is inert"));
}

#[test]
fn reasoning_blocks_at_different_indexes_stay_separate() {
    let mut out = Vec::new();
    merge_reasoning(
        &mut out,
        ReasoningDetail {
            text: Some("first".into()),
            ..detail("reasoning.text", 0)
        },
    );
    merge_reasoning(
        &mut out,
        ReasoningDetail {
            text: Some("second".into()),
            ..detail("reasoning.text", 1)
        },
    );
    assert_eq!(out.len(), 2);
    assert_eq!(out[1].text.as_deref(), Some("second"));
}

#[test]
fn a_late_signature_attaches_to_the_block_it_seals() {
    let mut out = Vec::new();
    merge_reasoning(
        &mut out,
        ReasoningDetail {
            text: Some("thinking".into()),
            ..detail("reasoning.text", 0)
        },
    );
    merge_reasoning(
        &mut out,
        ReasoningDetail {
            signature: Some("CAIS...".into()),
            ..detail("reasoning.text", 0)
        },
    );
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].signature.as_deref(), Some("CAIS..."));
}

#[test]
fn only_unsealed_reasoning_survives_a_model_switch() {
    let plain = ReasoningDetail {
        text: Some("open reasoning".into()),
        ..detail("reasoning.text", 0)
    };
    let signed = ReasoningDetail {
        text: Some("open reasoning".into()),
        signature: Some("CAIS...".into()),
        ..detail("reasoning.text", 0)
    };
    let sealed = ReasoningDetail {
        data: Some("opaque".into()),
        ..detail("reasoning.encrypted", 0)
    };
    assert!(plain.portable());
    assert!(
        !signed.portable(),
        "a signature binds the block to its model"
    );
    assert!(!sealed.portable());
}

#[test]
fn a_sealed_block_reads_as_nothing_in_the_text_only_paths() {
    let sealed = ReasoningDetail {
        data: Some("opaque".into()),
        ..detail("reasoning.encrypted", 0)
    };
    assert_eq!(sealed.plain(), None);
    let summarized = ReasoningDetail {
        summary: Some("  checked the config  ".into()),
        ..detail("reasoning.summary", 0)
    };
    assert_eq!(summarized.plain(), Some("checked the config"));
}

fn fragment(
    index: usize,
    id: Option<&str>,
    name: Option<&str>,
    args: Option<&str>,
) -> ToolCallDelta {
    ToolCallDelta {
        index,
        id: id.map(str::to_string),
        function: Some(crate::models::ToolCallFunctionDelta {
            name: name.map(str::to_string),
            arguments: args.map(str::to_string),
        }),
    }
}

#[test]
fn tool_call_fragments_with_the_same_index_and_id_merge_into_one() {
    let mut partials: BTreeMap<usize, PartialToolCall> = BTreeMap::new();
    merge_tool_call(
        &mut partials,
        fragment(0, Some("call_1"), Some("read"), Some("{\"a\"")),
    );
    merge_tool_call(&mut partials, fragment(0, None, None, Some(":1}")));
    assert_eq!(partials.len(), 1);
    let (index, slot) = partials.iter().next().unwrap();
    assert_eq!(*index, 0);
    assert_eq!(slot.id, "call_1");
    assert_eq!(slot.name, "read");
    assert_eq!(slot.arguments, "{\"a\":1}");
}

#[test]
fn a_reused_index_with_a_different_id_routes_to_a_fresh_slot() {
    let mut partials: BTreeMap<usize, PartialToolCall> = BTreeMap::new();
    merge_tool_call(
        &mut partials,
        fragment(0, Some("call_1"), Some("read"), Some("{\"a\":1}")),
    );
    merge_tool_call(
        &mut partials,
        fragment(0, Some("call_2"), Some("write"), Some("{\"b\":2}")),
    );
    assert_eq!(
        partials.len(),
        2,
        "reused index must not splice two distinct calls"
    );
    let by_id: std::collections::BTreeMap<&str, &PartialToolCall> =
        partials.values().map(|p| (p.id.as_str(), p)).collect();
    let first = by_id["call_1"];
    let second = by_id["call_2"];
    assert_eq!(first.name, "read");
    assert_eq!(first.arguments, "{\"a\":1}");
    assert_eq!(second.name, "write");
    assert_eq!(second.arguments, "{\"b\":2}");
}

#[test]
fn a_reused_index_with_only_a_different_name_routes_to_a_fresh_slot() {
    // Some providers never echo the id back in fragments; the name is the only
    // identity we get, so it alone has to be enough to detect a collision.
    let mut partials: BTreeMap<usize, PartialToolCall> = BTreeMap::new();
    merge_tool_call(
        &mut partials,
        fragment(0, None, Some("read"), Some("{\"a\":1}")),
    );
    merge_tool_call(
        &mut partials,
        fragment(0, None, Some("write"), Some("{\"b\":2}")),
    );
    assert_eq!(partials.len(), 2);
    let names: Vec<&str> = partials.values().map(|p| p.name.as_str()).collect();
    assert!(names.contains(&"read"));
    assert!(names.contains(&"write"));
}

#[test]
fn format_api_error_reads_fastapi_detail_shapes() {
    let body = r#"{"detail":"The 'stealth/ox-alpha' model is not supported when using Codex with a ChatGPT account."}"#;
    assert_eq!(
        format_api_error(StatusCode::BAD_REQUEST, body),
        "bad request (400): The 'stealth/ox-alpha' model is not supported when using Codex with a ChatGPT account."
    );
    let body = r#"{"detail":[{"loc":["body","model"],"msg":"field required"}]}"#;
    assert_eq!(
        format_api_error(StatusCode::BAD_REQUEST, body),
        "bad request (400): field required"
    );
}

#[test]
fn format_api_error_reads_a_bare_error_string() {
    let body = r#"{"error":"model \"qwen3\" not found, try pulling it first"}"#;
    assert_eq!(
        format_api_error(StatusCode::NOT_FOUND, body),
        "model or endpoint not found (404): model \"qwen3\" not found, try pulling it first"
    );
}
