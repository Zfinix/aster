use super::*;

#[test]
fn modes_round_trip() {
    for (mode, id, _, _) in modes() {
        assert_eq!(mode_from_id(id), Some(mode));
        assert_eq!(mode_id(mode), id);
    }
}

#[test]
fn calls_track_order_and_completion() {
    let calls = Calls::default();
    calls.push(Call {
        id: "1".into(),
        name: "read".into(),
    });
    calls.push(Call {
        id: "2".into(),
        name: "edit".into(),
    });
    assert_eq!(calls.current().map(|call| call.id), Some("1".into()));
    assert_eq!(calls.name_of("2"), Some("edit".into()));
    calls.finish("1");
    assert_eq!(calls.current().map(|call| call.id), Some("2".into()));
}

#[test]
fn terminal_text_uses_crlf() {
    assert_eq!(crlf("one\ntwo\r\nthree"), "one\r\ntwo\r\nthree");
}
