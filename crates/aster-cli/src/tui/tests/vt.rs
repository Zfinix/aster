//! A terminal, enough of one to hold [`super::InlineTerm`] to account. It
//! consumes the bytes the real backend writes and answers the only question
//! that matters for layout: what is on the screen, and what went to scrollback.
//!
//! Modelled on xterm.js, which is what VS Code runs, and which matches Ghostty
//! and iTerm2 on the two things layout turns on: a narrowed window rewraps
//! every row whose written cells run past the new width, and the caret rides
//! along with the row it sits on.

use ratatui::layout::Size;

/// A cell nothing was ever written to. Terminals tell these apart from a space
/// that was written, and rewrap on the difference, so this model has to too.
const UNSET: char = '\0';

#[derive(Clone)]
struct Row {
    cells: Vec<char>,
    /// This row is the tail of the one above, not a line of its own.
    wrapped: bool,
}

impl Row {
    fn blank(cols: u16) -> Self {
        Self {
            cells: vec![UNSET; cols as usize],
            wrapped: false,
        }
    }

    /// Columns actually written, which is what a terminal rewraps on.
    fn written(&self) -> usize {
        self.cells
            .iter()
            .rposition(|c| *c != UNSET)
            .map_or(0, |i| i + 1)
    }

    /// The cells that were written, untrimmed, so joining a wrapped run back
    /// together lines the halves up exactly as they were printed.
    fn raw(&self) -> String {
        self.cells[..self.written()]
            .iter()
            .map(|c| if *c == UNSET { ' ' } else { *c })
            .collect()
    }

    fn text(&self) -> String {
        self.raw().trim_end().to_string()
    }
}

pub(super) struct Vt {
    cols: u16,
    rows: u16,
    grid: Vec<Row>,
    scrollback: Vec<Row>,
    cursor: (u16, u16),
    region: (u16, u16),
    /// Set once the cursor writes the last column. Real terminals hold it there
    /// until the next glyph rather than wrapping eagerly, and a viewport that
    /// fills its width depends on that.
    wrap_pending: bool,
    pending: Vec<u8>,
    /// Whether a width change re-lays the rows. Every terminal this targets
    /// does, but classic xterm does not, and the pane has to survive both.
    reflows: bool,
    /// Whether a taller window is filled from scrollback. Terminals split on
    /// this too, and the pane has to survive both.
    restores: bool,
}

impl Vt {
    pub(super) fn new(cols: u16, rows: u16) -> Self {
        Self {
            cols,
            rows,
            grid: vec![Row::blank(cols); rows as usize],
            scrollback: Vec::new(),
            cursor: (0, 0),
            region: (0, rows - 1),
            wrap_pending: false,
            pending: Vec::new(),
            reflows: true,
            restores: true,
        }
    }

    /// A terminal that truncates rows on a width change instead of re-laying
    /// them, the way classic xterm does.
    pub(super) fn without_reflow(mut self) -> Self {
        self.reflows = false;
        self
    }

    /// A terminal that answers a taller window with blank rows at the bottom
    /// rather than rows out of scrollback. xterm.js does this whenever anything
    /// sits below the caret, which for this pane is always.
    pub(super) fn without_restore(mut self) -> Self {
        self.restores = false;
        self
    }

    pub(super) fn size(&self) -> Size {
        Size {
            width: self.cols,
            height: self.rows,
        }
    }

    pub(super) fn screen(&self) -> Vec<String> {
        self.grid.iter().map(Row::text).collect()
    }

    /// Everything the reader can scroll back to, oldest first, as the lines
    /// they read as: scrollback then the screen rows above the viewport, with
    /// rows a narrowed window split back-to-back joined up again.
    pub(super) fn transcript(&self, above: u16) -> Vec<String> {
        let rows = self
            .scrollback
            .iter()
            .chain(self.grid.iter().take(above as usize));
        let mut out: Vec<String> = Vec::new();
        for row in rows {
            match row.wrapped && !out.is_empty() {
                true => out
                    .last_mut()
                    .expect("wrapped follows a line")
                    .push_str(&row.raw()),
                false => out.push(row.raw()),
            }
        }
        out.into_iter().map(|l| l.trim_end().to_string()).collect()
    }

    pub(super) fn resize(&mut self, cols: u16, rows: u16) {
        if cols != self.cols {
            match self.reflows {
                true => self.reflow(cols, rows),
                false => {
                    for row in self.scrollback.iter_mut().chain(self.grid.iter_mut()) {
                        row.cells.resize(cols as usize, UNSET);
                    }
                    self.cols = cols;
                }
            }
        }
        self.fit(rows);
        self.cols = cols;
        self.rows = rows;
        self.region = (0, rows - 1);
        self.cursor.0 = self.cursor.0.min(cols - 1);
        self.cursor.1 = self.cursor.1.min(rows - 1);
        self.wrap_pending = false;
    }

    /// Re-lay every row at the new width, joining what the last width split and
    /// splitting what no longer fits, carrying the caret with its own content.
    fn reflow(&mut self, cols: u16, rows: u16) {
        let start = self.scrollback.len();
        let caret = start + self.cursor.1 as usize;
        let all: Vec<Row> = self
            .scrollback
            .drain(..)
            .chain(self.grid.drain(..))
            .collect();

        let mut out: Vec<Row> = Vec::new();
        // Where each old row's content begins once re-laid.
        let mut moved = vec![0usize; all.len()];
        let mut at = 0;
        while at < all.len() {
            let mut end = at + 1;
            while end < all.len() && all[end].wrapped {
                end += 1;
            }
            let mut content: Vec<char> = Vec::new();
            let mut offsets = Vec::with_capacity(end - at);
            for row in &all[at..end] {
                offsets.push(content.len());
                content.extend_from_slice(&row.cells[..row.written()]);
            }
            let line_start = out.len();
            for (i, chunk) in content.chunks(cols as usize).enumerate() {
                let mut cells = chunk.to_vec();
                cells.resize(cols as usize, UNSET);
                out.push(Row {
                    cells,
                    wrapped: i > 0,
                });
            }
            if out.len() == line_start {
                out.push(Row::blank(cols));
            }
            for (i, off) in offsets.into_iter().enumerate() {
                moved[at + i] = line_start + off / cols as usize;
            }
            at = end;
        }

        // Keep the window where it was unless the caret would fall out of it.
        let caret_row = moved.get(caret).copied().unwrap_or(0);
        let mut top = moved.get(start).copied().unwrap_or(0);
        if caret_row >= top + rows as usize {
            top = caret_row + 1 - rows as usize;
        }
        self.cursor.1 = (caret_row - top) as u16;
        self.scrollback = out[..top.min(out.len())].to_vec();
        self.grid = out[top.min(out.len())..].to_vec();
        self.cols = cols;
    }

    /// Take the window to `rows` the way every terminal does: shrinking keeps
    /// the caret on screen by scrolling the top into scrollback and dropping
    /// the rest off the bottom, growing pulls scrollback back in at the top.
    fn fit(&mut self, rows: u16) {
        if self.grid.len() > rows as usize {
            let scrolled = (self.cursor.1 + 1).saturating_sub(rows) as usize;
            for _ in 0..scrolled {
                self.scrollback.push(self.grid.remove(0));
            }
            self.cursor.1 -= scrolled as u16;
            self.grid.truncate(rows as usize);
        } else {
            let short = rows as usize - self.grid.len();
            let pulled = match self.restores {
                true => short.min(self.scrollback.len()),
                false => 0,
            };
            for _ in 0..pulled {
                self.grid
                    .insert(0, self.scrollback.pop().expect("pulled <= len"));
            }
            self.cursor.1 += pulled as u16;
        }
        while self.grid.len() < rows as usize {
            self.grid.push(Row::blank(self.cols));
        }
    }

    pub(super) fn feed(&mut self, bytes: &[u8]) {
        self.pending.extend_from_slice(bytes);
        let text = String::from_utf8_lossy(&std::mem::take(&mut self.pending)).into_owned();
        let mut chars = text.chars().peekable();
        while let Some(ch) = chars.next() {
            match ch {
                '\x1b' => match chars.peek() {
                    Some('[') => {
                        chars.next();
                        let mut seq = String::new();
                        for c in chars.by_ref() {
                            seq.push(c);
                            if c.is_ascii_alphabetic() || c == '@' {
                                break;
                            }
                        }
                        self.csi(&seq);
                    }
                    // Nothing InlineTerm writes needs the other escapes, and a
                    // silent skip here would hide a real change in what it emits.
                    other => panic!("unhandled escape ESC {other:?}"),
                },
                '\r' => {
                    self.cursor.0 = 0;
                    self.wrap_pending = false;
                }
                '\n' => {
                    self.wrap_pending = false;
                    self.linefeed();
                }
                c if (c as u32) < 0x20 => {}
                c => self.print(c),
            }
        }
    }

    fn csi(&mut self, seq: &str) {
        let (body, verb) = seq.split_at(seq.len() - 1);
        let private = body.starts_with('?');
        let args: Vec<u16> = body
            .trim_start_matches('?')
            .split(';')
            .map(|a| a.parse().unwrap_or(0))
            .collect();
        // Most verbs read an omitted or zero parameter as one; the erasers read
        // zero as a mode of its own, so they take it raw.
        let arg = |i: usize, default: u16| match args.get(i) {
            Some(0) | None => default,
            Some(n) => *n,
        };
        let mode = args.first().copied().unwrap_or(0);
        match verb {
            // Colours, attributes and mode toggles move nothing.
            "m" | "h" | "l" => {}
            "H" => {
                self.cursor = (arg(1, 1).min(self.cols) - 1, arg(0, 1).min(self.rows) - 1);
                self.wrap_pending = false;
            }
            "A" => {
                self.cursor.1 = self.cursor.1.saturating_sub(arg(0, 1));
                self.wrap_pending = false;
            }
            "r" if !private => {
                let bottom = match args.len() {
                    0 | 1 => self.rows,
                    _ => arg(1, self.rows),
                };
                self.region = (arg(0, 1) - 1, bottom.min(self.rows) - 1);
            }
            "S" => self.scroll_up(arg(0, 1)),
            "T" => self.scroll_down(arg(0, 1)),
            "M" => self.delete_lines(arg(0, 1)),
            "J" => match mode {
                0 => self.erase_below(),
                3 => self.scrollback.clear(),
                _ => self.grid = vec![Row::blank(self.cols); self.rows as usize],
            },
            "K" => {
                let (x, y) = (self.cursor.0 as usize, self.cursor.1 as usize);
                match mode {
                    1 => self.grid[y].cells[..=x].fill(UNSET),
                    2 => self.grid[y].cells.fill(UNSET),
                    _ => self.grid[y].cells[x..].fill(UNSET),
                }
            }
            _ => panic!("unhandled CSI {seq:?}"),
        }
    }

    /// `ED 0`: from the caret to the end of the screen, the caret's own row
    /// included from its column rightwards.
    fn erase_below(&mut self) {
        let (x, y) = (self.cursor.0 as usize, self.cursor.1 as usize);
        self.grid[y].cells[x..].fill(UNSET);
        for row in self.grid.iter_mut().skip(y + 1) {
            row.cells.fill(UNSET);
            row.wrapped = false;
        }
    }

    fn print(&mut self, ch: char) {
        if self.wrap_pending {
            self.cursor.0 = 0;
            self.linefeed();
            self.wrap_pending = false;
            let y = self.cursor.1 as usize;
            self.grid[y].wrapped = true;
        }
        let (x, y) = (self.cursor.0 as usize, self.cursor.1 as usize);
        self.grid[y].cells[x] = ch;
        match self.cursor.0 + 1 < self.cols {
            true => self.cursor.0 += 1,
            false => self.wrap_pending = true,
        }
    }

    fn linefeed(&mut self) {
        match self.cursor.1 == self.region.1 {
            true => self.scroll_up(1),
            false => self.cursor.1 = (self.cursor.1 + 1).min(self.rows - 1),
        }
    }

    fn scroll_up(&mut self, rows: u16) {
        let (top, bottom) = self.region;
        for _ in 0..rows {
            let gone = self.grid.remove(top as usize);
            // Only the top of the screen feeds scrollback; a region that starts
            // lower discards what leaves it, as every terminal we target does.
            if top == 0 {
                self.scrollback.push(gone);
            }
            self.grid.insert(bottom as usize, Row::blank(self.cols));
        }
    }

    fn scroll_down(&mut self, rows: u16) {
        let (top, bottom) = self.region;
        for _ in 0..rows {
            self.grid.remove(bottom as usize);
            self.grid.insert(top as usize, Row::blank(self.cols));
        }
    }

    fn delete_lines(&mut self, rows: u16) {
        let (_, bottom) = self.region;
        let at = self.cursor.1 as usize;
        for _ in 0..rows {
            self.grid.remove(at);
            self.grid.insert(bottom as usize, Row::blank(self.cols));
        }
    }
}
