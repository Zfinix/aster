use std::panic;

/// Restores the terminal and panic hook on drop, so no code path (early `?`,
/// panic, or normal exit) can leave the shell in raw/alt-screen mode.
pub(super) struct TuiGuard;

impl TuiGuard {
    pub(super) fn install() -> Self {
        let original = panic::take_hook();
        panic::set_hook(Box::new(move |info| {
            ratatui::restore();
            original(info);
        }));
        TuiGuard
    }
}

impl Drop for TuiGuard {
    fn drop(&mut self) {
        ratatui::restore();
        // Drop our hook so a later, unrelated panic does not emit stray restore
        // escape sequences to an already-restored terminal.
        let _ = panic::take_hook();
    }
}
