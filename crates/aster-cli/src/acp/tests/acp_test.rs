use agent_client_protocol::schema::v1::{Meta, PromptRequest};
use serde_json::json;

use super::steer_only;

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
