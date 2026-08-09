//! [`Renderable`] answers height and drawing from one object so they can never
//! disagree; [`Column`] stacks them and [`Insets`] replaces borders. This is
//! the entire layout system.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::{Paragraph, Widget, WidgetRef, Wrap};

pub(super) trait Renderable {
    fn render(&self, area: Rect, buf: &mut Buffer);
    fn desired_height(&self, width: u16) -> u16;
    /// Absolute cursor position when this renderable owns the caret.
    fn cursor_pos(&self, _area: Rect) -> Option<(u16, u16)> {
        None
    }
}

impl Renderable for Line<'static> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        WidgetRef::render_ref(self, area, buf);
    }
    fn desired_height(&self, _width: u16) -> u16 {
        1
    }
}

impl Renderable for Vec<Line<'static>> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        Paragraph::new(self.clone()).render(area, buf);
    }
    fn desired_height(&self, _width: u16) -> u16 {
        self.len() as u16
    }
}

impl Renderable for Paragraph<'static> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        self.render_ref(area, buf);
    }
    fn desired_height(&self, width: u16) -> u16 {
        self.line_count(width) as u16
    }
}

impl<R: Renderable> Renderable for Option<R> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        if let Some(r) = self {
            r.render(area, buf);
        }
    }
    fn desired_height(&self, width: u16) -> u16 {
        self.as_ref().map_or(0, |r| r.desired_height(width))
    }
    fn cursor_pos(&self, area: Rect) -> Option<(u16, u16)> {
        self.as_ref().and_then(|r| r.cursor_pos(area))
    }
}

/// Wraps a child and records the rect it was drawn into, so a later click can
/// be mapped back to it. Layout stays the layout system's business.
pub(super) struct Probe<'a, R> {
    inner: R,
    into: &'a std::cell::Cell<Option<Rect>>,
}

impl<'a, R: Renderable> Probe<'a, R> {
    pub(super) fn new(inner: R, into: &'a std::cell::Cell<Option<Rect>>) -> Self {
        Self { inner, into }
    }
}

impl<R: Renderable> Renderable for Probe<'_, R> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        self.into.set(Some(area));
        self.inner.render(area, buf);
    }
    fn desired_height(&self, width: u16) -> u16 {
        self.inner.desired_height(width)
    }
    fn cursor_pos(&self, area: Rect) -> Option<(u16, u16)> {
        self.inner.cursor_pos(area)
    }
}

/// Stacks children top to bottom, each getting its desired height.
pub(super) struct Column<'a> {
    children: Vec<Box<dyn Renderable + 'a>>,
}

impl<'a> Column<'a> {
    pub(super) fn new() -> Self {
        Self {
            children: Vec::new(),
        }
    }

    pub(super) fn push(&mut self, child: impl Renderable + 'a) {
        self.children.push(Box::new(child));
    }
}

impl Renderable for Column<'_> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        let mut y = area.y;
        for child in &self.children {
            let h = child.desired_height(area.width);
            let child_area = Rect::new(area.x, y, area.width, h).intersection(area);
            if !child_area.is_empty() {
                child.render(child_area, buf);
            }
            y = y.saturating_add(h);
        }
    }

    fn desired_height(&self, width: u16) -> u16 {
        self.children
            .iter()
            .map(|c| c.desired_height(width))
            .fold(0u16, u16::saturating_add)
    }

    /// The first child that claims the caret wins; at most one should.
    fn cursor_pos(&self, area: Rect) -> Option<(u16, u16)> {
        let mut y = area.y;
        for child in &self.children {
            let h = child.desired_height(area.width);
            let child_area = Rect::new(area.x, y, area.width, h).intersection(area);
            if !child_area.is_empty()
                && let Some(pos) = child.cursor_pos(child_area)
            {
                return Some(pos);
            }
            y = y.saturating_add(h);
        }
        None
    }
}

/// Draw a renderable inset from its area; the padding rows/columns count
/// toward its height.
pub(super) struct Inset<R> {
    inner: R,
    insets: Insets,
}

impl<R: Renderable> Inset<R> {
    pub(super) fn new(inner: R, insets: Insets) -> Self {
        Self { inner, insets }
    }
}

impl<R: Renderable> Renderable for Inset<R> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        self.inner.render(area.inset(self.insets), buf);
    }
    fn desired_height(&self, width: u16) -> u16 {
        let inner_width = width.saturating_sub(self.insets.left + self.insets.right);
        self.inner
            .desired_height(inner_width)
            .saturating_add(self.insets.top + self.insets.bottom)
    }
    fn cursor_pos(&self, area: Rect) -> Option<(u16, u16)> {
        self.inner.cursor_pos(area.inset(self.insets))
    }
}

/// A paragraph that wraps, reporting the wrapped height.
pub(super) fn wrapped(lines: Vec<Line<'static>>) -> Paragraph<'static> {
    Paragraph::new(lines).wrap(Wrap { trim: false })
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Insets {
    pub top: u16,
    pub left: u16,
    pub bottom: u16,
    pub right: u16,
}

impl Insets {
    pub(super) fn vh(v: u16, h: u16) -> Self {
        Self {
            top: v,
            left: h,
            bottom: v,
            right: h,
        }
    }

    pub(super) fn tlbr(top: u16, left: u16, bottom: u16, right: u16) -> Self {
        Self {
            top,
            left,
            bottom,
            right,
        }
    }
}

pub(super) trait RectExt {
    fn inset(&self, insets: Insets) -> Rect;
}

impl RectExt for Rect {
    fn inset(&self, insets: Insets) -> Rect {
        Rect {
            x: self.x.saturating_add(insets.left),
            y: self.y.saturating_add(insets.top),
            width: self
                .width
                .saturating_sub(insets.left.saturating_add(insets.right)),
            height: self
                .height
                .saturating_sub(insets.top.saturating_add(insets.bottom)),
        }
    }
}

#[cfg(test)]
#[path = "tests/render_test.rs"]
mod tests;
