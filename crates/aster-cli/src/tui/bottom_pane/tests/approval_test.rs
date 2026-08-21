use super::*;
use ratatui::crossterm::event::KeyModifiers;
use tokio::sync::oneshot;

#[derive(Clone, Debug, PartialEq)]
struct Decided(Answer, Option<PathBuf>);

fn request(preview: &str) -> (ApprovalRequest, oneshot::Receiver<Answer>) {
    let (respond, rx) = oneshot::channel();
    (
        ApprovalRequest {
            markdown: None,
            preview: preview.into(),
            scope: None,
            respond,
        },
        rx,
    )
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

#[test]
fn y_answers_yes_and_completes() {
    let (tx, mut rx_ev) = mpsc::unbounded_channel();
    let (req, rx) = request("edit a.rs:\n- old\n+ new");
    let mut v = ApprovalView::new(req, tx, Decided);
    v.handle_key(key(KeyCode::Char('y')));
    assert!(v.is_complete());
    assert_eq!(rx.blocking_recv(), Ok(Answer::Yes));
    assert_eq!(rx_ev.try_recv().unwrap(), Decided(Answer::Yes, None));
}

#[test]
fn a_second_request_queues_and_is_shown_next() {
    let (tx, _rx_ev) = mpsc::unbounded_channel::<Decided>();
    let (first, rx1) = request("edit a.rs:\n+ a");
    let (second, rx2) = request("edit b.rs:\n+ b");
    let mut v = ApprovalView::new(first, tx, Decided);
    assert!(v.try_consume_approval(second).is_none());

    v.handle_key(key(KeyCode::Char('n')));
    assert!(!v.is_complete(), "the queued request takes over");
    assert_eq!(rx1.blocking_recv(), Ok(Answer::No));

    v.handle_key(key(KeyCode::Char('y')));
    assert!(v.is_complete());
    assert_eq!(rx2.blocking_recv(), Ok(Answer::Yes));
}

#[test]
fn enter_confirms_the_highlighted_option() {
    let (tx, mut rx_ev) = mpsc::unbounded_channel();
    let (req, rx) = request("edit a.rs:\n+ a");
    let mut v = ApprovalView::new(req, tx, Decided);
    v.handle_key(key(KeyCode::Down));
    v.handle_key(key(KeyCode::Enter));
    assert_eq!(rx.blocking_recv(), Ok(Answer::Always));
    assert_eq!(rx_ev.try_recv().unwrap(), Decided(Answer::Always, None));
}
