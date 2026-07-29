//! A modal view stacked over the composer; the top of the stack gets the keys.

use ratatui::crossterm::event::KeyEvent;

use crate::tui::render::Renderable;

pub(crate) trait BottomPaneView<E>: Renderable {
    fn handle_key(&mut self, key: KeyEvent);

    /// True once the view is finished and should be popped.
    fn is_complete(&self) -> bool;

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
