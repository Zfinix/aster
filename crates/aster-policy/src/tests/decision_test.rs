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
fn deserializes_every_mode_name() {
    for (text, mode) in [
        ("\"plan\"", Mode::Plan),
        ("\"manual\"", Mode::Manual),
        ("\"auto\"", Mode::Auto),
        ("\"edit\"", Mode::Edit),
        ("\"yolo\"", Mode::Yolo),
    ] {
        assert_eq!(serde_json::from_str::<Mode>(text).expect(text), mode);
    }
}

/// The names dropped in the collapse. A stale config should stop the run
/// rather than silently resolve to something else.
#[test]
fn the_retired_mode_names_no_longer_parse() {
    for text in ["\"ask\"", "\"deny\""] {
        assert!(serde_json::from_str::<Mode>(text).is_err(), "{text}");
    }
}
