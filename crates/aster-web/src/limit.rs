//! A sliding-window rate limit. Calls past the window's capacity wait for the
//! oldest one to age out rather than failing, because a tool call that returns
//! "slow down" costs the model a whole turn to retry.

use std::collections::VecDeque;
use std::time::Duration;

use tokio::sync::Mutex;
use tokio::time::Instant;

const WINDOW: Duration = Duration::from_secs(60);

pub struct RateLimit {
    capacity: usize,
    recent: Mutex<VecDeque<Instant>>,
}

impl RateLimit {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            recent: Mutex::new(VecDeque::new()),
        }
    }

    /// Take one slot, sleeping until the window has room.
    pub async fn acquire(&self) {
        loop {
            let wait = {
                let mut recent = self.recent.lock().await;
                let now = Instant::now();
                while recent.front().is_some_and(|t| now - *t >= WINDOW) {
                    recent.pop_front();
                }
                match recent.len() < self.capacity {
                    true => {
                        recent.push_back(now);
                        return;
                    }
                    // Safe to unwrap: a full window has a front.
                    false => WINDOW - (now - *recent.front().expect("window is full")),
                }
            };
            tracing::debug!(?wait, "web search rate limit reached; waiting");
            tokio::time::sleep(wait).await;
        }
    }
}

#[cfg(test)]
#[path = "tests/limit_test.rs"]
mod tests;
