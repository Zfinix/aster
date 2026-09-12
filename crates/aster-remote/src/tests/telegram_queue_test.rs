use std::sync::{Arc, Mutex};

use super::{Chats, chat_state, take_queued};

fn chats() -> Chats {
    Arc::new(Mutex::new(Default::default()))
}

#[test]
fn nothing_waiting_is_nothing_to_run() {
    assert!(take_queued(&chats(), 1).is_none());
}

#[test]
fn waiting_messages_run_as_one_turn() {
    let chats = chats();
    chat_state(&chats, 1, |state| {
        state.queued.push((7, "first".into()));
        state.queued.push((8, "second".into()));
    });
    assert_eq!(
        take_queued(&chats, 1),
        Some((7, "first\n\nsecond".to_string())),
        "both run together, as a reply to the first"
    );
    assert!(take_queued(&chats, 1).is_none(), "the queue is drained");
}

#[test]
fn queues_are_per_chat() {
    let chats = chats();
    chat_state(&chats, 1, |state| state.queued.push((7, "mine".into())));
    assert!(take_queued(&chats, 2).is_none());
    assert!(take_queued(&chats, 1).is_some());
}
