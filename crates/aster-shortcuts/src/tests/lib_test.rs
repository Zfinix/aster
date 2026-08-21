use crate::{ShortcutsBackend, register_tools};

#[test]
fn registers_two_tools() {
    let tools = register_tools();
    assert_eq!(tools.len(), 2);
    assert_eq!(tools[0].id(), "shortcuts/list");
    assert_eq!(tools[1].id(), "shortcuts/run");
}

#[test]
fn backend_is_clone() {
    let b = ShortcutsBackend::new();
    let _ = b.clone();
}
