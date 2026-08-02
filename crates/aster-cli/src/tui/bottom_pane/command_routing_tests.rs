//! Enter-key routing: slash commands run as commands, but a dragged-in
//! absolute path (which also starts with `/`) submits as a message.

use super::*;
use ratatui::crossterm::event::KeyModifiers;
use tokio::sync::mpsc;

const COMMANDS: &[CommandDesc] = &[
    CommandDesc {
        name: "model",
        takes_arg: true,
        desc: "pick a model",
    },
    CommandDesc {
        name: "quit",
        takes_arg: false,
        desc: "exit",
    },
];

fn pane() -> BottomPane<()> {
    let (tx, _rx) = mpsc::unbounded_channel();
    BottomPane::new(
        COMMANDS,
        "hint",
        FrameRequester::noop(),
        tx,
        |_, _| (),
        |_| (),
    )
}

fn enter() -> KeyEvent {
    KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)
}

fn type_and_enter(pane: &mut BottomPane<()>, text: &str) -> InputResult {
    for ch in text.chars() {
        pane.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE), 80);
    }
    pane.handle_key(enter(), 80)
}

#[test]
fn a_known_command_runs_as_a_command() {
    let mut p = pane();
    match type_and_enter(&mut p, "/quit") {
        InputResult::Command(cmd) => assert_eq!(cmd, "quit"),
        _ => panic!("expected a command"),
    }
}

#[test]
fn a_command_with_args_runs_as_a_command() {
    let mut p = pane();
    match type_and_enter(&mut p, "/model gpt-4o") {
        InputResult::Command(cmd) => assert_eq!(cmd, "model gpt-4o"),
        _ => panic!("expected a command"),
    }
}

#[test]
fn a_dragged_absolute_path_submits_as_a_message() {
    let mut p = pane();
    let path = "/Users/chizi/Desktop/Screen\\ Recording\\ 2026-08-01\\ AM.mov";
    match type_and_enter(&mut p, path) {
        InputResult::Submitted { text, refs } => {
            assert!(text.contains("[@"), "path should fold to a ref: {text}");
            assert_eq!(refs.len(), 1);
            assert_eq!(
                refs[0].1,
                "/Users/chizi/Desktop/Screen Recording 2026-08-01 AM.mov"
            );
        }
        _ => panic!("expected a submission"),
    }
}

#[test]
fn an_unknown_slash_word_submits_as_a_message() {
    let mut p = pane();
    match type_and_enter(&mut p, "/bogus hi") {
        InputResult::Submitted { text, .. } => assert_eq!(text, "/bogus hi"),
        _ => panic!("expected a submission"),
    }
}

/// Every command in the real chat menu runs when typed in full.
#[test]
fn every_chat_command_runs_when_typed_in_full() {
    for c in crate::tui::chat::CHAT_COMMANDS {
        let (tx, _rx) = mpsc::unbounded_channel::<()>();
        let mut p: BottomPane<()> = BottomPane::new(
            crate::tui::chat::CHAT_COMMANDS,
            "hint",
            FrameRequester::noop(),
            tx,
            |_, _| (),
            |_| (),
        );
        let typed = format!("/{}", c.name);
        match type_and_enter(&mut p, &typed) {
            InputResult::Command(cmd) => assert_eq!(cmd, c.name, "typed {typed}"),
            InputResult::Submitted { text, .. } => panic!("{typed} submitted as message: {text}"),
            _ => panic!("{typed} did nothing"),
        }
    }
}

/// A bare `/` with the menu open ran as a message, which is how a lone "/"
/// ended up being sent to the model.
#[test]
fn a_bare_slash_runs_the_highlighted_command() {
    let mut p = pane();
    match type_and_enter(&mut p, "/") {
        InputResult::Command(cmd) => assert_eq!(cmd, "model"),
        InputResult::Submitted { text, .. } => panic!("submitted {text:?} instead of running"),
        _ => panic!("expected the highlighted command"),
    }
}

/// Arrowing through the menu changes nothing in the draft, so enter has to
/// read the highlight rather than the text.
#[test]
fn enter_runs_the_row_the_arrows_landed_on() {
    let mut p = pane();
    p.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE), 80);
    p.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE), 80);
    match p.handle_key(enter(), 80) {
        InputResult::Command(cmd) => assert_eq!(cmd, "quit"),
        InputResult::Submitted { text, .. } => panic!("submitted {text:?} instead of running"),
        _ => panic!("expected the second command"),
    }
}

/// A prefix runs the highlighted match, so `/qu` is `/quit`.
#[test]
fn a_prefix_runs_the_matching_command() {
    let mut p = pane();
    match type_and_enter(&mut p, "/qu") {
        InputResult::Command(cmd) => assert_eq!(cmd, "quit"),
        _ => panic!("expected a command"),
    }
}

/// The composer is emptied, so the next message does not carry the command.
#[test]
fn running_a_command_clears_the_draft() {
    let mut p = pane();
    type_and_enter(&mut p, "/");
    assert!(p.composer.is_empty(), "draft: {:?}", p.composer.text());
}

mod mouse {
    use super::*;
    use ratatui::buffer::Buffer;
    use ratatui::crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    use ratatui::layout::Rect;

    fn click(col: u16, row: u16) -> MouseEvent {
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: col,
            row,
            modifiers: KeyModifiers::NONE,
        }
    }

    /// Areas are recorded during rendering, so a click has to follow a draw.
    fn draw(p: &BottomPane<()>) {
        let area = Rect::new(0, 0, 80, p.desired_height(80).max(1));
        let mut buf = Buffer::empty(area);
        p.render(area, &mut buf);
    }

    #[test]
    fn the_mouse_is_captured_only_while_something_is_clickable() {
        let mut p = pane();
        assert!(!p.wants_mouse(), "idle composer needs no mouse");
        p.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE), 80);
        assert!(p.wants_mouse(), "the slash menu is clickable");
    }

    #[test]
    fn clicking_a_menu_row_runs_that_command() {
        let mut p = pane();
        p.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE), 80);
        draw(&p);
        let area = p.menu_area.get().expect("the menu drew somewhere");
        match p.handle_mouse(click(area.x + 2, area.y + 1)) {
            InputResult::Command(cmd) => assert_eq!(cmd, "quit"),
            _ => panic!("the second row is /quit"),
        }
    }

    #[test]
    fn clicking_outside_the_menu_changes_nothing() {
        let mut p = pane();
        p.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE), 80);
        draw(&p);
        let area = p.menu_area.get().expect("the menu drew somewhere");
        match p.handle_mouse(click(area.x, area.y + area.height + 3)) {
            InputResult::None => assert_eq!(p.composer.text(), "/"),
            _ => panic!("a click below the menu is not a command"),
        }
    }

    #[test]
    fn scrolling_the_menu_moves_the_highlight() {
        let mut p = pane();
        p.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE), 80);
        draw(&p);
        let mut wheel = click(1, 1);
        wheel.kind = MouseEventKind::ScrollDown;
        p.handle_mouse(wheel);
        assert_eq!(p.menu_sel, 1);
    }
}
