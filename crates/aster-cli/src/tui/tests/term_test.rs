//! Spacing is a property, not a look. Reading the transcript back off the
//! simulated terminal has to give exactly the rows that were handed to it, in
//! order: a blank band shows up as rows nobody wrote, a pane drawn over the
//! transcript as rows gone missing, and a pane left behind by a resize as a
//! second composer. Each test drives the real [`InlineTerm`] through a session
//! and reads the result back.

use std::cell::RefCell;
use std::io::{self, Write};
use std::rc::Rc;

use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};
use ratatui::style::Style;
use ratatui::widgets::Widget;

use super::super::term::InlineTerm;
use super::super::term::vt::Vt;

#[derive(Clone, Default)]
struct Pipe(Rc<RefCell<Vec<u8>>>);

impl Write for Pipe {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.borrow_mut().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct Harness {
    term: InlineTerm<Pipe>,
    pipe: Pipe,
    vt: Vt,
    /// Every transcript row handed over, so a test can demand all of them back.
    emitted: Vec<String>,
}

impl Harness {
    /// A session that has just started: the screen is clear and the pane is a
    /// four-row idle composer at the top, which is what `chat` opens with.
    fn start(cols: u16, rows: u16, height: u16) -> Self {
        Self::on(Vt::new(cols, rows), height)
    }

    fn on(vt: Vt, height: u16) -> Self {
        let pipe = Pipe::default();
        let term = InlineTerm::for_test(pipe.clone(), vt.size(), 0, height);
        let mut h = Self {
            term,
            pipe,
            vt,
            emitted: Vec::new(),
        };
        h.pump();
        h.clear();
        h
    }

    fn pump(&mut self) {
        let bytes = std::mem::take(&mut *self.pipe.0.borrow_mut());
        self.vt.feed(&bytes);
    }

    fn clear(&mut self) {
        self.term.clear_all().expect("clear");
        self.emitted.clear();
        self.pump();
    }

    fn insert(&mut self, lines: &[&str]) {
        let width = self.term.width();
        let area = Rect::new(0, 0, width, lines.len() as u16);
        let mut buf = Buffer::empty(area);
        for (y, line) in lines.iter().enumerate() {
            buf.set_stringn(0, y as u16, line, width as usize, Style::default());
        }
        self.term.insert_history(&buf).expect("insert");
        self.emitted
            .extend(lines.iter().map(|l| l.trim_end().to_string()));
        self.pump();
    }

    /// One frame of a pane `height` rows tall, each row labelled so a stale
    /// copy left on screen is impossible to mistake for a fresh one.
    fn draw(&mut self, height: u16) {
        self.term.set_height(height).expect("set_height");
        self.term
            .draw(|frame| {
                let area = frame.area();
                let buf = frame.buffer_mut();
                for y in 0..area.height {
                    let row = format!("pane {y}");
                    ratatui::text::Line::from(row)
                        .render(Rect::new(area.x, area.y + y, area.width, 1), buf);
                }
                // The composer always parks a caret in the pane, and where it
                // sits decides how much a shrinking terminal scrolls away.
                frame.set_cursor_position(Position::new(area.x + 2, area.bottom() - 2));
            })
            .expect("draw");
        self.pump();
    }

    fn resize(&mut self, cols: u16, rows: u16) {
        self.vt.resize(cols, rows);
        self.term.resized_to(self.vt.size()).expect("resize");
        self.pump();
    }

    fn viewport(&self) -> Rect {
        self.term.viewport()
    }

    fn transcript(&self) -> Vec<String> {
        self.vt.transcript(self.viewport().y)
    }

    /// Every row handed to the transcript is still readable, in order, with
    /// nothing inserted between them. Rows lost off the top of scrollback are
    /// allowed; rows reordered, dropped from the middle, or padded are not.
    fn assert_transcript_kept(&self) {
        let got = self.vt.transcript(self.viewport().y);
        let want = &self.emitted;
        assert!(
            got.len() <= want.len(),
            "transcript grew {} rows it was never given\nscreen:\n{}",
            got.len().saturating_sub(want.len()),
            self.vt.screen().join("\n"),
        );
        assert_eq!(
            got,
            want[want.len() - got.len()..],
            "transcript drifted from what was emitted\nscreen:\n{}",
            self.vt.screen().join("\n"),
        );
    }

    /// Every row is still there, in order, with blanks allowed only after the
    /// last of them. Weaker than `assert_transcript_kept`, for the one case
    /// where a blank run above the pane is the accepted cost.
    fn assert_nothing_lost(&self) {
        let mut got = self.transcript();
        while got.last().is_some_and(String::is_empty) {
            got.pop();
        }
        let want = &self.emitted;
        assert_eq!(
            got,
            want[want.len() - got.len()..],
            "transcript drifted from what was emitted\nscreen:\n{}",
            self.vt.screen().join("\n"),
        );
    }

    /// The pane appears once. A resize that leaves the frame it last drew on
    /// screen stamps a copy of the composer into the transcript for every drag
    /// step the terminal reports.
    fn assert_drawn_once(&self) {
        let view = self.viewport();
        let strays: Vec<String> = self
            .vt
            .screen()
            .into_iter()
            .enumerate()
            .filter(|(y, row)| {
                !(view.y..view.bottom()).contains(&(*y as u16)) && row.contains("pane ")
            })
            .map(|(y, row)| format!("{y}: {row}"))
            .collect();
        assert!(
            strays.is_empty(),
            "{} stale pane rows outside {view:?}:\n{}\nscreen:\n{}",
            strays.len(),
            strays.join("\n"),
            self.vt.screen().join("\n"),
        );
    }

    fn assert_sound(&self) {
        self.assert_transcript_kept();
        self.assert_drawn_once();
        let screen = self.vt.size();
        let view = self.viewport();
        assert!(
            view.bottom() <= screen.height,
            "pane {view:?} hangs off a {}-row screen",
            screen.height,
        );
        assert_eq!(
            view.width,
            self.term.width(),
            "the pane is measured at one width and drawn at another",
        );
    }
}

fn block(tag: &str, rows: usize) -> Vec<String> {
    (0..rows).map(|i| format!("{tag} {i}")).collect()
}

/// Transcript rows that fill most of the width, the way a wrapped paragraph or
/// a `skills` line does. Narrow the terminal and these rewrap; short rows do
/// not, and a test built only from those never exercises a width change.
fn wide_block(tag: &str, rows: usize, width: usize) -> Vec<String> {
    (0..rows)
        .map(|i| {
            let head = format!("{tag} {i} ");
            format!("{head}{}", "x".repeat(width - head.len()))
        })
        .collect()
}

fn refs(lines: &[String]) -> Vec<&str> {
    lines.iter().map(String::as_str).collect()
}

#[test]
fn a_fresh_session_stacks_the_pane_under_the_welcome_block() {
    let mut h = Harness::start(100, 40, 4);
    let welcome = block("welcome", 23);
    h.insert(&refs(&welcome));
    h.draw(5);
    h.assert_sound();
    assert_eq!(h.viewport().y, 23);
    assert_eq!(h.vt.screen()[23], "pane 0");
}

#[test]
fn growing_the_window_leaves_no_band_between_transcript_and_pane() {
    let mut h = Harness::start(195, 62, 4);
    h.insert(&refs(&block("line", 29)));
    h.draw(5);
    h.resize(195, 30);
    h.draw(5);
    h.assert_sound();
    h.resize(195, 62);
    h.draw(5);
    h.assert_sound();
}

#[test]
fn shrinking_the_window_keeps_every_transcript_row() {
    let mut h = Harness::start(195, 62, 4);
    h.insert(&refs(&block("line", 29)));
    h.draw(5);
    h.resize(195, 24);
    h.draw(5);
    h.assert_sound();
}

#[test]
fn a_pane_that_grows_and_shrinks_keeps_every_row_it_scrolled_past() {
    let mut h = Harness::start(100, 30, 4);
    h.insert(&refs(&block("line", 40)));
    h.draw(4);
    // Growing borrows rows from the screen, and the terminal cannot hand back
    // what went into scrollback. Shrinking has to leave padding in their place,
    // but the rows themselves must all still be there to scroll back to.
    h.draw(16);
    h.draw(4);
    h.assert_sound();
}

#[test]
fn closing_a_tall_pane_leaves_no_blank_band_behind() {
    // The shape of the reported bug: a picker opens over a young session and
    // closes again, and the rows it borrowed came back as blanks wedged between
    // scrollback and the rest of the transcript.
    let mut h = Harness::start(195, 40, 4);
    h.insert(&refs(&block("line", 29)));
    h.draw(5);
    h.draw(24);
    h.draw(5);
    h.assert_sound();
    h.insert(&refs(&block("after", 4)));
    h.draw(5);
    h.assert_sound();
}

#[test]
fn transcript_written_while_a_tall_pane_is_open_stays_contiguous() {
    let mut h = Harness::start(100, 30, 4);
    h.insert(&refs(&block("head", 10)));
    h.draw(14);
    h.insert(&refs(&block("body", 12)));
    h.draw(14);
    h.draw(4);
    h.insert(&refs(&block("tail", 12)));
    h.draw(4);
    h.assert_sound();
}

#[test]
fn narrowing_never_draws_the_pane_over_the_transcript() {
    let mut h = Harness::start(120, 30, 4);
    h.insert(&refs(&block("line", 12)));
    h.draw(5);
    h.resize(60, 30);
    h.draw(5);
    h.assert_transcript_kept();
    assert!(h.viewport().y >= 12, "pane landed on transcript rows");
}

#[test]
fn clearing_starts_the_next_session_at_the_top() {
    let mut h = Harness::start(100, 30, 4);
    h.insert(&refs(&block("old", 20)));
    h.draw(5);
    h.clear();
    h.insert(&refs(&block("new", 3)));
    h.draw(5);
    h.assert_sound();
    assert_eq!(h.viewport().y, 3);
    assert!(
        h.transcript().iter().all(|l| !l.starts_with("old")),
        "a cleared session still shows the old transcript",
    );
}

#[test]
fn a_resize_storm_never_loses_a_row_or_opens_a_gap() {
    let mut h = Harness::start(120, 40, 4);
    h.insert(&refs(&block("line", 18)));
    h.draw(5);
    for (cols, rows) in [
        (120, 20),
        (120, 40),
        (80, 40),
        // Corners drag both dimensions at once, which used to skip the row
        // accounting entirely and scroll the transcript away twice over.
        (60, 18),
        (150, 28),
        (200, 50),
        (80, 12),
        (120, 40),
    ] {
        h.resize(cols, rows);
        h.draw(5);
        h.assert_transcript_kept();
        assert!(h.viewport().bottom() <= rows, "pane hangs off the screen");
        assert_eq!(h.viewport().width, h.term.width());
    }
}

#[test]
fn a_terminal_wider_than_the_reading_limit_measures_and_draws_the_same() {
    let mut h = Harness::start(400, 40, 4);
    h.insert(&refs(&block("line", 6)));
    h.draw(5);
    h.assert_sound();
    assert_eq!(h.term.width(), 240);
}

#[test]
fn a_very_narrow_terminal_still_fits_its_pane_on_screen() {
    let mut h = Harness::start(12, 20, 4);
    h.insert(&refs(&block("l", 4)));
    h.draw(5);
    h.assert_sound();
    assert_eq!(h.term.width(), 12);
}

#[test]
fn the_pane_never_swallows_the_whole_screen() {
    let mut h = Harness::start(100, 20, 4);
    h.insert(&refs(&block("line", 4)));
    h.draw(200);
    h.assert_transcript_kept();
    assert!(
        h.viewport().height < 20,
        "a pane of {} rows left no transcript on 20",
        h.viewport().height,
    );
}

#[test]
fn dragging_the_window_narrow_stamps_no_second_composer() {
    // The reported bug: a split drag reports a resize per step, and each one
    // left the frame it had drawn behind, so the composer piled up down the
    // screen while the transcript above it was eaten.
    let mut h = Harness::start(195, 44, 4);
    h.insert(&refs(&wide_block("line", 29, 180)));
    h.draw(5);
    for cols in [150, 120, 95, 75, 60] {
        h.resize(cols, 44);
        h.draw(5);
        h.assert_drawn_once();
        h.assert_transcript_kept();
    }
    h.assert_sound();
}

#[test]
fn a_drag_that_ends_where_it_started_leaves_one_pane() {
    let mut h = Harness::start(120, 30, 4);
    h.insert(&refs(&wide_block("line", 10, 110)));
    h.draw(5);
    for (cols, rows) in [(100, 30), (80, 26), (60, 22), (80, 26), (120, 30)] {
        h.resize(cols, rows);
        h.draw(5);
        h.assert_drawn_once();
    }
    h.assert_sound();
}

#[test]
fn a_terminal_that_does_not_rewrap_still_gets_one_composer() {
    // Where the rows end up after a width change is the terminal's business and
    // the two families answer differently. Counting rows cannot be right for
    // both, but rubbing out the last frame from the caret up is, so whatever
    // else a drag costs, it never leaves a second composer behind.
    let mut h = Harness::on(Vt::new(195, 44).without_reflow(), 4);
    h.insert(&refs(&wide_block("line", 29, 180)));
    h.draw(5);
    for cols in [150, 120, 95, 75, 60] {
        h.resize(cols, 44);
        h.draw(5);
        h.assert_drawn_once();
    }
}

#[test]
fn a_terminal_that_keeps_its_scrollback_on_growing_loses_no_transcript() {
    // Whether a taller window is filled from scrollback or with blank rows is
    // the terminal's call, and the two answers put the end of the transcript in
    // different places. Guessing the roomier of the two can leave a blank run
    // above the pane; guessing the other way would draw the pane over rows that
    // are still there. The transcript is what must not be gambled with.
    let mut h = Harness::on(Vt::new(195, 62).without_restore(), 4);
    h.insert(&refs(&block("line", 29)));
    h.draw(5);
    h.resize(195, 30);
    h.draw(5);
    h.assert_transcript_kept();
    h.assert_drawn_once();
    h.resize(195, 62);
    h.draw(5);
    // Every row survives and the pane is drawn once. What this terminal does
    // cost is a blank run above the pane, the width of what it declined to hand
    // back, which is why `assert_transcript_kept` is too strong here.
    h.assert_nothing_lost();
    h.assert_drawn_once();
}
