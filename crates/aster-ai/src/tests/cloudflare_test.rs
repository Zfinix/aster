use super::*;

#[test]
fn only_the_workers_ai_endpoint_lists_models_the_cloudflare_way() {
    let base = "https://api.cloudflare.com/client/v4/accounts/abc123/ai/v1";
    assert!(is_workers_ai(base));
    assert!(is_workers_ai(&format!("{base}/")));
    assert!(!is_workers_ai(
        "https://api.cloudflare.com/client/v4/accounts/abc123"
    ));
    assert!(!is_workers_ai("https://openrouter.ai/api/v1"));
}

#[test]
fn the_gist_shape_parses() {
    let body = r#"{
        "updated": "2026-09-08",
        "models": [
            {
                "id": "@cf/meta/llama-3.3-70b-instruct-fp8-fast",
                "task": "Text Generation",
                "description": "llama",
                "beta": false,
                "deprecated": false,
                "context_window": 24000,
                "pricing": null,
                "vision": false
            },
            {
                "id": "@cf/black-forest-labs/flux-1-schnell",
                "task": "Text-to-Image",
                "description": "flux",
                "beta": false,
                "deprecated": false,
                "context_window": null,
                "pricing": null,
                "vision": false
            }
        ]
    }"#;
    let models = parse_catalog(body).unwrap();
    // Image and speech models stay out; chat is all Aster sends.
    assert_eq!(models.len(), 1);
    assert_eq!(models[0].id, "@cf/meta/llama-3.3-70b-instruct-fp8-fast");
    assert_eq!(models[0].context_window, Some(24000));
}

#[test]
fn the_gist_parses_with_optional_fields_missing() {
    let gist = r#"{
        "updated": "2026-09-08",
        "models": [
            { "id": "@cf/zai-org/glm-5.3", "task": "Text Generation", "context_window": 128000 },
            { "id": "@cf/black-forest-labs/flux-2-dev", "task": "Text-to-Image" },
            { "id": "@cf/meta/llama-3.2-11b-vision-instruct", "task": "Image-to-Text", "vision": true }
        ]
    }"#;
    // The full map stays in the gist; only chat models reach the picker.
    let models = parse_catalog(gist).unwrap();
    assert_eq!(models.len(), 1);
    assert_eq!(models[0].id, "@cf/zai-org/glm-5.3");
    assert_eq!(models[0].context_window, Some(128000));
}

#[test]
fn the_search_url_drops_the_openai_path_and_asks_for_chat_models() {
    let url = models_url(
        "https://api.cloudflare.com/client/v4/accounts/abc123/ai/v1",
        2,
    );
    assert_eq!(
        url,
        "https://api.cloudflare.com/client/v4/accounts/abc123/ai/models/search\
         ?task=Text%20Generation&per_page=100&page=2"
    );
}

#[test]
fn models_come_back_named_with_the_vision_flag_the_search_declares() {
    let body = r#"{
        "success": true,
        "result": [
            { "name": "@cf/meta/llama-3.3-70b-instruct-fp8-fast", "properties": [
                { "property_id": "context_window", "value": "24000" }
            ]},
            { "name": "@cf/meta/llama-3.2-11b-vision-instruct", "properties": [
                { "property_id": "vision", "value": "true" }
            ]}
        ]
    }"#;
    let models = parse_models(body).unwrap();
    let ids: Vec<&str> = models.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "@cf/meta/llama-3.3-70b-instruct-fp8-fast",
            "@cf/meta/llama-3.2-11b-vision-instruct"
        ]
    );
    assert_eq!(models[0].takes_images, Some(false));
    assert_eq!(models[1].takes_images, Some(true));
}

#[test]
fn workers_ai_takes_a_string_content_and_no_zero_seed() {
    // The account endpoint rejects the parts array and a seed below 1, so a
    // plain text turn came back as a 400 rather than a reply.
    let body = serde_json::json!({
        "model": "@cf/meta/llama-3.3-70b-instruct-fp8-fast",
        "seed": 0,
        "messages": [
            { "role": "user", "content": [{ "type": "text", "text": "hi" }] },
            { "role": "assistant", "content": "already a string" }
        ]
    });
    let adapted = adapt_request(&body);
    assert!(adapted.get("seed").is_none(), "a seed of 0 must be dropped");
    assert_eq!(adapted["messages"][0]["content"], "hi");
    assert_eq!(adapted["messages"][1]["content"], "already a string");
}

#[test]
fn a_tool_call_message_carries_an_empty_string_rather_than_null() {
    // Null content reads as a missing field here, which failed every turn that
    // called a tool.
    let body = serde_json::json!({
        "messages": [
            { "role": "assistant", "content": null, "tool_calls": [{ "id": "1" }] },
            { "role": "tool", "content": "1 | hello" }
        ]
    });
    let adapted = adapt_request(&body);
    assert_eq!(adapted["messages"][0]["content"], "");
    assert_eq!(adapted["messages"][1]["content"], "1 | hello");
}

#[test]
fn a_seed_workers_ai_accepts_is_left_alone() {
    let body = serde_json::json!({ "seed": 7, "messages": [] });
    assert_eq!(adapt_request(&body)["seed"], 7);
}

#[test]
fn the_context_window_is_read_however_the_search_spells_the_number() {
    let body = r#"{"result":[
        { "name": "a", "properties": [{ "property_id": "context_window", "value": "24000" }] },
        { "name": "b", "properties": [{ "property_id": "context_window", "value": 8192 }] },
        { "name": "c", "properties": [] }
    ]}"#;
    let windows: Vec<Option<u32>> = parse_models(body)
        .unwrap()
        .iter()
        .map(|m| m.context_window)
        .collect();
    assert_eq!(windows, [Some(24000), Some(8192), None]);
}
