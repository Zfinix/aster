//! Minimal inline-viewport terminal, derived from ratatui's `Terminal` (MIT, ©
//! Ratatui Developers). The cursor is queried once at startup and never
//! re-anchored, and finished lines go into the terminal's own scrollback.
//!
//! One law holds after every operation: the viewport sits flush under the
//! transcript, so its top row is the transcript's height capped by the room the
//! screen has left. `term_test` drives this against a simulated terminal.
//! Layout guessed from where the terminal might have put things is what used to
//! leave blank bands in transcripts.

use std::collections::VecDeque;
use std::io::{self, Stdout, Write};

use anyhow::Result;
use ratatui::backend::{Backend, CrosstermBackend};
use ratatui::buffer::{Buffer, Cell};
use ratatui::layout::{Position, Rect, Size};
use unicode_width::UnicodeWidthStr;

/// Wrapping the transcript wider than this is unreadable, so the viewport and
/// the history blocks both stop here however wide the terminal is.
const MAX_WIDTH: u16 = 240;

/// The one width every layer measures with: the pane's `desired_height`, the
/// viewport it renders into, and the history blocks above it. Two of them
/// disagreeing is a blank row or a clipped one.
fn content_width(screen: Size) -> u16 {
    screen.width.clamp(1, MAX_WIDTH)
}

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

pub(super) struct InlineTerm<W: Write = Stdout> {
    backend: CrosstermBackend<W>,
    buffers: [Buffer; 2],
    current: usize,
    viewport: Rect,
    screen: Size,
    transcript: Transcript,
    /// The row the caret was last parked on. A shrinking terminal scrolls
    /// exactly far enough to keep it on screen, which is how much of the
    /// transcript moved out from under us.
    cursor_row: u16,
}

/// Every row written above the viewport, by the width it printed at, split
/// into the part that has scrolled into scrollback and the part still on
/// screen. The transcript is the only thing between the top of the screen and
/// the pane, so counting its rows is what says where the pane goes: no layer
/// has to guess where the terminal put anything.
#[derive(Default)]
struct Transcript {
    rows: VecDeque<u16>,
    /// Leading entries of `rows` that are in scrollback rather than on screen.
    banked: usize,
    /// Columns of entry `banked` that went with them. A terminal rewraps a row
    /// whole and can land the top of the screen part-way down it, so the
    /// boundary has to be held in columns; rounding it to whole rows is a row
    /// of drift every time the window narrows.
    consumed: u16,
}

/// Rows kept. Only the on-screen tail decides layout and only the rows just
/// past it can ever come back, so older ones are dropped; no terminal is
/// anywhere near this tall.
const MAX_TRACKED: usize = 4096;

/// Screen rows a transcript row of `used` columns covers when printed at
/// `width`. An empty row still takes one.
fn height(used: u16, width: u16) -> u16 {
    used.div_ceil(width.max(1)).max(1)
}

impl Transcript {
    fn push(&mut self, used: u16) {
        self.rows.push_back(used);
        while self.rows.len() > MAX_TRACKED {
            self.rows.pop_front();
            match self.banked.checked_sub(1) {
                Some(left) => self.banked = left,
                None => self.consumed = 0,
            }
        }
    }

    fn clear(&mut self) {
        self.rows.clear();
        self.banked = 0;
        self.consumed = 0;
    }

    /// Screen rows the on-screen part covers when printed at `width`.
    fn on_screen(&self, width: u16) -> u16 {
        self.rows
            .iter()
            .enumerate()
            .skip(self.banked)
            .map(|(i, used)| {
                let rows = height(*used, width);
                match i == self.banked {
                    true => rows - self.off_screen(rows, width),
                    false => rows,
                }
            })
            .fold(0u16, u16::saturating_add)
    }

    /// Rows of the first on-screen entry that are above the top of the screen.
    fn off_screen(&self, rows: u16, width: u16) -> u16 {
        (self.consumed / width.max(1)).min(rows)
    }

    /// Account for `rows` screen rows having scrolled off the top.
    fn bank(&mut self, rows: u16, width: u16) {
        let mut left = rows;
        while left > 0 && self.banked < self.rows.len() {
            let total = height(self.rows[self.banked], width);
            let showing = total - self.off_screen(total, width);
            if left < showing {
                self.consumed += left * width.max(1);
                return;
            }
            left -= showing;
            self.banked += 1;
            self.consumed = 0;
        }
    }

    /// Account for `rows` screen rows having come back out of scrollback.
    fn unbank(&mut self, rows: u16, width: u16) {
        let mut left = rows;
        while left > 0 {
            let hidden = self.consumed / width.max(1);
            if hidden > 0 {
                let back = left.min(hidden);
                self.consumed -= back * width.max(1);
                left -= back;
                continue;
            }
            let Some(prev) = self.banked.checked_sub(1) else {
                return;
            };
            self.banked = prev;
            let total = height(self.rows[prev], width);
            if left >= total {
                left -= total;
                self.consumed = 0;
            } else {
                self.consumed = (total - left) * width.max(1);
                return;
            }
        }
    }
}

impl InlineTerm<Stdout> {
    /// Queries the cursor exactly once to anchor the viewport; must run before
    /// a crossterm `EventStream` exists or the reply is swallowed.
    pub(super) fn new(height: u16) -> Result<Self> {
        let mut backend = CrosstermBackend::new(io::stdout());
        let screen = backend.size()?;
        let anchor = backend.get_cursor_position()?.y;
        Self::anchored(backend, screen, anchor, height)
    }

    /// Called on `TuiEvent::Resize` so the viewport tracks the terminal.
    pub(super) fn resized(&mut self) -> Result<()> {
        let screen = self.backend.size()?;
        self.resized_to(screen)
    }
}

impl<W: Write> InlineTerm<W> {
    /// Opens a viewport of `height` rows at the cursor, pushing the screen up
    /// when the cursor sits too low for it to fit.
    fn anchored(
        mut backend: CrosstermBackend<W>,
        screen: Size,
        anchor: u16,
        height: u16,
    ) -> Result<Self> {
        let height = clamp_height(height, screen);
        // Open room below the cursor so the viewport fits on screen.
        let below = height.saturating_sub(1);
        backend.append_lines(below)?;
        let overflow = (anchor + height).saturating_sub(screen.height);
        let top = anchor - overflow.min(anchor);

        let viewport = Rect::new(0, top, content_width(screen), height);
        Ok(Self {
            backend,
            buffers: [Buffer::empty(viewport), Buffer::empty(viewport)],
            current: 0,
            viewport,
            screen,
            transcript: Transcript::default(),
            cursor_row: top,
        })
    }

    pub(super) fn width(&self) -> u16 {
        content_width(self.screen)
    }

    #[cfg(test)]
    pub(super) fn viewport(&self) -> Rect {
        self.viewport
    }

    /// Build one over an arbitrary writer with the screen size and cursor row
    /// supplied, so a test can drive it without a terminal attached.
    #[cfg(test)]
    pub(super) fn for_test(writer: W, screen: Size, anchor: u16, height: u16) -> Self {
        Self::anchored(CrosstermBackend::new(writer), screen, anchor, height)
            .expect("writing to a buffer cannot fail")
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
                self.cursor_row = pos.y;
                self.backend.set_cursor_position(pos)?;
                self.backend.show_cursor()?;
            }
            None => {
                self.backend.hide_cursor()?;
                self.park(self.viewport.y)?;
            }
        }
        Backend::flush(&mut self.backend)?;
        self.current = 1 - self.current;
        Ok(())
    }

    /// Move the viewport boundary without touching the cursor. Growing takes
    /// rows from the transcript, which scroll into scrollback for real;
    /// shrinking gives them back to the screen, never to the transcript, whose
    /// rows are gone for good and cannot be faked with blanks.
    pub(super) fn set_height(&mut self, height: u16) -> Result<()> {
        let height = clamp_height(height, self.screen);
        let old = self.viewport;
        if height == old.height {
            return Ok(());
        }

        let top = old.y.saturating_sub(borrowed(old, height, self.screen));
        self.scroll_into_scrollback(old.y, old.y - top)?;
        // Rows the viewport gave up below it.
        if top + height < old.bottom() {
            self.clear_rows(top + height..old.bottom())?;
        }

        self.viewport = Rect::new(0, top, content_width(self.screen), height);
        self.buffers = [Buffer::empty(self.viewport), Buffer::empty(self.viewport)];
        // The screen under the new viewport is stale; clear it so the next
        // draw's diff-from-empty repaints everything.
        self.clear_rows(self.viewport.y..self.viewport.bottom())?;
        self.park(self.viewport.y)
    }

    /// Insert finished lines above the viewport, into scrollback. Ratatui's
    /// scrolling-regions algorithm, on our own tracked state.
    pub(super) fn insert_history(&mut self, cells: &Buffer) -> Result<()> {
        self.track(cells);
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
        Backend::flush(&mut self.backend)?;
        Ok(())
    }

    fn scroll_into_scrollback(&mut self, region_bottom: u16, rows: u16) -> Result<()> {
        if region_bottom == 0 || rows == 0 {
            return Ok(());
        }
        // Region and cursor address are 1-based, so `region_bottom` names the
        // region's last row, which is the row above the viewport.
        write!(self.backend, "\x1b[1;{region_bottom}r")?;
        write!(self.backend, "\x1b[{region_bottom};1H")?;
        for _ in 0..rows {
            write!(self.backend, "\r\n")?;
        }
        write!(self.backend, "\x1b[r")?;
        self.transcript.bank(rows, self.width());
        Ok(())
    }

    /// Wipe the screen and scrollback and re-anchor at the top, for `/clear`.
    /// The stale diff buffers are dropped so the next draw repaints the pane.
    pub(super) fn clear_all(&mut self) -> Result<()> {
        use ratatui::crossterm::cursor::MoveTo;
        use ratatui::crossterm::execute;
        use ratatui::crossterm::terminal::{Clear, ClearType};
        execute!(
            self.backend,
            Clear(ClearType::All),
            Clear(ClearType::Purge),
            MoveTo(0, 0),
        )?;
        self.viewport = Rect::new(0, 0, content_width(self.screen), self.viewport.height);
        self.buffers = [Buffer::empty(self.viewport), Buffer::empty(self.viewport)];
        self.transcript.clear();
        self.cursor_row = 0;
        Ok(())
    }

    /// The terminal resized and reflowed the screen under us. Work out where it
    /// left the end of the transcript, then put the pane back against it.
    pub(super) fn resized_to(&mut self, screen: Size) -> Result<()> {
        let old_screen = self.screen;
        let height = clamp_height(self.viewport.height, screen);
        let width = content_width(screen);
        // Rows of the pane above the caret, once rewrapped: how far to step back
        // up to reach the top of the frame, and how far the caret now sits below
        // the end of the transcript.
        let above = self.frame_rows_above_caret(width);
        self.erase_last_frame(above)?;
        self.screen = screen;

        // The terminal rewraps first and only then takes the window to its new
        // height, scrolling the top away by however far the caret would
        // otherwise fall off and giving rows back when there is room again.
        let caret = self.transcript.on_screen(width).saturating_add(above);
        self.transcript
            .bank(caret.saturating_sub(screen.height - 1), width);
        self.transcript
            .unbank(screen.height.saturating_sub(old_screen.height), width);
        let end = self.transcript.on_screen(width).min(screen.height);
        let top = end.min(screen.height.saturating_sub(height));

        // Transcript that no longer fits belongs in scrollback. Painting the
        // pane over it instead is how a resize used to eat the conversation.
        if end > top {
            self.viewport.y = end;
            self.scroll_into_scrollback(end, end - top)?;
        }

        self.viewport = Rect::new(0, top, width, height);
        self.buffers = [Buffer::empty(self.viewport), Buffer::empty(self.viewport)];
        // Only rows from the viewport down: everything above it is transcript.
        self.clear_rows(top..screen.height)?;
        self.park(top)
    }

    /// Rub out the frame last drawn, wherever the resize moved it to. The caret
    /// is inside that frame and the terminal carried it along, so stepping up
    /// from the caret finds the frame without knowing a row number, whatever
    /// the terminal did to the rows.
    fn erase_last_frame(&mut self, above: u16) -> Result<()> {
        write!(self.backend, "\r")?;
        if above > 0 {
            write!(self.backend, "\x1b[{above}A")?;
        }
        write!(self.backend, "\x1b[0J")?;
        Ok(())
    }

    /// How tall the part of the last frame above the caret stands once printed
    /// at `width`. Its rows rewrap like any others, and the shaded band makes
    /// even the blank ones full-width content, so they are measured, not counted.
    fn frame_rows_above_caret(&self, width: u16) -> u16 {
        let frame = &self.buffers[1 - self.current];
        frame
            .content
            .chunks(frame.area.width.max(1) as usize)
            .take(self.cursor_row.saturating_sub(self.viewport.y) as usize)
            .map(|row| (used(row) as u16).div_ceil(width).max(1))
            .fold(0u16, u16::saturating_add)
    }

    /// Leave the caret on a row we can name. `erase_last_frame` steps up from
    /// wherever the caret is, so every write that moves it has to say where it
    /// left it, or the next resize erases from the wrong place.
    fn park(&mut self, row: u16) -> Result<()> {
        self.backend.set_cursor_position(Position::new(0, row))?;
        self.cursor_row = row;
        Ok(())
    }

    /// Remember how wide each row prints, so a later rewrap can be counted
    /// rather than guessed at.
    fn track(&mut self, cells: &Buffer) {
        let width = cells.area.width as usize;
        for row in cells.content.chunks(width.max(1)) {
            self.transcript.push(used(row) as u16);
        }
    }

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
        // Only up to the last cell that carries something. Padding a row out to
        // the full width would make it real content to the terminal, and a
        // terminal that reflows would then break that padding onto rows of its
        // own, filling a narrowed window with blanks.
        let iter = to_draw
            .iter()
            .enumerate()
            .filter(move |(i, _)| i % width < used(&to_draw[i - i % width..][..width]))
            .filter(keeps_cell(width))
            .map(|(i, c)| ((i % width) as u16, y + (i / width) as u16, c));
        self.backend.draw(iter)?;
        Ok(rest)
    }

    /// `EL` rather than a row of spaces: spaces are content, and a terminal that
    /// reflows would wrap them onto rows of their own later on.
    fn clear_rows(&mut self, rows: std::ops::Range<u16>) -> Result<()> {
        for y in rows {
            write!(self.backend, "\x1b[{};1H\x1b[2K", y + 1)?;
        }
        Ok(())
    }
}

/// Columns of `row` worth writing: everything up to the last cell that would
/// leave a mark. A cell is blank only if nothing was styled onto it either,
/// since a shaded space still paints.
fn used(row: &[Cell]) -> usize {
    row.iter()
        .rposition(|c| c != &Cell::EMPTY)
        .map_or(0, |i| i + 1)
}

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

/// Rows a pane of `height` has to take from the transcript above it, once the
/// blank screen below it has been used up.
fn borrowed(old: Rect, height: u16, screen: Size) -> u16 {
    let room_below = screen.height.saturating_sub(old.bottom());
    height
        .saturating_sub(old.height)
        .saturating_sub(room_below)
        .min(old.y)
}

const MAX_VIEWPORT_NUM: u16 = 3;
const MAX_VIEWPORT_DEN: u16 = 5;

fn clamp_height(height: u16, screen: Size) -> u16 {
    let share = (screen.height * MAX_VIEWPORT_NUM / MAX_VIEWPORT_DEN).max(1);
    let ceiling = share.min(screen.height.saturating_sub(1).max(1));
    height.clamp(1, ceiling)
}

#[cfg(test)]
#[path = "tests/vt.rs"]
mod vt;

#[cfg(test)]
#[path = "tests/term_test.rs"]
mod term_test;

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

    fn anchored(height: u16, screen: Size) -> Rect {
        Rect::new(0, screen.height - height, screen.width, height)
    }

    #[test]
    fn a_pane_takes_only_the_rows_the_screen_below_it_cannot_cover() {
        let s = screen(40);
        assert_eq!(borrowed(anchored(3, s), 20, s), 17);
        assert_eq!(borrowed(Rect::new(0, 30, s.width, 4), 14, s), 4);
    }

    #[test]
    fn a_pane_with_room_below_it_borrows_nothing() {
        let s = screen(40);
        let floating = Rect::new(0, 5, s.width, 3);
        assert_eq!(borrowed(floating, 10, s), 0);
        assert_eq!(borrowed(floating, 2, s), 0);
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
