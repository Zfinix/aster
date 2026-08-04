//! Percentile summaries over a sample. Samples are one entry per tool call or
//! round, so sorting is cheaper than maintaining a sketch.

use serde::{Deserialize, Serialize};

/// Nearest-rank percentiles. `total` is the sum, which for durations is the
/// wall time a category actually cost.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Dist {
    pub n: usize,
    pub p50: f64,
    pub p90: f64,
    pub p99: f64,
    pub max: f64,
    pub mean: f64,
    pub total: f64,
}

impl Dist {
    pub fn new(mut values: Vec<f64>) -> Self {
        if values.is_empty() {
            return Self::default();
        }
        values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let total: f64 = values.iter().sum();
        Self {
            n: values.len(),
            p50: percentile(&values, 0.50),
            p90: percentile(&values, 0.90),
            p99: percentile(&values, 0.99),
            max: *values.last().unwrap_or(&0.0),
            mean: total / values.len() as f64,
            total,
        }
    }
}

/// `sorted` must be ascending and non-empty.
fn percentile(sorted: &[f64], q: f64) -> f64 {
    let rank = (q * sorted.len() as f64).ceil() as usize;
    sorted[rank.saturating_sub(1).min(sorted.len() - 1)]
}

#[cfg(test)]
#[path = "stats_tests.rs"]
mod tests;
