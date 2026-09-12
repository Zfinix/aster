use super::*;
use aster_persist::MessageEvent;

fn seeded(home: &Path, repo: &Path, title: &str, said: &[(&str, &str)]) -> (Store, String) {
    let store = Store::open(home).unwrap();
    let mut writer = store.new_session(repo, repo, None, None).unwrap();
    writer.set_title(title).unwrap();
    for (role, content) in said {
        let message = match *role {
            "user" => MessageEvent::user(*content),
            _ => MessageEvent::assistant(Some((*content).to_string()), Vec::new()),
        };
        writer.append_message(message).unwrap();
    }
    let id = writer.id().to_string();
    (store, id)
}

#[test]
fn lists_saved_chats_newest_first_and_marks_this_one() {
    let home = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    let (store, id) = seeded(
        home.path(),
        repo.path(),
        "Find missing installer",
        &[("user", "where is the installer")],
    );

    let out = list(
        &store,
        repo.path(),
        Some(&id),
        &HistoryArgs {
            id: None,
            query: None,
            limit: None,
            all: false,
        },
    );

    assert!(out.contains(&id), "{out}");
    assert!(out.contains("Find missing installer"), "{out}");
    assert!(out.contains("[this chat]"), "{out}");
}

#[test]
fn reads_this_chat_back_when_asked_for_current() {
    let home = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    let (store, id) = seeded(
        home.path(),
        repo.path(),
        "Telegram bridge",
        &[
            ("user", "can you read the current chat history"),
            ("assistant", "yes, here it is"),
        ],
    );

    let out = show(&store, repo.path(), "current", Some(&id));

    assert!(
        out.contains("user: can you read the current chat history"),
        "{out}"
    );
    assert!(out.contains("assistant: yes, here it is"), "{out}");
}

#[test]
fn says_which_id_it_could_not_find() {
    let home = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    let store = Store::open(home.path()).unwrap();

    let out = show(&store, repo.path(), "nope", None);

    assert!(out.starts_with("error: no saved chat with id"), "{out}");
}

#[test]
fn a_query_lists_only_the_chats_that_mention_it() {
    let home = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    let (store, _) = seeded(
        home.path(),
        repo.path(),
        "Bridge work",
        &[("user", "the telegram bridge drops replies")],
    );

    let hit = list(
        &store,
        repo.path(),
        None,
        &HistoryArgs {
            id: None,
            query: Some("telegram"),
            limit: None,
            all: false,
        },
    );
    let miss = list(
        &store,
        repo.path(),
        None,
        &HistoryArgs {
            id: None,
            query: Some("kubernetes"),
            limit: None,
            all: false,
        },
    );

    assert!(hit.contains("Bridge work"), "{hit}");
    assert!(hit.contains("the telegram bridge drops replies"), "{hit}");
    assert_eq!(miss, "No saved chat mentions \"kubernetes\".");
}

#[test]
fn a_long_chat_keeps_its_tail() {
    let lines: Vec<String> = (0..50).map(|n| format!("user: message {n}")).collect();

    let out = tail(&lines, 120);

    assert!(out.starts_with("[earlier messages left out]"), "{out}");
    assert!(out.contains("message 49"), "{out}");
    assert!(!out.contains("message 10"), "{out}");
}
