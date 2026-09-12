//! One turn as a single comparable score, so a later run of the same task can
//! be told whether it was faster and by how much.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::turn::{Call, Turn};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct RunScore {
    pub rounds: usize,
    pub calls: usize,
    pub wall_secs: u64,
    pub active_secs: u64,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub errors: usize,
    pub barren: usize,
    pub no_change: usize,
    /// Calls whose tool and arguments already ran earlier in the same turn.
    pub repeated: usize,
    pub screenshots: usize,
}

impl RunScore {
    pub fn of(turn: &Turn) -> Self {
        Self {
            rounds: turn.rounds(),
            calls: turn.calls.len(),
            wall_secs: turn.wall().round() as u64,
            active_secs: turn.active().round() as u64,
            prompt_tokens: turn.prompt_tokens,
            completion_tokens: turn.completion_tokens,
            errors: turn.calls.iter().filter(|c| c.error).count(),
            barren: turn.calls.iter().filter(|c| c.barren).count(),
            no_change: turn.calls.iter().filter(|c| c.no_change).count(),
            repeated: repeated(&turn.calls),
            screenshots: turn.calls.iter().filter(|c| is_screenshot(c)).count(),
        }
    }

    /// Rounds first, because every round is a model wait; then calls; then
    /// the seconds aster itself spent. Lower is better.
    pub fn key(&self) -> (usize, usize, u64) {
        (self.rounds, self.calls, self.active_secs)
    }

    pub fn better_than(&self, other: &Self) -> bool {
        self.key() < other.key()
    }

    pub fn line(&self) -> String {
        format!(
            "{} rounds, {} calls, {}s active / {}s wall, {} errors, {} no-change, {} repeated, {} shots, {}k+{}k tokens",
            self.rounds,
            self.calls,
            self.active_secs,
            self.wall_secs,
            self.errors,
            self.no_change,
            self.repeated,
            self.screenshots,
            self.prompt_tokens / 1000,
            self.completion_tokens / 1000,
        )
    }
}

fn repeated(calls: &[Call]) -> usize {
    let mut seen = HashSet::new();
    calls
        .iter()
        .filter(|c| !seen.insert((c.tool.as_str(), c.arguments.as_str())))
        .count()
}

fn is_screenshot(call: &Call) -> bool {
    call.tool == "run_command"
        && call.arguments.contains("asterctl")
        && call.arguments.contains("\"shot\"")
}

#[cfg(test)]
#[path = "tests/score_test.rs"]
mod tests;
