//! The chat TUI palette, from the aster-tui-spec mockup.
//!
//! Four grays carry the hierarchy: body, label, tool row, meta. Coral is the
//! accent, amber means the agent is waiting on you, and the diff tints span
//! the row while marks sit a step darker than their text.

use ratatui::style::{Color, Style};

/// Body text (#c9c9c4).
pub(super) const TEXT: Color = Color::Rgb(0xc9, 0xc9, 0xc4);
/// Labels and arguments (#8a8a85).
pub(super) const DIM: Color = Color::Rgb(0x8a, 0x8a, 0x85);
/// Tool rows (#5a5a5a).
pub(super) const DIMMER: Color = Color::Rgb(0x5a, 0x5a, 0x5a);
/// Meta, hints, elisions (#3f3f3f).
pub(super) const FAINT: Color = Color::Rgb(0x3f, 0x3f, 0x3f);
/// The fill behind a user turn (#191919).
pub(super) const RAIL_BG: Color = Color::Rgb(0x19, 0x19, 0x19);
/// Errors (#ef5a6f).
pub(super) const ROSE: Color = Color::Rgb(0xef, 0x5a, 0x6f);
/// Paths and tool names (#6cb6e3).
pub(super) const BLUE: Color = Color::Rgb(0x6c, 0xb6, 0xe3);
/// Command names and keywords (#b48ce3).
pub(super) const PURPLE: Color = Color::Rgb(0xb4, 0x8c, 0xe3);
/// Waiting on the user (#f8cb66). The only colour that means that.
pub(super) const AMBER: Color = Color::Rgb(0xf8, 0xcb, 0x66);
/// Selected option row fill (#2a1a10); the text on it is coral.
pub(super) const SEL_BG: Color = Color::Rgb(0x2a, 0x1a, 0x10);

/// Added diff row (#12240f on #9ecb84), its `+` mark a step darker.
pub(super) const ADD_BG: Color = Color::Rgb(0x12, 0x24, 0x0f);
pub(super) const ADD_FG: Color = Color::Rgb(0x9e, 0xcb, 0x84);
pub(super) const ADD_MARK: Color = Color::Rgb(0x5f, 0x8f, 0x4a);
/// Removed diff row (#2a1518 on #e08b8b), its `-` mark a step darker.
pub(super) const DEL_BG: Color = Color::Rgb(0x2a, 0x15, 0x18);
pub(super) const DEL_FG: Color = Color::Rgb(0xe0, 0x8b, 0x8b);
pub(super) const DEL_MARK: Color = Color::Rgb(0xa3, 0x4f, 0x4f);

pub(super) fn dim() -> Style {
    Style::default().fg(DIM)
}

pub(super) fn dimmer() -> Style {
    Style::default().fg(DIMMER)
}

pub(super) fn faint() -> Style {
    Style::default().fg(FAINT)
}
