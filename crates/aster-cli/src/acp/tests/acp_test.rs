use agent_client_protocol::schema::v1::{ClientCapabilities, Meta, PromptRequest};
use serde_json::json;

use super::{reply_format, steer_only};
use crate::reply_format::ReplyFormat;

fn request(meta: Option<Meta>) -> PromptRequest {
    PromptRequest::new("session", Vec::new()).meta(meta)
}

#[test]
fn a_plain_prompt_may_run_as_its_own_turn() {
    assert!(!steer_only(&request(None)));
    assert!(!steer_only(&request(Some(Meta::from_iter([(
        "steerOnly".to_string(),
        json!(false),
    )])))));
}

#[test]
fn a_steer_only_prompt_says_so() {
    assert!(steer_only(&request(Some(Meta::from_iter([(
        "steerOnly".to_string(),
        json!(true),
    )])))));
}

#[test]
fn only_a_client_that_asks_for_openui_gets_it() {
    let asks = |meta: Option<Meta>| reply_format(&ClientCapabilities::new().meta(meta));
    let openui: Meta = serde_json::from_value(json!({ "aster.dev/openui": true })).unwrap();
    let declined: Meta = serde_json::from_value(json!({ "aster.dev/openui": false })).unwrap();
    assert_eq!(
        [asks(Some(openui)), asks(Some(declined)), asks(None)],
        [
            ReplyFormat::OpenUi,
            ReplyFormat::Markdown,
            ReplyFormat::Markdown
        ]
    );
}
