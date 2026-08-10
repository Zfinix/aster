//! Minimal inline-viewport terminal, derived from ratatui's `Terminal`
//! (MIT, © Ratatui Developers). The cursor is queried once at startup;
//! height changes and history inserts never re-anchor, which is what keeps
//! the viewport from landing on top of scrollback mid-stream. Finished lines
//! go into the terminal's own scrollback, so scrolling the transcript is the
//! terminal's job and history is not capped at one screen.

use std::io::{self, Stdout};

use anyhow::Result;
use ratatui::backend::{Backend, CrosstermBackend};
use ratatui::buffer::{Buffer, Cell};
use ratatui::layout::{Position, Rect, Size};

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
        let mut height = cells.area.height;

        // If the viewport floats above the bottom, push it down first.
        if self.viewport.bottom() < self.screen.height {
            let to_draw = height.min(self.screen.height - self.viewport.bottom());
            self.backend.scroll_region_down(
                self.viewport.top()..self.viewport.bottom() + to_draw,
                to_draw,
            )?;
            remaining = self.draw_cleared(self.viewport.top(), to_draw, remaining)?;
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
            remaining = self.draw_cleared(top - to_draw, to_draw, remaining)?;
            height -= to_draw;
        }
        self.backend.flush()?;
        Ok(())
    }

    /// Open `rows` blank rows at the bottom of the region above the viewport,
    /// handing what leaves the top to the terminal's scrollback.
    ///
    /// The scroll has to come from line feeds at the bottom margin. `SU`, which
    /// is what a plain scroll-up emits, moves the same rows but throws away
    /// what falls off, so history above the viewport was capped at one screen
    /// and older lines were gone rather than merely out of view. The region
    /// stops short of the viewport, so the viewport itself never moves and
    /// needs no repaint.
    fn scroll_into_scrollback(&mut self, region_bottom: u16, rows: u16) -> Result<()> {
        use std::io::Write;
        if region_bottom == 0 || rows == 0 {
            return Ok(());
        }
        // Region and cursor address are 1-based, so `region_bottom` names the
        // region's last row, which is the row above the viewport.
        write!(self.backend, "\x1b[1;{region_bottom}r")?;
        // Padding a shrinking pane left behind is not history. `SU` discards
        // what leaves the top, which is exactly what those rows deserve and
        // exactly why the line feeds below cannot be used on them.
        let discarded = rows.min(self.blank_top);
        if discarded > 0 {
            write!(self.backend, "\x1b[{discarded}S")?;
            self.blank_top -= discarded;
        }
        write!(self.backend, "\x1b[{region_bottom};1H")?;
        for _ in 0..rows - discarded {
            write!(self.backend, "\r\n")?;
        }
        write!(self.backend, "\x1b[r")?;
        Ok(())
    }

    /// Move the transcript back down into the rows a shrinking pane gave up,
    /// the inverse of [`Self::scroll_into_scrollback`]. Scrollback cannot be
    /// pulled back, so the rows opened at the top are blank; they are counted
    /// so the next scroll drops them rather than filing them as history.
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

    /// The terminal was resized: re-clamp the viewport and force a repaint.
    pub(super) fn resized(&mut self) -> Result<()> {
        self.screen = self.backend.size()?;
        let height = clamp_height(self.viewport.height, self.screen);
        let top = self
            .viewport
            .y
            .min(self.screen.height.saturating_sub(height));
        self.viewport = Rect::new(0, top, self.screen.width, height);
        self.buffers = [Buffer::empty(self.viewport), Buffer::empty(self.viewport)];
        // The screen reflowed under us, so where the padding ended up is a
        // guess. Forget it rather than discard a row of real transcript.
        self.blank_top = 0;
        self.clear_rows(self.viewport.y..self.viewport.bottom())?;
        Ok(())
    }

    /// Draw cells onto rows known to be blank (just scrolled clear).
    fn draw_cleared<'a>(&mut self, y: u16, rows: u16, cells: &'a [Cell]) -> Result<&'a [Cell]> {
        let width = self.screen.width as usize;
        let take = (width * rows as usize).min(cells.len());
        let (to_draw, rest) = cells.split_at(take);
        let iter = to_draw
            .iter()
            .enumerate()
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

/// What the rows above the viewport do when its boundary moves.
#[derive(Debug, PartialEq, Eq)]
enum Shift {
    None,
    /// Transcript scrolls up, the top rows passing into scrollback.
    Up(u16),
    /// Transcript scrolls back down into the rows the pane gave up.
    Down(u16),
}

/// Where the viewport lands at `height`, and what the transcript above does
/// to get there. Growing takes free rows below before it takes any from the
/// transcript; shrinking hands them back the same way, which is what keeps a
/// pane that opens and closes from leaving a band of dead rows behind it.
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

/// Share of the screen the viewport may take. Growing it scrolls the
/// transcript away, so an unbounded pane walks the conversation off the top
/// until nothing of it is left. A view taller than its share clips instead,
/// which is the pane's problem to window around.
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
    #[test]
    fn a_partial_grow_takes_only_what_the_rows_below_cannot_cover() {
        let s = screen(40);
        let floating = Rect::new(0, 30, s.width, 4);
        // Six rows free below, so a ten-row growth borrows four from above.
        assert_eq!(reflow(floating, 14, s), (26, Shift::Up(4)));
    }
}
