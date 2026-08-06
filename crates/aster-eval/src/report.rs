//! Aggregates turns into the numbers worth acting on, and renders them.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use serde::{Deserialize, Serialize};

use crate::stats::Dist;
use crate::turn::Turn;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Report {
    pub sessions: usize,
    pub turns: usize,
    pub rounds: usize,
    pub calls: usize,
    /// Calls per tool round. 1.0 means the model never batches, and every
    /// call costs its own model round-trip.
    pub batch_factor: f64,
    pub single_call_rate: f64,
    /// Share of tool results that told the model nothing.
    pub barren_rate: f64,
    pub rounds_per_turn: Dist,
    pub model_rtt: Dist,
    pub turn_wall: Dist,
    pub turn_active: Dist,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub tools: Vec<ToolStat>,
    pub models: Vec<ModelStat>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolStat {
    pub name: String,
    pub calls: usize,
    pub barren: usize,
    pub barren_rate: f64,
    pub duration: Dist,
    pub result_chars: Dist,
}

/// Accumulator for building a [`ToolStat`] across turns.
#[derive(Default)]
struct ToolAccum {
    calls: usize,
    barren: usize,
    durations: Vec<f64>,
    result_chars: Vec<f64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModelStat {
    pub model: String,
    pub turns: usize,
    pub rounds: usize,
    pub calls: usize,
    pub batch_factor: f64,
    pub single_call_rate: f64,
    pub rtt: Dist,
}

impl Report {
    pub fn build(sessions: usize, turns: &[Turn]) -> Self {
        let batches: Vec<usize> = turns
            .iter()
            .flat_map(|t| t.batches.iter().copied())
            .collect();
        let calls = batches.iter().sum::<usize>();
        let barren = turns
            .iter()
            .flat_map(|t| t.calls.iter())
            .filter(|c| c.barren)
            .count();
        let results = turns.iter().flat_map(|t| t.calls.iter()).count();

        Self {
            sessions,
            turns: turns.len(),
            rounds: batches.len(),
            calls,
            batch_factor: ratio(calls, batches.len()),
            single_call_rate: ratio(batches.iter().filter(|&&n| n == 1).count(), batches.len()),
            barren_rate: ratio(barren, results),
            rounds_per_turn: Dist::new(turns.iter().map(|t| t.rounds() as f64).collect()),
            model_rtt: Dist::new(
                turns
                    .iter()
                    .flat_map(|t| t.latencies.iter().copied())
                    .collect(),
            ),
            turn_wall: Dist::new(turns.iter().map(Turn::wall).collect()),
            turn_active: Dist::new(turns.iter().map(Turn::active).collect()),
            prompt_tokens: turns.iter().map(|t| t.prompt_tokens).sum(),
            completion_tokens: turns.iter().map(|t| t.completion_tokens).sum(),
            tools: tool_stats(turns),
            models: model_stats(turns),
        }
    }

    /// Headline metrics against an earlier report, so a change can be shown to
    /// have helped rather than argued about.
    pub fn compare(&self, baseline: &Report) -> Vec<Delta> {
        vec![
            Delta::new(
                "batch factor",
                baseline.batch_factor,
                self.batch_factor,
                true,
            ),
            Delta::new(
                "single-call rounds",
                baseline.single_call_rate,
                self.single_call_rate,
                false,
            ),
            Delta::new(
                "barren results",
                baseline.barren_rate,
                self.barren_rate,
                false,
            ),
            Delta::new(
                "rounds/turn p50",
                baseline.rounds_per_turn.p50,
                self.rounds_per_turn.p50,
                false,
            ),
            Delta::new(
                "model rtt p50",
                baseline.model_rtt.p50,
                self.model_rtt.p50,
                false,
            ),
            Delta::new(
                "active turn p90",
                baseline.turn_active.p90,
                self.turn_active.p90,
                false,
            ),
        ]
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Delta {
    pub metric: String,
    pub before: f64,
    pub after: f64,
    /// Whether a rise is the good direction, so callers do not have to know.
    pub higher_is_better: bool,
}

impl Delta {
    fn new(metric: &str, before: f64, after: f64, higher_is_better: bool) -> Self {
        Self {
            metric: metric.to_string(),
            before,
            after,
            higher_is_better,
        }
    }

    /// `None` when the metric did not move, which is neither a win nor a
    /// regression and should not be reported as one.
    pub fn improved(&self) -> Option<bool> {
        if self.after == self.before {
            return None;
        }
        Some(match self.higher_is_better {
            true => self.after > self.before,
            false => self.after < self.before,
        })
    }
}

fn tool_stats(turns: &[Turn]) -> Vec<ToolStat> {
    let mut by_name: BTreeMap<&str, ToolAccum> = BTreeMap::new();
    for call in turns.iter().flat_map(|t| t.calls.iter()) {
        let entry = by_name.entry(&call.tool).or_default();
        entry.calls += 1;
        entry.barren += usize::from(call.barren);
        entry.durations.extend(call.duration);
        entry.result_chars.push(call.result_chars as f64);
    }
    let mut stats: Vec<ToolStat> = by_name
        .into_iter()
        .map(|(name, acc)| ToolStat {
            name: name.to_string(),
            calls: acc.calls,
            barren: acc.barren,
            barren_rate: ratio(acc.barren, acc.calls),
            duration: Dist::new(acc.durations),
            result_chars: Dist::new(acc.result_chars),
        })
        .collect();
    stats.sort_by(|a, b| b.calls.cmp(&a.calls).then(a.name.cmp(&b.name)));
    stats
}

fn model_stats(turns: &[Turn]) -> Vec<ModelStat> {
    let mut by_model: BTreeMap<&str, Vec<&Turn>> = BTreeMap::new();
    for turn in turns {
        by_model
            .entry(turn.model.as_deref().unwrap_or("unknown"))
            .or_default()
            .push(turn);
    }
    let mut stats: Vec<ModelStat> = by_model
        .into_iter()
        .map(|(model, turns)| {
            let batches: Vec<usize> = turns
                .iter()
                .flat_map(|t| t.batches.iter().copied())
                .collect();
            ModelStat {
                model: model.to_string(),
                turns: turns.len(),
                rounds: batches.len(),
                calls: batches.iter().sum(),
                batch_factor: ratio(batches.iter().sum::<usize>(), batches.len()),
                single_call_rate: ratio(batches.iter().filter(|&&n| n == 1).count(), batches.len()),
                rtt: Dist::new(
                    turns
                        .iter()
                        .flat_map(|t| t.latencies.iter().copied())
                        .collect(),
                ),
            }
        })
        .collect();
    stats.sort_by(|a, b| b.rounds.cmp(&a.rounds).then(a.model.cmp(&b.model)));
    stats
}

fn ratio(part: usize, whole: usize) -> f64 {
    match whole {
        0 => 0.0,
        _ => part as f64 / whole as f64,
    }
}

/// The report as a fixed-width block, which is what a terminal and a diff in a
/// pull request both want.
pub fn render(report: &Report) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "sessions {}   turns {}   rounds {}   calls {}",
        report.sessions, report.turns, report.rounds, report.calls
    );
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "batch factor     {:.2}   ({:.0}% of rounds carry one call)",
        report.batch_factor,
        report.single_call_rate * 100.0
    );
    let _ = writeln!(
        out,
        "rounds/turn      {:.0}      p90 {:.0}   max {:.0}",
        report.rounds_per_turn.p50, report.rounds_per_turn.p90, report.rounds_per_turn.max
    );
    let _ = writeln!(
        out,
        "model rtt        {:.1}s    p90 {:.1}s  max {:.1}s",
        report.model_rtt.p50, report.model_rtt.p90, report.model_rtt.max
    );
    let _ = writeln!(
        out,
        "active turn      {:.1}s    p90 {:.1}s  max {:.1}s",
        report.turn_active.p50, report.turn_active.p90, report.turn_active.max
    );
    let _ = writeln!(
        out,
        "barren results   {:.1}%   of every tool result",
        report.barren_rate * 100.0
    );
    let _ = writeln!(
        out,
        "tokens           {} in / {} out",
        report.prompt_tokens, report.completion_tokens
    );

    if !report.tools.is_empty() {
        let _ = writeln!(
            out,
            "\ntool             calls  barren     p50     p90    total"
        );
        for tool in &report.tools {
            let _ = writeln!(
                out,
                "{:<16} {:>5} {:>6.1}% {:>6.2}s {:>6.2}s {:>7.1}s",
                tool.name,
                tool.calls,
                tool.barren_rate * 100.0,
                tool.duration.p50,
                tool.duration.p90,
                tool.duration.total
            );
        }
    }

    if report.models.len() > 1 {
        let _ = writeln!(
            out,
            "\nmodel                              rounds  batch  single   rtt p50"
        );
        for model in &report.models {
            let _ = writeln!(
                out,
                "{:<34} {:>6} {:>6.2} {:>6.0}% {:>8.1}s",
                model.model,
                model.rounds,
                model.batch_factor,
                model.single_call_rate * 100.0,
                model.rtt.p50
            );
        }
    }
    out
}

pub fn render_comparison(deltas: &[Delta]) -> String {
    let mut out = String::from("\nvs baseline\n");
    for delta in deltas {
        let _ = writeln!(
            out,
            "  {:<20} {:>8.2} -> {:>8.2}  {}",
            delta.metric,
            delta.before,
            delta.after,
            match delta.improved() {
                Some(true) => "better",
                Some(false) => "worse",
                None => "unchanged",
            }
        );
    }
    out
}

#[cfg(test)]
#[path = "tests/report_test.rs"]
mod tests;
