use std::panic;

/// Restores the terminal on drop so no code path (early `?`, panic, normal
/// exit) leaves the shell in raw/alt-screen mode.
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
        // Drop our hook so a later panic can't emit stray restore sequences.
        let _ = panic::take_hook();
    }
}
