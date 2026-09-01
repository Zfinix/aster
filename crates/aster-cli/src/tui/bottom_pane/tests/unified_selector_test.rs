use super::*;
use ratatui::crossterm::event::KeyModifiers;

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn item(section: UnifiedSection, name: &str, is_current: bool, event: u8) -> UnifiedItem<u8> {
    UnifiedItem {
        section,
        name: name.into(),
        description: String::new(),
        is_current,
        event,
    }
}

fn view(tx: mpsc::UnboundedSender<u8>) -> UnifiedSelector<u8> {
    let items = vec![
        item(UnifiedSection::Options, "Thinking", false, 1),
        item(UnifiedSection::Mode, "plan", false, 2),
        item(UnifiedSection::Mode, "edit", true, 3),
        item(UnifiedSection::Effort, "low", false, 4),
        item(UnifiedSection::Effort, "high", false, 5),
    ];
    UnifiedSelector::new(items, tx)
}

#[test]
fn it_opens_on_the_row_in_force() {
    let (tx, _rx) = mpsc::unbounded_channel();
    assert_eq!(view(tx).selected, 2);
}

#[test]
fn a_click_skips_the_section_headers() {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let mut v = view(tx);
    // filter row, OPTIONS, Thinking, MODE, plan, edit
    assert!(!v.handle_click(3));
    assert!(v.handle_click(5));
    assert_eq!(rx.try_recv().ok(), Some(3));
    assert!(v.is_complete());
}

#[test]
fn moving_walks_the_items_not_the_headers() {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let mut v = view(tx);
    v.handle_key(key(KeyCode::Down));
    v.handle_key(key(KeyCode::Enter));
    assert_eq!(rx.try_recv().ok(), Some(4));
}

#[test]
fn typing_filters_and_keeps_the_selection_on_a_shown_row() {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let mut v = view(tx);
    for c in "hi".chars() {
        v.handle_key(key(KeyCode::Char(c)));
    }
    assert_eq!(v.selected, 0, "only Thinking matches");
    v.handle_key(key(KeyCode::Enter));
    assert_eq!(rx.try_recv().ok(), Some(1));
}

#[test]
fn esc_closes_without_choosing() {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let mut v = view(tx);
    v.handle_key(key(KeyCode::Esc));
    assert!(v.is_complete());
    assert!(rx.try_recv().is_err());
}

#[test]
fn height_does_not_grow_with_the_item_count() {
    let (tx, _rx) = mpsc::unbounded_channel();
    let items = (0..400)
        .map(|i| item(UnifiedSection::Provider, &format!("provider-{i}"), false, 0))
        .collect();
    let tall = UnifiedSelector::new(items, tx).desired_height(80);
    assert!(tall < 24, "the panel would eat the screen: {tall}");
}
