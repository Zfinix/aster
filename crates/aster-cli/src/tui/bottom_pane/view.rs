//! A modal view stacked over the composer; the top of the stack gets the keys.

use ratatui::crossterm::event::KeyEvent;

use crate::tui::render::Renderable;

pub(crate) trait BottomPaneView<E>: Renderable {
    fn handle_key(&mut self, key: KeyEvent);

    /// True once the view is finished and should be popped.
    fn is_complete(&self) -> bool;

    /// Click on `row`, counted from the view's own first row. `true` when the
    /// click landed on something; views that are not lists ignore it.
    fn handle_click(&mut self, _row: u16) -> bool {
        false
    }

    /// Wheel or trackpad scroll: negative is up. Default is no movement.
    fn handle_scroll(&mut self, _delta: isize) {}

    /// Absorb an approval into this view's queue; return it back if declined.
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
