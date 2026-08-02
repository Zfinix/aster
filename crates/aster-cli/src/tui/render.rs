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
mod tests {
    use super::*;
    use ratatui::text::Span;

    struct Fixed(u16, Option<(u16, u16)>);
    impl Renderable for Fixed {
        fn render(&self, _area: Rect, _buf: &mut Buffer) {}
        fn desired_height(&self, _width: u16) -> u16 {
            self.0
        }
        fn cursor_pos(&self, area: Rect) -> Option<(u16, u16)> {
            self.1.map(|(x, y)| (area.x + x, area.y + y))
        }
    }

    #[test]
    fn column_sums_heights_and_offsets_the_cursor() {
        let mut col = Column::new();
        col.push(Fixed(3, None));
        col.push(Fixed(2, Some((1, 1))));
        col.push(Fixed(4, None));
        assert_eq!(col.desired_height(80), 9);

        let area = Rect::new(0, 10, 80, 9);
        // The cursor child starts at y = 10 + 3, plus its own (1, 1) offset.
        assert_eq!(col.cursor_pos(area), Some((1, 14)));
    }

    #[test]
    fn insets_shrink_the_area_and_grow_the_height() {
        let r = Rect::new(0, 0, 20, 10);
        assert_eq!(r.inset(Insets::vh(1, 2)), Rect::new(2, 1, 16, 8));

        let inset = Inset::new(Fixed(3, None), Insets::tlbr(1, 0, 2, 0));
        assert_eq!(inset.desired_height(20), 6);
    }

    #[test]
    fn insets_never_underflow_a_small_rect() {
        let r = Rect::new(0, 0, 2, 1);
        let shrunk = r.inset(Insets::vh(2, 4));
        assert_eq!(shrunk.width, 0);
        assert_eq!(shrunk.height, 0);
    }

    #[test]
    fn wrapped_paragraph_reports_wrapped_height() {
        let p = wrapped(vec![Line::from(Span::raw("a ".repeat(30)))]);
        assert!(p.desired_height(20) > 1);
        assert_eq!(p.desired_height(200), 1);
    }
}
