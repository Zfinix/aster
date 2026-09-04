//! [`Tui`] owns raw mode, events, and an inline viewport ([`super::term`]);
//! finished blocks go above it into real scrollback via `insert_history`.
//! Drawing is on demand through [`FrameRequester`], coalesced per deadline.

use std::io::{self, Write};
use std::time::{Duration, Instant};

use anyhow::Result;
use futures_util::StreamExt;
use ratatui::crossterm::cursor::MoveTo;
use ratatui::crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture, Event,
    EventStream, KeyEvent, KeyEventKind, MouseEvent,
};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{Clear, ClearType, disable_raw_mode, enable_raw_mode};
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::{Paragraph, Widget};
use tokio::sync::mpsc;

pub(super) use super::term::Frame;
use super::term::InlineTerm;

const MIN_FRAME_INTERVAL: Duration = Duration::from_millis(16);

/// One input to the UI loop, merged from the terminal and the frame scheduler.
pub(super) enum TuiEvent {
    Key(KeyEvent),
    Mouse(MouseEvent),
    Paste(String),
    Resize,
    Draw,
}

/// Cheap-to-clone handle for requesting frames from anywhere (status spinner,
/// stream arrival, key handling).
#[derive(Clone)]
pub(super) struct FrameRequester {
    tx: mpsc::UnboundedSender<Instant>,
}

impl FrameRequester {
    /// A detached handle for tests; its requests go nowhere.
    #[cfg(test)]
    pub(super) fn noop() -> Self {
        let (tx, _rx) = mpsc::unbounded_channel();
        Self { tx }
    }

    pub(super) fn schedule_now(&self) {
        let _ = self.tx.send(Instant::now());
    }

    pub(super) fn schedule_in(&self, delay: Duration) {
        let _ = self.tx.send(Instant::now() + delay);
    }
}

pub(super) struct Tui {
    term: InlineTerm,
    events: Option<EventStream>,
    frame_tx: mpsc::UnboundedSender<Instant>,
    frame_rx: mpsc::UnboundedReceiver<Instant>,
    next_frame: Option<Instant>,
    last_draw: Instant,
    mouse: bool,
}

impl Tui {
    pub(super) fn new(height: u16) -> Result<Self> {
        enable_raw_mode()?;
        execute!(io::stdout(), EnableBracketedPaste)?;
        let (frame_tx, frame_rx) = mpsc::unbounded_channel();
        Ok(Self {
            term: InlineTerm::new(height)?,
            events: None,
            frame_tx,
            frame_rx,
            next_frame: None,
            last_draw: Instant::now() - MIN_FRAME_INTERVAL,
            mouse: false,
        })
    }

    /// Capture the mouse only while something is there to click. Held on, it
    /// would take away the terminal's own selection and copy over the
    /// transcript, which is the whole point of rendering into scrollback.
    pub(super) fn set_mouse(&mut self, on: bool) {
        if self.mouse == on {
            return;
        }
        let result = match on {
            true => execute!(io::stdout(), EnableMouseCapture),
            false => execute!(io::stdout(), DisableMouseCapture),
        };
        match result {
            Ok(()) => self.mouse = on,
            Err(e) => tracing::debug!("could not toggle mouse capture: {e}"),
        }
    }

    pub(super) fn frame_requester(&self) -> FrameRequester {
        FrameRequester {
            tx: self.frame_tx.clone(),
        }
    }

    pub(super) fn width(&self) -> u16 {
        self.term.width()
    }

    /// Wait for the next thing the UI loop should react to. Key repeats and
    /// releases are filtered here so callers only ever see presses.
    pub(super) async fn next_event(&mut self) -> TuiEvent {
        loop {
            while let Ok(at) = self.frame_rx.try_recv() {
                self.request_frame(at);
            }

            let deadline = self.next_frame;
            let events = self.events.get_or_insert_with(EventStream::new);
            tokio::select! {
                ev = events.next() => match ev {
                    Some(Ok(Event::Key(key))) if key.kind == KeyEventKind::Press => {
                        return TuiEvent::Key(key);
                    }
                    Some(Ok(Event::Mouse(m))) => return TuiEvent::Mouse(m),
                    Some(Ok(Event::Paste(text))) => return TuiEvent::Paste(text),
                    Some(Ok(Event::Resize(..))) => return TuiEvent::Resize,
                    Some(_) => {}
                    // The stream ended: the terminal is gone; report a draw so
                    // the caller's loop can notice shutdown state.
                    None => return TuiEvent::Draw,
                },
                _ = sleep_until(deadline), if deadline.is_some() => {
                    self.next_frame = None;
                    self.last_draw = Instant::now();
                    return TuiEvent::Draw;
                }
                req = self.frame_rx.recv() => {
                    if let Some(at) = req {
                        self.request_frame(at);
                    }
                }
            }
        }
    }

    fn request_frame(&mut self, at: Instant) {
        let at = at.max(self.last_draw + MIN_FRAME_INTERVAL);
        self.next_frame = Some(self.next_frame.map_or(at, |cur| cur.min(at)));
    }

    /// Draw the viewport at `height` rows; the boundary moves without ever
    /// re-anchoring on the cursor.
    pub(super) fn draw(&mut self, height: u16, render: impl FnOnce(&mut Frame)) -> Result<()> {
        self.term.set_height(height.max(1))?;
        self.term.draw(render)
    }

    /// Append a finished block above the viewport, into real scrollback.
    pub(super) fn insert_history(&mut self, lines: Vec<Line<'static>>) -> Result<()> {
        let height = u16::try_from(lines.len()).unwrap_or(u16::MAX);
        if height == 0 {
            return Ok(());
        }
        let area = Rect::new(0, 0, self.term.width(), height);
        let mut buf = ratatui::buffer::Buffer::empty(area);
        Paragraph::new(lines).render(area, &mut buf);
        self.term.insert_history(&buf)
    }

    /// `/clear`: wipe the screen and scrollback, re-anchor at the top.
    pub(super) fn clear_screen(&mut self) -> Result<()> {
        self.term.clear_all()
    }

    /// Called on `TuiEvent::Resize` so the viewport tracks the terminal.
    pub(super) fn resized(&mut self) -> Result<()> {
        self.term.resized()
    }
}

impl Drop for Tui {
    fn drop(&mut self) {
        // Clear the viewport and hand the cursor back to the shell below it.
        self.set_mouse(false);
        let mut out = io::stdout();
        let _ = execute!(
            out,
            MoveTo(0, self.term.viewport_top()),
            Clear(ClearType::FromCursorDown)
        );
        let _ = execute!(out, DisableBracketedPaste, ratatui::crossterm::cursor::Show);
        let _ = disable_raw_mode();
        let _ = out.flush();
    }
}

async fn sleep_until(deadline: Option<Instant>) {
    match deadline {
        Some(at) => tokio::time::sleep_until(tokio::time::Instant::from_std(at)).await,
        None => std::future::pending().await,
    }
}

/// Emergency restore for the panic hook, which has no handle on the terminal.
pub(super) fn restore_raw() {
    let mut out = io::stdout();
    let _ = execute!(out, DisableMouseCapture);
    let _ = execute!(out, DisableBracketedPaste, ratatui::crossterm::cursor::Show);
    let _ = disable_raw_mode();
    let _ = out.flush();
}
