use super::*;

#[test]
fn sanitize_caps_items_and_text() {
    let long = "x".repeat(500);
    let items: Vec<Announcement> = (0..8)
        .map(|i| Announcement {
            id: format!("id-{i}"),
            text: long.clone(),
        })
        .collect();
    let out = sanitize(items);
    assert_eq!(out.len(), MAX_ITEMS);
    assert_eq!(out[0].text.chars().count(), MAX_TEXT_CHARS);
}

#[test]
fn sanitize_drops_empty_entries() {
    let out = sanitize(vec![
        Announcement {
            id: String::new(),
            text: "no id".into(),
        },
        Announcement {
            id: "ok".into(),
            text: String::new(),
        },
        Announcement {
            id: "good".into(),
            text: "New: scheduled runs. Set a cron and aster runs itself.".into(),
        },
    ]);
    assert_eq!(
        out,
        vec![Announcement {
            id: "good".into(),
            text: "New: scheduled runs. Set a cron and aster runs itself.".into(),
        }]
    );
}
