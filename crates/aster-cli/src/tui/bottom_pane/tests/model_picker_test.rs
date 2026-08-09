use super::*;

fn picker(count: usize) -> ModelPickerView {
    let models = (0..count).map(|i| format!("vendor/model-{i}")).collect();
    let (tx, _rx) = mpsc::unbounded_channel();
    let mut view = ModelPickerView::new("vendor/model-0", models, tx);
    view.query.clear();
    view
}

#[test]
fn height_does_not_grow_with_the_model_count() {
    let small = picker(4).desired_height(80);
    let huge = picker(500).desired_height(80);
    assert!(
        huge <= small + VISIBLE_ROWS as u16,
        "small {small}, huge {huge}"
    );
    assert!(huge < 20, "the pane would eat the screen: {huge}");
}

#[test]
fn the_window_follows_the_selection() {
    let mut view = picker(500);
    view.selected = 400;
    let shown = view.lines();
    assert!(
        shown.iter().any(|l| l.to_string().contains("model-400")),
        "selection scrolled off: {shown:?}"
    );
    assert!(shown.iter().any(|l| l.to_string().contains("+490 more")));
}

#[test]
fn the_window_stops_at_both_ends() {
    assert_eq!(window_start(0, 500), 0);
    assert_eq!(window_start(499, 500), 500 - VISIBLE_ROWS);
    assert_eq!(window_start(3, 4), 0);
}
