use super::*;

#[test]
fn stricter_plan_beats_everything() {
    assert_eq!(Mode::Plan.stricter(Mode::Edit), Mode::Plan);
    assert_eq!(Mode::Edit.stricter(Mode::Plan), Mode::Plan);
    assert_eq!(Mode::Plan.stricter(Mode::Manual), Mode::Plan);
}

#[test]
fn stricter_manual_beats_auto_and_edit() {
    assert_eq!(Mode::Manual.stricter(Mode::Auto), Mode::Manual);
    assert_eq!(Mode::Edit.stricter(Mode::Manual), Mode::Manual);
    assert_eq!(Mode::Auto.stricter(Mode::Edit), Mode::Auto);
}

#[test]
fn stricter_same_mode_is_a_no_op() {
    assert_eq!(Mode::Edit.stricter(Mode::Edit), Mode::Edit);
    assert_eq!(Mode::Plan.stricter(Mode::Plan), Mode::Plan);
}

#[test]
fn yolo_is_least_strict() {
    assert_eq!(Mode::Yolo.stricter(Mode::Edit), Mode::Edit);
    assert_eq!(Mode::Yolo.stricter(Mode::Plan), Mode::Plan);
    assert_eq!(Mode::Edit.stricter(Mode::Yolo), Mode::Edit);
}

#[test]
fn deserializes_legacy_ask_and_deny_names() {
    assert_eq!(
        serde_json::from_str::<Mode>("\"ask\"").expect("ask parses"),
        Mode::Manual
    );
    assert_eq!(
        serde_json::from_str::<Mode>("\"deny\"").expect("deny parses"),
        Mode::Plan
    );
}
