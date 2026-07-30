//! TUI color theme. Set at startup, swapped at runtime (e.g. `/yolo`).

use ratatui::style::{Color, Modifier, Style};
use std::sync::RwLock;

use aster_policy::Mode;

static ACTIVE: RwLock<Theme> = RwLock::new(Theme::DEFAULT);

pub fn set(t: Theme) {
    if let Ok(mut w) = ACTIVE.write() {
        *w = t;
    }
}

/// Returns a snapshot of the active theme. Widgets that need multiple values
/// should call this once and bind it, so every span in one draw sees the same
/// palette even if the theme is swapped mid-frame.
pub fn get() -> Theme {
    ACTIVE.read().map(|r| r.clone()).unwrap_or(Theme::DEFAULT)
}

#[derive(Debug, Clone)]
#[allow(dead_code)] // palette fields are read selectively across TUI views
pub struct Theme {
    pub text: Color,
    pub dim: Color,
    pub dimmer: Color,
    pub faint: Color,
    pub accent: Color,
    pub error: Color,
    pub rail_bg: Color,
    pub pane_bg: Color,
    pub sel_bg: Color,
    pub amber: Color,
    pub blue: Color,
    pub purple: Color,
    pub add_bg: Color,
    pub add_fg: Color,
    pub add_mark: Color,
    pub del_bg: Color,
    pub del_fg: Color,
    pub del_mark: Color,
    pub inline_code_bg: Color,
    pub inline_code_fg: Color,
    pub heading_fg: Color,
    pub link_fg: Color,
    pub placeholder: Color,
    pub success: Color,
    pub warning: Color,
    pub severity_critical: Color,
    pub severity_high: Color,
    pub severity_medium: Color,
    pub severity_low: Color,
    pub severity_info: Color,
}

impl Theme {
    /// Current default palette, matching the aster-tui-spec mockup.
    pub const DEFAULT: Theme = Theme {
        text: Color::Rgb(0xc9, 0xc9, 0xc4),
        dim: Color::Rgb(0x8a, 0x8a, 0x85),
        dimmer: Color::Rgb(0x5a, 0x5a, 0x5a),
        faint: Color::Rgb(0x3f, 0x3f, 0x3f),
        accent: Color::Rgb(242, 118, 79),
        error: Color::Rgb(0xef, 0x5a, 0x6f),
        rail_bg: Color::Rgb(0x19, 0x19, 0x19),
        pane_bg: Color::Rgb(0x19, 0x19, 0x19),
        sel_bg: Color::Rgb(0x2a, 0x1a, 0x10),
        amber: Color::Rgb(0xf8, 0xcb, 0x66),
        blue: Color::Rgb(0x6c, 0xb6, 0xe3),
        purple: Color::Rgb(0xb4, 0x8c, 0xe3),
        add_bg: Color::Rgb(0x12, 0x24, 0x0f),
        add_fg: Color::Rgb(0x9e, 0xcb, 0x84),
        add_mark: Color::Rgb(0x5f, 0x8f, 0x4a),
        del_bg: Color::Rgb(0x2a, 0x15, 0x18),
        del_fg: Color::Rgb(0xe0, 0x8b, 0x8b),
        del_mark: Color::Rgb(0xa3, 0x4f, 0x4f),
        inline_code_bg: Color::Rgb(0x20, 0x20, 0x20),
        inline_code_fg: Color::Rgb(0xdd, 0xdd, 0xd8),
        heading_fg: Color::Rgb(0xdd, 0xdd, 0xd8),
        link_fg: Color::Rgb(0x6c, 0xb6, 0xe3),
        placeholder: Color::Rgb(0x4d, 0x4d, 0x4d),
        success: Color::Green,
        warning: Color::Yellow,
        severity_critical: Color::Red,
        severity_high: Color::LightRed,
        severity_medium: Color::Yellow,
        severity_low: Color::Blue,
        severity_info: Color::DarkGray,
    };

    /// Red-tinted theme for YOLO mode — everything gets a red cast so the user
    /// never forgets they are running without a sandbox.
    pub const YOLO: Theme = Theme {
        text: Color::Rgb(0xd4, 0xc4, 0xc4),
        dim: Color::Rgb(0x8a, 0x7a, 0x7a),
        dimmer: Color::Rgb(0x5a, 0x45, 0x45),
        faint: Color::Rgb(0x3f, 0x2a, 0x2a),
        accent: Color::Rgb(0xf0, 0x38, 0x38),
        error: Color::Rgb(0xf0, 0x38, 0x38),
        rail_bg: Color::Rgb(0x1a, 0x0a, 0x0a),
        pane_bg: Color::Rgb(0x1a, 0x0a, 0x0a),
        sel_bg: Color::Rgb(0x2a, 0x10, 0x10),
        amber: Color::Rgb(0xf0, 0x80, 0x40),
        blue: Color::Rgb(0x90, 0x90, 0xd0),
        purple: Color::Rgb(0xc0, 0x80, 0xd0),
        add_bg: Color::Rgb(0x1a, 0x1a, 0x0a),
        add_fg: Color::Rgb(0x9e, 0xcb, 0x84),
        add_mark: Color::Rgb(0x5f, 0x8f, 0x4a),
        del_bg: Color::Rgb(0x2a, 0x10, 0x10),
        del_fg: Color::Rgb(0xe0, 0x60, 0x60),
        del_mark: Color::Rgb(0xa3, 0x30, 0x30),
        inline_code_bg: Color::Rgb(0x20, 0x15, 0x15),
        inline_code_fg: Color::Rgb(0xdd, 0xd0, 0xd0),
        heading_fg: Color::Rgb(0xdd, 0xd0, 0xd0),
        link_fg: Color::Rgb(0x90, 0x90, 0xd0),
        placeholder: Color::Rgb(0x4d, 0x35, 0x35),
        success: Color::Rgb(0x7e, 0xab, 0x6a),
        warning: Color::Rgb(0xd0, 0xa0, 0x40),
        severity_critical: Color::Rgb(0xe0, 0x40, 0x40),
        severity_high: Color::Rgb(0xe0, 0x60, 0x60),
        severity_medium: Color::Rgb(0xd0, 0xa0, 0x40),
        severity_low: Color::Rgb(0x80, 0x80, 0xd0),
        severity_info: Color::Rgb(0x60, 0x40, 0x40),
    };

    #[allow(dead_code)]
    pub fn text_style(&self) -> Style {
        Style::default().fg(self.text)
    }

    pub fn dim_style(&self) -> Style {
        Style::default().fg(self.dim)
    }

    pub fn dimmer_style(&self) -> Style {
        Style::default().fg(self.dimmer)
    }

    pub fn faint_style(&self) -> Style {
        Style::default().fg(self.faint)
    }

    pub fn accent_style(&self) -> Style {
        Style::default().fg(self.accent)
    }

    pub fn accent_bold(&self) -> Style {
        Style::default()
            .fg(self.accent)
            .add_modifier(Modifier::BOLD)
    }

    #[allow(dead_code)]
    pub fn error_style(&self) -> Style {
        Style::default().fg(self.error)
    }

    pub fn amber_style(&self) -> Style {
        Style::default().fg(self.amber)
    }

    #[allow(dead_code)]
    pub fn blue_style(&self) -> Style {
        Style::default().fg(self.blue)
    }

    pub fn selected_style(&self) -> Style {
        Style::default().fg(self.accent).bg(self.sel_bg)
    }

    pub fn pane_bg_style(&self) -> Style {
        Style::default().bg(self.pane_bg)
    }

    #[allow(dead_code)]
    pub fn rail_bg_style(&self) -> Style {
        Style::default().bg(self.rail_bg)
    }

    pub fn code_style(&self) -> Style {
        Style::default()
            .fg(self.inline_code_fg)
            .bg(self.inline_code_bg)
    }

    #[allow(dead_code)]
    pub fn bold_style(&self) -> Style {
        Style::default().add_modifier(Modifier::BOLD)
    }

    #[allow(dead_code)]
    pub fn dark_gray_style(&self) -> Style {
        Style::default().fg(self.faint)
    }

    pub fn mode_color(&self, mode: Mode) -> Color {
        match mode {
            Mode::Edit => self.accent,
            Mode::Auto => self.accent,
            Mode::Manual => self.amber,
            Mode::Plan => self.dimmer,
            Mode::Yolo => self.error,
        }
    }

    #[allow(dead_code)]
    pub fn add_row_style(&self) -> Style {
        Style::default().fg(self.add_fg).bg(self.add_bg)
    }

    #[allow(dead_code)]
    pub fn del_row_style(&self) -> Style {
        Style::default().fg(self.del_fg).bg(self.del_bg)
    }
}
