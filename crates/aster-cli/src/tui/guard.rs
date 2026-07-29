use std::panic;

/// Restores the terminal on drop so no code path (early `?`, panic, normal
/// exit) leaves the shell in raw mode. `restore` differs by UI: the review TUI
/// owns the alternate screen, chat only owns raw mode and an inline viewport.
pub(super) struct TuiGuard {
    restore: fn(),
}

impl TuiGuard {
    pub(super) fn install(restore: fn()) -> Self {
        let original = panic::take_hook();
        panic::set_hook(Box::new(move |info| {
            restore();
            original(info);
        }));
        Self { restore }
    }
}

impl Drop for TuiGuard {
    fn drop(&mut self) {
        (self.restore)();
        // Drop our hook so a later panic can't emit stray restore sequences.
        let _ = panic::take_hook();
    }
}
