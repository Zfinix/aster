//! Minimal inline-viewport terminal, derived from ratatui's `Terminal`
//! (MIT, © Ratatui Developers). The cursor is queried once at startup;
//! height changes and history inserts never re-anchor, which is what keeps
//! the viewport from landing on top of scrollback mid-stream.

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
    /// Growth takes free rows below first, then scrolls history up; shrink
    /// gives rows back on the anchored side.
    pub(super) fn set_height(&mut self, height: u16) -> Result<()> {
        let height = clamp_height(height, self.screen);
        let old = self.viewport;
        if height == old.height {
            return Ok(());
        }

        let top = if height < old.height {
            // Shrink toward the anchored side, clearing the vacated rows.
            if old.bottom() >= self.screen.height {
                let top = old.bottom() - height;
                self.clear_rows(old.y..top)?;
                top
            } else {
                self.clear_rows(old.y + height..old.bottom())?;
                old.y
            }
        } else {
            // Grow into free rows below; whatever is left scrolls history up.
            let delta = height - old.height;
            let room_below = self.screen.height.saturating_sub(old.bottom());
            let need_above = delta.saturating_sub(room_below).min(old.y);
            if need_above > 0 {
                self.backend.scroll_region_up(0..old.y, need_above)?;
            }
            old.y - need_above
        };

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
            self.backend.scroll_region_up(0..top, to_draw)?;
            remaining = self.draw_cleared(top - to_draw, to_draw, remaining)?;
            height -= to_draw;
        }
        self.backend.flush()?;
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

/// Always leave at least one row of screen to scroll history through.
fn clamp_height(height: u16, screen: Size) -> u16 {
    height.clamp(1, screen.height.saturating_sub(1).max(1))
}
