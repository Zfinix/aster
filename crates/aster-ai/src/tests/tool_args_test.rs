use super::{heal, heal_calls};
use crate::models::{ToolCall, ToolCallFunction};

fn call(arguments: &str) -> ToolCall {
    ToolCall {
        id: "call_1".into(),
        kind: "function".into(),
        function: ToolCallFunction {
            name: "read_file".into(),
            arguments: arguments.into(),
        },
    }
}

#[test]
fn leaves_valid_arguments_alone() {
    assert_eq!(heal(r#"{"path":"src/lib.rs"}"#), r#"{"path":"src/lib.rs"}"#);
}

#[test]
fn treats_empty_arguments_as_an_empty_object() {
    assert_eq!(heal(""), "{}");
    assert_eq!(heal("   "), "{}");
}

#[test]
fn closes_a_stream_cut_mid_string() {
    assert_eq!(heal(r#"{"path":"src/li"#), r#"{"path":"src/li"}"#);
}

#[test]
fn closes_nested_scopes_in_order() {
    assert_eq!(
        heal(r#"{"edits":[{"path":"a.rs""#),
        r#"{"edits":[{"path":"a.rs"}]}"#
    );
}

#[test]
fn drops_a_pair_that_has_no_value_yet() {
    assert_eq!(heal(r#"{"path":"a.rs","content":"#), r#"{"path":"a.rs"}"#);
    assert_eq!(heal(r#"{"path":"a.rs","#), r#"{"path":"a.rs"}"#);
}

#[test]
fn unwraps_double_encoded_arguments() {
    assert_eq!(heal(r#""{\"path\":\"a.rs\"}""#), r#"{"path":"a.rs"}"#);
}

#[test]
fn falls_back_to_an_empty_object_when_nothing_can_be_saved() {
    assert_eq!(heal("not json at all"), "{}");
    assert_eq!(heal("[1, 2, 3]"), "{}");
}

#[test]
fn heals_every_call_in_the_message() {
    let mut calls = vec![call(r#"{"path":"a.rs"}"#), call(r#"{"path":"b"#)];
    heal_calls(&mut calls);
    assert_eq!(calls[0].function.arguments, r#"{"path":"a.rs"}"#);
    assert_eq!(calls[1].function.arguments, r#"{"path":"b"}"#);
}
