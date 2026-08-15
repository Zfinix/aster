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
