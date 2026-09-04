//! A modal view stacked over the composer; the top of the stack gets the keys.

use ratatui::crossterm::event::KeyEvent;

use crate::tui::render::Renderable;

pub(crate) trait BottomPaneView<E>: Renderable {
    fn handle_key(&mut self, key: KeyEvent);

    fn is_complete(&self) -> bool;

    fn handle_click(&mut self, _row: u16) -> bool {
        false
    }

    fn handle_scroll(&mut self, _delta: isize) {}

    fn try_consume_approval(
        &mut self,
        req: crate::chat::ApprovalRequest,
    ) -> Option<crate::chat::ApprovalRequest> {
        Some(req)
    }

    fn handle_paste(&mut self, _text: String) -> bool {
        false
    }
}
