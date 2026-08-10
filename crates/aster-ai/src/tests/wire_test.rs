use super::*;

fn roles(messages: &[Value]) -> Vec<&str> {
    messages.iter().filter_map(|m| role_of(m)).collect()
}

#[test]
fn a_leading_system_prompt_is_left_alone() {
    let messages = vec![
        json!({ "role": "system", "content": "you are aster" }),
        json!({ "role": "user", "content": "hello" }),
    ];
    let folded = fold_system_notes(messages.clone());
    assert_eq!(folded, messages);
}

#[test]
fn a_note_after_an_assistant_reply_becomes_its_own_user_turn() {
    // The shape Anthropic rejects: `messages.2` is a system message following
    // an assistant reply that did not end in a tool result.
    let folded = fold_system_notes(vec![
        json!({ "role": "system", "content": "you are aster" }),
        json!({ "role": "user", "content": "what pr is open" }),
        json!({ "role": "assistant", "content": "none that I can see" }),
        json!({ "role": "system", "content": "Edits are now enabled (auto)" }),
        json!({ "role": "user", "content": "try again" }),
    ]);
    assert_eq!(
        roles(&folded),
        ["system", "user", "assistant", "user", "user"]
    );
    assert_eq!(
        folded[3]["content"],
        json!("<system-note>Edits are now enabled (auto)</system-note>")
    );
}

#[test]
fn a_note_after_a_user_turn_merges_into_it() {
    let folded = fold_system_notes(vec![
        json!({ "role": "system", "content": "you are aster" }),
        json!({ "role": "user", "content": "what pr is open" }),
        json!({ "role": "system", "content": "Edits are now disabled" }),
    ]);
    assert_eq!(roles(&folded), ["system", "user"]);
    assert_eq!(
        folded[1]["content"],
        json!("what pr is open\n\n<system-note>Edits are now disabled</system-note>")
    );
}

#[test]
fn consecutive_leading_system_messages_all_stay_system() {
    let folded = fold_system_notes(vec![
        json!({ "role": "system", "content": "persona" }),
        json!({ "role": "system", "content": "skills index" }),
        json!({ "role": "user", "content": "hi" }),
    ]);
    assert_eq!(roles(&folded), ["system", "system", "user"]);
}

#[test]
fn tool_turns_are_never_merged_into() {
    let folded = fold_system_notes(vec![
        json!({ "role": "system", "content": "you are aster" }),
        json!({ "role": "user", "content": "list the prs" }),
        json!({ "role": "assistant", "content": null, "tool_calls": [{ "id": "c1" }] }),
        json!({ "role": "tool", "tool_call_id": "c1", "content": "{}" }),
        json!({ "role": "system", "content": "Edits are now disabled" }),
    ]);
    assert_eq!(
        roles(&folded),
        ["system", "user", "assistant", "tool", "user"]
    );
    // The tool result is untouched; the note rides in a turn of its own.
    assert_eq!(folded[3]["tool_call_id"], json!("c1"));
}

#[test]
fn a_note_with_structured_content_still_becomes_a_user_turn() {
    let folded = fold_system_notes(vec![
        json!({ "role": "user", "content": "hi" }),
        json!({ "role": "system", "content": [{ "type": "text", "text": "note" }] }),
    ]);
    assert_eq!(roles(&folded), ["user", "user"]);
    assert!(folded[1]["content"].is_array());
}

#[test]
fn the_typed_path_folds_the_same_way() {
    let folded = fold_system_chat(vec![
        ChatMessage {
            role: "system".into(),
            content: "you are aster".into(),
        },
        ChatMessage {
            role: "user".into(),
            content: "what pr is open".into(),
        },
        ChatMessage {
            role: "assistant".into(),
            content: "none".into(),
        },
        ChatMessage {
            role: "system".into(),
            content: "Edits are now enabled (auto)".into(),
        },
    ]);
    let roles: Vec<&str> = folded.iter().map(|m| m.role.as_str()).collect();
    assert_eq!(roles, ["system", "user", "assistant", "user"]);
    assert_eq!(
        folded[3].content,
        "<system-note>Edits are now enabled (auto)</system-note>"
    );
}

#[test]
fn a_conversation_with_no_notes_is_unchanged() {
    let messages = vec![
        json!({ "role": "system", "content": "you are aster" }),
        json!({ "role": "user", "content": "hi" }),
        json!({ "role": "assistant", "content": "hello" }),
        json!({ "role": "user", "content": "bye" }),
    ];
    assert_eq!(fold_system_notes(messages.clone()), messages);
}
