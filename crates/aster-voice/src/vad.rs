//! Decides when a speaker has finished, from the loudness of each slice of
//! audio. The room's noise is learned from the quietest early slice, so a hum
//! never counts as talk and talk already underway is not mistaken for noise.

use std::time::Duration;

const LEARN: Duration = Duration::from_millis(250);
const MIN_SPEECH: Duration = Duration::from_millis(300);
pub(crate) const SILENCE_STOP: Duration = Duration::from_millis(1500);
const SPEECH_FLOOR: f32 = 0.01;
const ABOVE_NOISE: f32 = 3.0;

#[derive(Debug)]
pub(crate) struct Endpoint {
    noise: f32,
    elapsed: Duration,
    spoken: Duration,
    quiet: Duration,
}

impl Default for Endpoint {
    fn default() -> Self {
        Self {
            noise: f32::MAX,
            elapsed: Duration::ZERO,
            spoken: Duration::ZERO,
            quiet: Duration::ZERO,
        }
    }
}

impl Endpoint {
    /// Feeds one slice `step` long at loudness `level` (RMS, 0 to 1) and says
    /// whether the speaker has finished.
    pub(crate) fn update(&mut self, level: f32, step: Duration) -> bool {
        self.elapsed += step;
        if self.elapsed <= LEARN {
            self.noise = self.noise.min(level);
            return false;
        }
        if level > (self.noise * ABOVE_NOISE).max(SPEECH_FLOOR) {
            self.spoken += step;
            self.quiet = Duration::ZERO;
            return false;
        }
        self.noise = self.noise * 0.98 + level * 0.02;
        self.quiet += step;
        self.spoken >= MIN_SPEECH && self.quiet >= SILENCE_STOP
    }
}

#[cfg(test)]
#[path = "vad_tests.rs"]
mod tests;
