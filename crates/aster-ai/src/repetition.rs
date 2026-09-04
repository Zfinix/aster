//! Detects a model reply that degenerates into verbatim repetition, so the
//! harness can stop a turn that would otherwise stream identical text forever.

use std::fmt;

const REPETITION_WINDOW: usize = 40;
const REPETITION_BUF: usize = 256;
const REPETITION_STOP_AFTER: usize = 4;
const MIN_CHUNK_LETTERS: usize = 8;

pub const DEGENERATE_MSG: &str = "the model's reply degenerated into repeated text and was stopped; \
     switch to a stronger model or higher effort and retry";

/// Marker error for a reply the guard cut off. `agent_loop` matches on this
/// type so a degenerate reply is never misread as a model that rejected tools.
#[derive(Debug, Clone, Copy)]
pub struct DegenerateOutput;

impl fmt::Display for DegenerateOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("the model's reply degenerated into repeated text")
    }
}

impl std::error::Error for DegenerateOutput {}

/// Trips when the trailing window keeps reappearing in the recent buffer. A fixed
/// chunk boundary would miss periodic text ("I'm committing. " repeats every 17
/// chars); searching the buffer is phase-independent.
#[derive(Default)]
pub struct RepetitionGuard {
    buf: String,
    repeat: usize,
}

impl RepetitionGuard {
    /// Feed the next slice of streamed text. Returns true once the stream is
    /// judged degenerate; the caller should stop emitting and abort.
    pub fn feed(&mut self, delta: &str) -> bool {
        self.buf.push_str(delta);
        if self.buf.len() > REPETITION_BUF {
            let keep = boundary(&self.buf, self.buf.len() - REPETITION_BUF);
            self.buf.drain(..keep);
        }
        if self.buf.len() < 2 * REPETITION_WINDOW {
            return false;
        }
        let split = boundary(&self.buf, self.buf.len() - REPETITION_WINDOW);
        let (window, tail) = self.buf.split_at(split);
        if tail.chars().filter(|c| c.is_alphabetic()).count() < MIN_CHUNK_LETTERS {
            self.repeat = 0;
            return false;
        }
        if window.contains(tail) {
            self.repeat += 1;
            self.repeat >= REPETITION_STOP_AFTER
        } else {
            self.repeat = 0;
            false
        }
    }
}

fn boundary(text: &str, index: usize) -> usize {
    let mut index = index.min(text.len());
    while index < text.len() && !text.is_char_boundary(index) {
        index += 1;
    }
    index
}

/// Whether assembled content is degenerate, for responses that arrive whole.
pub fn is_degenerate(content: &str) -> bool {
    let mut guard = RepetitionGuard::default();
    // Feed in window-sized slices so the repeat count can climb; a single feed
    // would only ever run one comparison.
    let mut rest = content;
    while !rest.is_empty() {
        let take = rest
            .char_indices()
            .nth(REPETITION_WINDOW)
            .map(|(i, _)| i)
            .unwrap_or(rest.len());
        let (slice, next) = rest.split_at(take);
        if guard.feed(slice) {
            return true;
        }
        rest = next;
    }
    false
}

#[cfg(test)]
#[path = "tests/repetition_test.rs"]
mod tests;
