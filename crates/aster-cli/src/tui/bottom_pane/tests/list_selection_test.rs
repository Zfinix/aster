use super::*;
use ratatui::crossterm::event::KeyModifiers;

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn view(tx: mpsc::UnboundedSender<u8>) -> ListSelectionView<u8> {
    let items = vec![
        SelectionItem {
            name: "one".into(),
            description: "first".into(),
            is_current: false,
            event: 1,
        },
        SelectionItem {
            name: "two".into(),
            description: "second".into(),
            is_current: true,
            event: 2,
        },
        SelectionItem {
            name: "three".into(),
            description: "third".into(),
            is_current: false,
            event: 3,
        },
    ];
    ListSelectionView::new("Pick", items, tx, None)
}

fn long_view(count: usize) -> ListSelectionView<u8> {
    let (tx, _rx) = mpsc::unbounded_channel();
    let items = (0..count)
        .map(|i| SelectionItem {
            name: format!("session-{i}"),
            description: "a saved session".into(),
            is_current: false,
            event: 0,
        })
        .collect();
    ListSelectionView::new("Resume", items, tx, None)
}

#[test]
fn height_does_not_grow_with_the_item_count() {
    let short = long_view(3).desired_height(80);
    let long = long_view(400).desired_height(80);
    assert!(
        long <= short + VISIBLE_ROWS as u16,
        "short {short}, long {long}"
    );
    assert!(long < 20, "the pane would eat the screen: {long}");
}

#[test]
fn the_window_follows_the_selection_and_counts_the_rest() {
    let mut v = long_view(400);
    v.selected = 300;
    let shown = v.lines();
    assert!(
        shown.iter().any(|l| l.to_string().contains("session-300")),
        "selection scrolled off: {shown:?}"
    );
    assert!(
        shown.iter().any(|l| l.to_string().contains("+390 more")),
        "{shown:?}"
    );
}

#[test]
fn opens_on_the_current_item_and_wraps() {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let mut v = view(tx);
    assert_eq!(v.selected, 1);
    v.handle_key(key(KeyCode::Down));
    v.handle_key(key(KeyCode::Down));
    assert_eq!(v.selected, 0);
    v.handle_key(key(KeyCode::Enter));
    assert!(v.is_complete());
    assert_eq!(rx.try_recv().unwrap(), 1);
}

#[test]
fn digits_select_and_accept_in_one_press() {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let mut v = view(tx);
    v.handle_key(key(KeyCode::Char('3')));
    assert!(v.is_complete());
    assert_eq!(rx.try_recv().unwrap(), 3);
}

#[test]
fn typing_filters_the_rows_and_enter_accepts_the_survivor() {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let mut v = view(tx);
    v.handle_key(key(KeyCode::Char('t')));
    v.handle_key(key(KeyCode::Char('h')));
    v.handle_key(key(KeyCode::Enter));
    assert!(v.is_complete());
    assert_eq!(rx.try_recv().unwrap(), 3);
}

#[test]
fn backspace_widens_the_filter_again() {
    let (tx, _rx) = mpsc::unbounded_channel();
    let mut v = view(tx);
    v.handle_key(key(KeyCode::Char('t')));
    v.handle_key(key(KeyCode::Char('h')));
    assert_eq!(v.filtered().len(), 1);
    v.handle_key(key(KeyCode::Backspace));
    assert_eq!(v.filtered().len(), 2, "t matches two and three");
}

#[test]
fn y_and_n_answer_a_yes_no_list_without_enter() {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let items = vec![
        SelectionItem {
            name: "No, keep it".into(),
            description: String::new(),
            is_current: true,
            event: 0u8,
        },
        SelectionItem {
            name: "Yes, delete it".into(),
            description: String::new(),
            is_current: false,
            event: 1,
        },
    ];
    let mut v = ListSelectionView::new("Delete?", items, tx, None);
    v.handle_key(key(KeyCode::Char('y')));
    assert!(v.is_complete());
    assert_eq!(rx.try_recv().unwrap(), 1);
}

#[test]
fn esc_sends_dismiss_event_when_provided() {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let items = vec![SelectionItem {
        name: "only".into(),
        description: String::new(),
        is_current: false,
        event: 1u8,
    }];
    let mut v = ListSelectionView::new("Pick", items, tx, Some(99u8));
    v.handle_key(key(KeyCode::Esc));
    assert!(v.is_complete());
    assert_eq!(rx.try_recv().unwrap(), 99);
}

#[test]
fn esc_cancels_without_sending() {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let mut v = view(tx);
    v.handle_key(key(KeyCode::Esc));
    assert!(v.is_complete());
    assert!(rx.try_recv().is_err());
}

#[test]
fn out_of_range_digit_does_nothing() {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let mut v = view(tx);
    v.handle_key(key(KeyCode::Char('9')));
    assert!(!v.is_complete());
    assert!(rx.try_recv().is_err());
}
