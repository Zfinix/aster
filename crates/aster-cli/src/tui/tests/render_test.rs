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
