//! Minimal inline-viewport terminal, derived from ratatui's `Terminal` (MIT, ©
//! Ratatui Developers). The cursor is queried once at startup and never
//! re-anchored, and finished lines go into the terminal's own scrollback.

use std::io::{self, Stdout};

use anyhow::Result;
use ratatui::backend::{Backend, CrosstermBackend};
use ratatui::buffer::{Buffer, Cell};
use ratatui::layout::{Position, Rect, Size};
use unicode_width::UnicodeWidthStr;

type CBackend = CrosstermBackend<Stdout>;

/// What one draw call renders into; the caller sets the caret through it.
pub(super) struct Frame<'a> {
    area: Rect,
    buf: &'a mut Buffer,
    cursor: &'a mut Option<Position>,
}

impl Frame<'_> {
    pub(super) fn area(&self) -> Rect {
        self.area
    }

    pub(super) fn buffer_mut(&mut self) -> &mut Buffer {
        self.buf
    }

    pub(super) fn set_cursor_position(&mut self, pos: Position) {
        *self.cursor = Some(pos);
    }
}

pub(super) struct InlineTerm {
    backend: CBackend,
    buffers: [Buffer; 2],
    current: usize,
    viewport: Rect,
    screen: Size,
    /// Blank rows a shrinking pane opened at the top of the screen. They are
    /// padding, not transcript, so the next upward scroll drops them instead
    /// of filing them in scrollback.
    blank_top: u16,
}

impl InlineTerm {
    /// Queries the cursor exactly once to anchor the viewport; must run before
    /// a crossterm `EventStream` exists or the reply is swallowed.
    pub(super) fn new(height: u16) -> Result<Self> {
        let mut backend = CrosstermBackend::new(io::stdout());
        let screen = backend.size()?;
        let height = clamp_height(height, screen);
        let pos = backend.get_cursor_position()?;

        // Open room below the cursor so the viewport fits on screen.
        let below = height.saturating_sub(1);
        backend.append_lines(below)?;
        let overflow = (pos.y + height).saturating_sub(screen.height);
        let top = pos.y - overflow.min(pos.y);

        let viewport = Rect::new(0, top, screen.width, height);
        Ok(Self {
            backend,
            buffers: [Buffer::empty(viewport), Buffer::empty(viewport)],
            current: 0,
            viewport,
            screen,
            blank_top: 0,
        })
    }

    pub(super) fn width(&self) -> u16 {
        self.screen.width.clamp(20, 240)
    }

    pub(super) fn viewport_top(&self) -> u16 {
        self.viewport.y
    }

    /// Render a frame and flush the difference from the previous one.
    pub(super) fn draw(&mut self, render: impl FnOnce(&mut Frame)) -> Result<()> {
        let mut cursor = None;
        self.buffers[self.current].reset();
        render(&mut Frame {
            area: self.viewport,
            buf: &mut self.buffers[self.current],
            cursor: &mut cursor,
        });

        let previous = &self.buffers[1 - self.current];
        let updates = previous.diff(&self.buffers[self.current]);
        self.backend.draw(updates.into_iter())?;
        match cursor {
            Some(pos) => {
                self.backend.set_cursor_position(pos)?;
                self.backend.show_cursor()?;
            }
            None => self.backend.hide_cursor()?,
        }
        self.backend.flush()?;
        self.current = 1 - self.current;
        Ok(())
    }

    /// Move the viewport boundary without touching scrollback or the cursor.
    /// The transcript above moves with the boundary in both directions, so a
    /// pane that grows and shrinks again leaves the screen as it found it.
    pub(super) fn set_height(&mut self, height: u16) -> Result<()> {
        let height = clamp_height(height, self.screen);
        let old = self.viewport;
        if height == old.height {
            return Ok(());
        }

        let (top, shift) = reflow(old, height, self.screen);
        match shift {
            Shift::None => {}
            Shift::Up(rows) => self.scroll_into_scrollback(old.y, rows)?,
            Shift::Down(rows) => self.reclaim_above(top, rows)?,
        }
        // Rows the viewport gave up below it, which only exists while it still
        // floats above the screen bottom.
        if top + height < old.bottom() {
            self.clear_rows(top + height..old.bottom())?;
        }

        self.viewport = Rect::new(0, top, self.screen.width, height);
        self.buffers = [Buffer::empty(self.viewport), Buffer::empty(self.viewport)];
        // The screen under the new viewport is stale; clear it so the next
        // draw's diff-from-empty repaints everything.
        self.clear_rows(self.viewport.y..self.viewport.bottom())?;
        Ok(())
    }

    /// Insert finished lines above the viewport, into scrollback. Ratatui's
    /// scrolling-regions algorithm, on our own tracked state.
    pub(super) fn insert_history(&mut self, cells: &Buffer) -> Result<()> {
        let mut remaining: &[Cell] = &cells.content;
        let stride = cells.area.width;
        let mut height = cells.area.height;

        // If the viewport floats above the bottom, push it down first.
        if self.viewport.bottom() < self.screen.height {
            let to_draw = height.min(self.screen.height - self.viewport.bottom());
            self.backend.scroll_region_down(
                self.viewport.top()..self.viewport.bottom() + to_draw,
                to_draw,
            )?;
            remaining = self.draw_cleared(self.viewport.top(), to_draw, stride, remaining)?;
            self.viewport.y += to_draw;
            for buf in &mut self.buffers {
                buf.area.y = self.viewport.y;
            }
            height -= to_draw;
        }

        let top = self.viewport.top();
        while height > 0 && top > 0 {
            let to_draw = height.min(top);
            self.scroll_into_scrollback(top, to_draw)?;
            remaining = self.draw_cleared(top - to_draw, to_draw, stride, remaining)?;
            height -= to_draw;
        }
        self.backend.flush()?;
        Ok(())
    }

    /// Open `rows` blank rows below the region above the viewport, handing what
    /// leaves the top to scrollback via line feeds at the bottom margin.
    fn scroll_into_scrollback(&mut self, region_bottom: u16, rows: u16) -> Result<()> {
        use std::io::Write;
        if region_bottom == 0 || rows == 0 {
            return Ok(());
        }
        // Region and cursor address are 1-based, so `region_bottom` names the
        // region's last row, which is the row above the viewport.
        write!(self.backend, "\x1b[1;{region_bottom}r")?;
        // Padding a shrinking pane left behind is not history. `DL` removes it
        // in place; terminals like tmux and xterm.js would file an `SU` row
        // into scrollback, which is how blank runs used to litter transcripts.
        let discarded = rows.min(self.blank_top);
        if discarded > 0 {
            write!(self.backend, "\x1b[1;1H\x1b[{discarded}M")?;
            self.blank_top -= discarded;
        }
        write!(self.backend, "\x1b[{region_bottom};1H")?;
        for _ in 0..rows - discarded {
            write!(self.backend, "\r\n")?;
        }
        write!(self.backend, "\x1b[r")?;
        Ok(())
    }

    /// Move the transcript back down into the rows a shrinking pane gave up, the
    /// inverse of [`Self::scroll_into_scrollback`]. Scrollback cannot be pulled back,
    /// so the rows opened at the top are blank and counted, not filed as history.
    fn reclaim_above(&mut self, region_bottom: u16, rows: u16) -> Result<()> {
        if region_bottom == 0 || rows == 0 {
            return Ok(());
        }
        self.backend.scroll_region_down(0..region_bottom, rows)?;
        self.blank_top = (self.blank_top + rows).min(region_bottom);
        Ok(())
    }

    /// Wipe the screen and scrollback and re-anchor at the top, for `/clear`.
    /// The stale diff buffers are dropped so the next draw repaints the pane.
    pub(super) fn clear_all(&mut self) -> Result<()> {
        use ratatui::crossterm::cursor::MoveTo;
        use ratatui::crossterm::execute;
        use ratatui::crossterm::terminal::{Clear, ClearType};
        execute!(
            io::stdout(),
            Clear(ClearType::All),
            Clear(ClearType::Purge),
            MoveTo(0, 0),
        )?;
        self.viewport = Rect::new(0, 0, self.screen.width, self.viewport.height);
        self.buffers = [Buffer::empty(self.viewport), Buffer::empty(self.viewport)];
        self.blank_top = 0;
        Ok(())
    }

    /// The terminal was resized: the screen rewrapped under us, dragging the
    /// old pane image with it. Repaint where the pane belongs and clear every
    /// row its old image can now occupy; anything less leaves pane fragments
    /// littered through the transcript.
    pub(super) fn resized(&mut self) -> Result<()> {
        let old = self.viewport;
        let old_screen = self.screen;
        self.screen = self.backend.size()?;
        let (top, clear_from) = reflow_on_resize(old, old_screen, self.screen);
        let height = clamp_height(old.height, self.screen);

        self.viewport = Rect::new(0, top, self.screen.width, height);
        self.buffers = [Buffer::empty(self.viewport), Buffer::empty(self.viewport)];
        // Where the padding ended up after the rewrap is a guess. Forget it
        // rather than discard a row of real transcript.
        self.blank_top = 0;
        self.clear_rows(clear_from..self.screen.height)?;
        Ok(())
    }

    /// Draw cells onto rows known to be blank (just scrolled clear). `stride`
    /// is the source buffer's row width, which the screen may exceed.
    fn draw_cleared<'a>(
        &mut self,
        y: u16,
        rows: u16,
        stride: u16,
        cells: &'a [Cell],
    ) -> Result<&'a [Cell]> {
        let width = stride as usize;
        let take = (width * rows as usize).min(cells.len());
        let (to_draw, rest) = cells.split_at(take);
        let iter = to_draw
            .iter()
            .enumerate()
            .filter(keeps_cell(width))
            .map(|(i, c)| ((i % width) as u16, y + (i / width) as u16, c));
        self.backend.draw(iter)?;
        Ok(rest)
    }

    fn clear_rows(&mut self, rows: std::ops::Range<u16>) -> Result<()> {
        if rows.is_empty() {
            return Ok(());
        }
        let area = Rect::new(0, rows.start, self.screen.width, rows.end - rows.start);
        let blank = Buffer::empty(area);
        let stale = Buffer::filled(area, Cell::new("?"));
        self.backend.draw(stale.diff(&blank).into_iter())?;
        Ok(())
    }
}

/// Which cells of a rendered buffer are real glyphs, not the blank continuation
/// cell a wide grapheme leaves behind (ratatui stores a literal space there).
/// Printing those spaces pushes every following column one cell right, so a run
/// of CJK or emoji would come out as `字 字 字`. The filter tracks the columns a
/// wide grapheme already covers and drops its continuation cells; the column-0
/// reset keeps a wide grapheme that lands in a row's last column from eating
/// the first cell of the next row.
fn keeps_cell(width: usize) -> impl FnMut(&(usize, &Cell)) -> bool {
    let mut skip = 0usize;
    move |(i, cell)| {
        if *i % width == 0 {
            skip = 0;
        }
        if skip > 0 {
            skip -= 1;
            false
        } else {
            skip = cell.symbol().width().saturating_sub(1);
            true
        }
    }
}

/// What the rows above the viewport do when its boundary moves.
#[derive(Debug, PartialEq, Eq)]
enum Shift {
    None,
    /// Transcript scrolls up, the top rows passing into scrollback.
    Up(u16),
    /// Transcript scrolls back down into the rows the pane gave up.
    Down(u16),
}

/// Where the viewport lands at `height`, and what the transcript above does to get
/// there. Growing takes free rows below before any from the transcript, and
/// shrinking hands them back the same way, leaving no band of dead rows.
fn reflow(old: Rect, height: u16, screen: Size) -> (u16, Shift) {
    if height == old.height {
        return (old.y, Shift::None);
    }
    if height < old.height {
        // Only a bottom-anchored pane took rows from the transcript to grow,
        // so only a bottom-anchored pane has any to give back.
        if old.bottom() >= screen.height {
            let top = old.bottom() - height;
            return (top, Shift::Down(top - old.y));
        }
        return (old.y, Shift::None);
    }
    let delta = height - old.height;
    let room_below = screen.height.saturating_sub(old.bottom());
    let need_above = delta.saturating_sub(room_below).min(old.y);
    (old.y - need_above, Shift::Up(need_above))
}

/// Where the viewport lands after a terminal resize, and the first row to
/// clear. Painted rows carry their padding, so on a narrowing each old pane
/// row rewraps into exactly `ceil(old_w / new_w)` rows ending at the screen
/// bottom; on a widening the unwrap above shrinks, lifting the old image to
/// at best `old.y * old_w / new_w`. Clearing from the lower of the two bounds
/// covers the old image whichever way the width moved.
fn reflow_on_resize(old: Rect, old_screen: Size, screen: Size) -> (u16, u16) {
    let height = clamp_height(old.height, screen);
    let (old_w, new_w) = (old_screen.width.max(1), screen.width.max(1));

    let top = if old.bottom() >= old_screen.height {
        screen.height.saturating_sub(height)
    } else {
        old.y.min(screen.height.saturating_sub(height))
    };

    let rewrapped = (old.height as u32 * old_w.div_ceil(new_w) as u32).min(screen.height as u32);
    let sunk = screen.height.saturating_sub((rewrapped as u16).max(height));
    let risen = ((old.y as u32 * old_w as u32) / new_w as u32).min(screen.height as u32) as u16;
    (top, top.min(sunk).min(risen))
}

/// Share of the screen the viewport may take. Growing it scrolls the transcript
/// away, so an unbounded pane walks the conversation off the top. A view taller
/// than its share clips instead.
const MAX_VIEWPORT_NUM: u16 = 3;
const MAX_VIEWPORT_DEN: u16 = 5;

/// Always leave rows of screen for the transcript to sit in.
fn clamp_height(height: u16, screen: Size) -> u16 {
    let share = (screen.height * MAX_VIEWPORT_NUM / MAX_VIEWPORT_DEN).max(1);
    let ceiling = share.min(screen.height.saturating_sub(1).max(1));
    height.clamp(1, ceiling)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Style;

    fn screen(height: u16) -> Size {
        Size { width: 80, height }
    }

    #[test]
    fn the_viewport_never_takes_the_whole_screen() {
        for h in [10u16, 24, 40, 100] {
            let capped = clamp_height(u16::MAX, screen(h));
            assert!(
                capped < h,
                "viewport {capped} left no transcript on {h} rows"
            );
        }
    }

    #[test]
    fn a_pane_that_fits_its_share_is_left_alone() {
        assert_eq!(clamp_height(4, screen(40)), 4);
        assert_eq!(clamp_height(13, screen(40)), 13);
    }

    #[test]
    fn a_tiny_screen_still_yields_a_usable_viewport() {
        assert_eq!(clamp_height(4, screen(2)), 1);
        assert!(clamp_height(4, screen(1)) >= 1);
    }

    /// A pane sitting on the screen bottom, the state it is in once there is
    /// any transcript at all.
    fn anchored(height: u16, screen: Size) -> Rect {
        Rect::new(0, screen.height - height, screen.width, height)
    }

    #[test]
    fn growing_an_anchored_pane_scrolls_the_transcript_up() {
        let s = screen(40);
        assert_eq!(reflow(anchored(3, s), 20, s), (20, Shift::Up(17)));
    }

    #[test]
    fn shrinking_an_anchored_pane_scrolls_the_transcript_back_down() {
        let s = screen(40);
        assert_eq!(reflow(anchored(20, s), 3, s), (37, Shift::Down(17)));
    }

    /// The bug this pairing exists for: opening a picker and closing it used
    /// to leave the transcript 17 rows higher than it started, with the gap
    /// standing open above the pane.
    #[test]
    fn a_grow_and_shrink_round_trip_puts_the_transcript_back() {
        let s = screen(40);
        let start = anchored(3, s);
        let (grown_top, up) = reflow(start, 20, s);
        let grown = Rect::new(0, grown_top, s.width, 20);
        let (back_top, down) = reflow(grown, start.height, s);
        assert_eq!(back_top, start.y);
        assert_eq!((up, down), (Shift::Up(17), Shift::Down(17)));
    }

    /// Before there is enough transcript to reach the bottom, the pane grows
    /// into the empty rows below and nothing above it moves.
    #[test]
    fn a_floating_pane_grows_downward_without_touching_the_transcript() {
        let s = screen(40);
        let floating = Rect::new(0, 5, s.width, 3);
        assert_eq!(reflow(floating, 10, s), (5, Shift::Up(0)));
        assert_eq!(reflow(floating, 2, s), (5, Shift::None));
    }

    /// Only the rows that could not come from below are taken from the
    /// transcript.
    /// Narrowing 100 → 60 doubles every painted row, so a six-row pane leaves
    /// a twelve-row image ending at the screen bottom; all of it is cleared.
    #[test]
    fn narrowing_clears_the_whole_rewrapped_pane_image() {
        let old_screen = Size {
            width: 100,
            height: 30,
        };
        let new_screen = Size {
            width: 60,
            height: 30,
        };
        let (top, clear_from) = reflow_on_resize(anchored(6, old_screen), old_screen, new_screen);
        assert_eq!(top, 24);
        assert_eq!(clear_from, 18);
    }

    /// Widening 60 → 100 lets the unwrap above pull the old pane image up, to
    /// at best old.y scaled by the width ratio; clear from there down.
    #[test]
    fn widening_clears_down_from_where_the_old_image_may_have_risen() {
        let old_screen = Size {
            width: 60,
            height: 30,
        };
        let new_screen = Size {
            width: 100,
            height: 30,
        };
        let (top, clear_from) = reflow_on_resize(anchored(6, old_screen), old_screen, new_screen);
        assert_eq!(top, 24);
        assert_eq!(clear_from, 24 * 60 / 100);
    }

    /// A height-only change keeps the pane image at the bottom; only that
    /// band needs clearing.
    #[test]
    fn a_height_only_resize_clears_only_the_bottom_band() {
        let old_screen = Size {
            width: 80,
            height: 30,
        };
        let new_screen = Size {
            width: 80,
            height: 15,
        };
        let (top, clear_from) = reflow_on_resize(anchored(6, old_screen), old_screen, new_screen);
        assert_eq!(top, 9);
        assert_eq!(clear_from, 9);
    }

    #[test]
    fn a_partial_grow_takes_only_what_the_rows_below_cannot_cover() {
        let s = screen(40);
        let floating = Rect::new(0, 30, s.width, 4);
        // Six rows free below, so a ten-row growth borrows four from above.
        assert_eq!(reflow(floating, 14, s), (26, Shift::Up(4)));
    }

    #[test]
    fn wide_grapheme_continuation_cells_are_dropped() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 1));
        buf.set_string(0, 0, "🎉🎉", Style::default());
        let kept: Vec<usize> = buf
            .content
            .iter()
            .enumerate()
            .filter(keeps_cell(8))
            .map(|(i, _)| i)
            .collect();
        // Each emoji covers two cells; the cells right after them hold a
        // literal space that must not be printed.
        assert!(kept.contains(&0) && kept.contains(&2));
        assert!(!kept.contains(&1) && !kept.contains(&3));
    }

    #[test]
    fn a_wide_grapheme_in_the_last_column_does_not_eat_the_next_row() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 2, 2));
        buf[(0, 0)].set_symbol("あ");
        buf[(0, 1)].set_symbol("x");
        // (1, 0) is あ's continuation space, dropped; the next row survives.
        let kept: Vec<usize> = buf
            .content
            .iter()
            .enumerate()
            .filter(keeps_cell(2))
            .map(|(i, _)| i)
            .collect();
        assert_eq!(kept, vec![0, 2, 3]);
    }
}
