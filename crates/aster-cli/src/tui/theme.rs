//! TUI color theme. Set at startup, swapped at runtime (e.g. `/yolo`).
//! Theme transitions interpolate colours over 450 ms so the shift is visible
//! but not jarring.

use std::sync::RwLock;
use std::time::{Duration, Instant};

use ratatui::style::{Color, Modifier, Style};

use aster_policy::Mode;

#[derive(Debug, Clone)]
struct ThemeState {
    current: Theme,
    target: Theme,
    transition_start: Option<Instant>,
}

impl ThemeState {
    const fn stable(theme: Theme) -> Self {
        Self {
            current: theme,
            target: theme,
            transition_start: None,
        }
    }

    fn get_interpolated(&self) -> Theme {
        match self.transition_start {
            Some(start) => {
                let t = (start.elapsed().as_secs_f32() / TRANSITION.as_secs_f32()).min(1.0);
                self.current.lerp(&self.target, overshoot(t))
            }
            None => self.target,
        }
    }
}

/// How long a theme swap takes to settle. Zero under test: the palette is a
/// process-global, so a transition in one test would bleed colours into every
/// other test running beside it.
#[cfg(not(test))]
const TRANSITION: Duration = Duration::from_millis(450);
#[cfg(test)]
const TRANSITION: Duration = Duration::ZERO;

/// Back-out ease: races past the target and settles back, so the swap reads as
/// a flash rather than a fade nobody notices in a four-row viewport.
fn overshoot(t: f32) -> f32 {
    const C: f32 = 2.4;
    let p = t - 1.0;
    1.0 + p * p * ((C + 1.0) * p + C)
}

static ACTIVE: RwLock<ThemeState> = RwLock::new(ThemeState::stable(Theme::DEFAULT));

/// Gradient stops for the YOLO takeover animation: bright at the shockwave
/// front, ember at the tail. Entering runs hot red; leaving settles back to the
/// default accent family.
pub fn takeover_palette(entering: bool) -> (Color, Color, Color, Color) {
    if entering {
        (
            Color::Rgb(0xff, 0x45, 0x45),
            Color::Rgb(0xf0, 0x38, 0x38),
            Color::Rgb(0x8a, 0x1f, 0x1f),
            Color::Rgb(0x4a, 0x12, 0x12),
        )
    } else {
        (
            Color::Rgb(0xf8, 0xcb, 0x66),
            Color::Rgb(0xf2, 0x76, 0x4f),
            Color::Rgb(0x8a, 0x4a, 0x2a),
            Color::Rgb(0x3f, 0x2a, 0x1a),
        )
    }
}

pub fn set(t: Theme) {
    let mut state = ACTIVE.write().unwrap();
    if state.target.same_theme(&t) {
        return;
    }
    // Snapshot the current interpolated colour so the next lerp picks up
    // from wherever the animation already is.
    state.current = state.get_interpolated();
    state.target = t;
    state.transition_start = Some(Instant::now());
}

/// End any transition now. Callers that repaint the whole screen use this so
/// the blocks they rebuild are written in the final palette, not a blend of the
/// way there.
pub fn settle() {
    let mut state = ACTIVE.write().unwrap();
    state.current = state.target;
    state.transition_start = None;
}

/// Returns a snapshot of the active theme. When a transition is in flight
/// the returned theme is a blend of the source and target palettes.
pub fn get() -> Theme {
    ACTIVE
        .read()
        .map(|r| r.get_interpolated())
        .unwrap_or(Theme::DEFAULT)
}

/// True while a theme transition is animating. Callers can use this to
/// request extra frames until the transition settles.
pub fn is_transitioning() -> bool {
    ACTIVE
        .read()
        .map(|r| r.transition_start.is_some_and(|s| s.elapsed() < TRANSITION))
        .unwrap_or(false)
}

#[derive(Debug, Clone, Copy)]
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
    /// Row colors of the Aster mark, top to bottom.
    pub mark: [Color; 10],
}

impl Theme {
    /// Current default palette, matching the aster-tui-spec mockup.
    pub const DEFAULT: Theme = Theme {
        text: Color::Rgb(0xff, 0xff, 0xff),
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
        success: Color::Rgb(0x7e, 0xab, 0x6a),
        warning: Color::Rgb(0xf8, 0xcb, 0x66),
        severity_critical: Color::Rgb(0xef, 0x5a, 0x6f),
        severity_high: Color::Rgb(0xe0, 0x6a, 0x6a),
        severity_medium: Color::Rgb(0xf8, 0xcb, 0x66),
        severity_low: Color::Rgb(0x6c, 0xb6, 0xe3),
        severity_info: Color::Rgb(0x8a, 0x8a, 0x85),
        mark: [
            Color::Rgb(239, 90, 111),
            Color::Rgb(239, 90, 111),
            Color::Rgb(241, 110, 79),
            Color::Rgb(243, 130, 79),
            Color::Rgb(245, 152, 78),
            Color::Rgb(246, 168, 84),
            Color::Rgb(247, 182, 89),
            Color::Rgb(247, 193, 95),
            Color::Rgb(248, 203, 102),
            Color::Rgb(248, 203, 102),
        ],
    };

    /// Light palette for pale terminals: warm paper background, dark text, the
    /// same accent family darkened enough to read on white.
    pub const LIGHT: Theme = Theme {
        text: Color::Rgb(0x1a, 0x1a, 0x1a),
        dim: Color::Rgb(0x6a, 0x6a, 0x66),
        dimmer: Color::Rgb(0x9a, 0x9a, 0x96),
        faint: Color::Rgb(0xb8, 0xb4, 0xae),
        accent: Color::Rgb(0xc2, 0x5a, 0x30),
        error: Color::Rgb(0xc0, 0x3a, 0x50),
        rail_bg: Color::Rgb(0xf5, 0xf2, 0xee),
        pane_bg: Color::Rgb(0xf5, 0xf2, 0xee),
        sel_bg: Color::Rgb(0xf0, 0xdf, 0xd2),
        amber: Color::Rgb(0x9a, 0x6d, 0x14),
        blue: Color::Rgb(0x2a, 0x6f, 0x9e),
        purple: Color::Rgb(0x74, 0x4a, 0xb8),
        add_bg: Color::Rgb(0xe2, 0xee, 0xda),
        add_fg: Color::Rgb(0x3f, 0x6b, 0x2a),
        add_mark: Color::Rgb(0x86, 0xa8, 0x72),
        del_bg: Color::Rgb(0xf4, 0xdc, 0xdc),
        del_fg: Color::Rgb(0xa3, 0x3a, 0x3a),
        del_mark: Color::Rgb(0xc0, 0x84, 0x84),
        inline_code_bg: Color::Rgb(0xe8, 0xe4, 0xde),
        inline_code_fg: Color::Rgb(0x33, 0x30, 0x2c),
        heading_fg: Color::Rgb(0x33, 0x30, 0x2c),
        link_fg: Color::Rgb(0x2a, 0x6f, 0x9e),
        placeholder: Color::Rgb(0xb0, 0xac, 0xa6),
        success: Color::Rgb(0x4f, 0x7a, 0x3e),
        warning: Color::Rgb(0x9a, 0x6d, 0x14),
        severity_critical: Color::Rgb(0xc0, 0x3a, 0x50),
        severity_high: Color::Rgb(0xb8, 0x4a, 0x4a),
        severity_medium: Color::Rgb(0x9a, 0x6d, 0x14),
        severity_low: Color::Rgb(0x2a, 0x6f, 0x9e),
        severity_info: Color::Rgb(0x6a, 0x6a, 0x66),
        mark: [
            Color::Rgb(0xc2, 0x3a, 0x50),
            Color::Rgb(0xc2, 0x3a, 0x50),
            Color::Rgb(0xc2, 0x4a, 0x34),
            Color::Rgb(0xc2, 0x5a, 0x30),
            Color::Rgb(0xc2, 0x66, 0x30),
            Color::Rgb(0xc0, 0x70, 0x36),
            Color::Rgb(0xbc, 0x7a, 0x3c),
            Color::Rgb(0xb8, 0x82, 0x42),
            Color::Rgb(0xb4, 0x88, 0x48),
            Color::Rgb(0xb4, 0x88, 0x48),
        ],
    };

    /// Red-tinted theme for YOLO mode — everything gets a red cast so the
    /// user never forgets they are running without a sandbox.
    pub const YOLO: Theme = Theme {
        text: Color::Rgb(0xff, 0xff, 0xff),
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
        mark: [Color::Rgb(0xf0, 0x38, 0x38); 10],
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

    fn same_theme(&self, other: &Theme) -> bool {
        self.text == other.text
            && self.dim == other.dim
            && self.dimmer == other.dimmer
            && self.faint == other.faint
            && self.accent == other.accent
    }

    fn lerp_color(a: Color, b: Color, t: f32) -> Color {
        match (a, b) {
            (Color::Rgb(ar, ag, ab), Color::Rgb(br, bg, bb)) => Color::Rgb(
                ((ar as f32) + ((br as f32) - (ar as f32)) * t).round() as u8,
                ((ag as f32) + ((bg as f32) - (ag as f32)) * t).round() as u8,
                ((ab as f32) + ((bb as f32) - (ab as f32)) * t).round() as u8,
            ),
            _ => {
                if t < 0.5 {
                    a
                } else {
                    b
                }
            }
        }
    }

    fn lerp(&self, target: &Theme, t: f32) -> Theme {
        Theme {
            text: Self::lerp_color(self.text, target.text, t),
            dim: Self::lerp_color(self.dim, target.dim, t),
            dimmer: Self::lerp_color(self.dimmer, target.dimmer, t),
            faint: Self::lerp_color(self.faint, target.faint, t),
            accent: Self::lerp_color(self.accent, target.accent, t),
            error: Self::lerp_color(self.error, target.error, t),
            rail_bg: Self::lerp_color(self.rail_bg, target.rail_bg, t),
            pane_bg: Self::lerp_color(self.pane_bg, target.pane_bg, t),
            sel_bg: Self::lerp_color(self.sel_bg, target.sel_bg, t),
            amber: Self::lerp_color(self.amber, target.amber, t),
            blue: Self::lerp_color(self.blue, target.blue, t),
            purple: Self::lerp_color(self.purple, target.purple, t),
            add_bg: Self::lerp_color(self.add_bg, target.add_bg, t),
            add_fg: Self::lerp_color(self.add_fg, target.add_fg, t),
            add_mark: Self::lerp_color(self.add_mark, target.add_mark, t),
            del_bg: Self::lerp_color(self.del_bg, target.del_bg, t),
            del_fg: Self::lerp_color(self.del_fg, target.del_fg, t),
            del_mark: Self::lerp_color(self.del_mark, target.del_mark, t),
            inline_code_bg: Self::lerp_color(self.inline_code_bg, target.inline_code_bg, t),
            inline_code_fg: Self::lerp_color(self.inline_code_fg, target.inline_code_fg, t),
            heading_fg: Self::lerp_color(self.heading_fg, target.heading_fg, t),
            link_fg: Self::lerp_color(self.link_fg, target.link_fg, t),
            placeholder: Self::lerp_color(self.placeholder, target.placeholder, t),
            success: Self::lerp_color(self.success, target.success, t),
            warning: Self::lerp_color(self.warning, target.warning, t),
            severity_critical: Self::lerp_color(
                self.severity_critical,
                target.severity_critical,
                t,
            ),
            severity_high: Self::lerp_color(self.severity_high, target.severity_high, t),
            severity_medium: Self::lerp_color(self.severity_medium, target.severity_medium, t),
            severity_low: Self::lerp_color(self.severity_low, target.severity_low, t),
            severity_info: Self::lerp_color(self.severity_info, target.severity_info, t),
            mark: [
                Self::lerp_color(self.mark[0], target.mark[0], t),
                Self::lerp_color(self.mark[1], target.mark[1], t),
                Self::lerp_color(self.mark[2], target.mark[2], t),
                Self::lerp_color(self.mark[3], target.mark[3], t),
                Self::lerp_color(self.mark[4], target.mark[4], t),
                Self::lerp_color(self.mark[5], target.mark[5], t),
                Self::lerp_color(self.mark[6], target.mark[6], t),
                Self::lerp_color(self.mark[7], target.mark[7], t),
                Self::lerp_color(self.mark[8], target.mark[8], t),
                Self::lerp_color(self.mark[9], target.mark[9], t),
            ],
        }
    }
}
