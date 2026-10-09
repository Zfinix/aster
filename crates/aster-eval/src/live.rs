//! Live evals: run aster against fixed cases and compare models, where
//! [`crate::turn`] can only grade sessions that already happened. Each case
//! spawns `aster --stream` and is judged from the NDJSON it emits.

use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::stats::Dist;

/// A case that has not answered by now is stuck, and waiting longer only
/// spends money on the loop that got it there.
const CASE_TIMEOUT: Duration = Duration::from_secs(600);

/// One thing a model has to get right, and the tool budget it gets.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Case {
    pub name: String,
    pub prompt: String,
    #[serde(default)]
    pub must_mention: Option<String>,
    #[serde(default)]
    pub calls: Vec<String>,
    #[serde(default)]
    pub avoids: Vec<String>,
    #[serde(default)]
    pub at_most: Vec<(String, usize)>,
    #[serde(default)]
    pub max_rounds: Option<usize>,
}

impl Case {
    /// Every way the transcript broke the case, in plain words. Empty is a pass.
    fn failures(&self, transcript: &Transcript) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(failure) = &transcript.failure {
            out.push(format!("aster stopped: {failure}"));
        } else if !transcript.finished {
            out.push("aster exited without an answer".into());
        }
        let count = |tool: &str| transcript.tools.iter().filter(|t| *t == tool).count();
        for tool in &self.calls {
            if count(tool) == 0 {
                out.push(format!("never called {tool}"));
            }
        }
        for tool in &self.avoids {
            if count(tool) > 0 {
                out.push(format!("called {tool}"));
            }
        }
        for (tool, cap) in &self.at_most {
            let n = count(tool);
            if n > *cap {
                out.push(format!("{tool} called {n} times, limit {cap}"));
            }
        }
        if let Some(cap) = self.max_rounds
            && transcript.rounds > cap
        {
            out.push(format!("{} rounds, limit {cap}", transcript.rounds));
        }
        if let Some(text) = &self.must_mention
            && !transcript
                .text
                .to_lowercase()
                .contains(&text.to_lowercase())
        {
            out.push(format!("answer never mentions {text:?}"));
        }
        out
    }
}

/// The cases shipped with aster. Each encodes a harness fix, so a regression in
/// that fix fails an eval rather than going unnoticed until the next report.
pub fn default_cases() -> Vec<Case> {
    vec![
        Case {
            name: "reaches a gitignored directory".into(),
            prompt: "What activation events does the vscode extension in this repo declare? \
                     Answer from its package.json."
                .into(),
            must_mention: Some("onStartupFinished".into()),
            calls: Vec::new(),
            avoids: vec!["edit_file".into()],
            // `editors/` is gitignored. Before the ignore fallback every
            // discovery tool came back empty and the model kept guessing, at
            // nine calls and still wrong. The round cap holds whichever tool
            // the model picks, where the find_files cap alone passes vacuously.
            at_most: vec![("find_files".into(), 5)],
            max_rounds: Some(4),
        },
        Case {
            name: "does not re-run an identical search".into(),
            prompt: "Which function in crates/aster-tools decides whether a directory is \
                     skipped? Name it and the file it lives in."
                .into(),
            must_mention: Some("is_skipped".into()),
            // `explore` answers this in one call, so no tool is required; the
            // round cap keeps the search_files cap from passing vacuously.
            calls: Vec::new(),
            avoids: vec!["edit_file".into()],
            at_most: vec![("search_files".into(), 3)],
            max_rounds: Some(3),
        },
        Case {
            name: "batches lookups into one round".into(),
            prompt: "Compare how find.rs and list.rs in crates/aster-tools handle .gitignore. \
                     Read both before answering."
                .into(),
            must_mention: None,
            // No required tool: `explore` reads files too, and the round cap is
            // what this case is about.
            calls: Vec::new(),
            avoids: vec!["edit_file".into()],
            at_most: vec![("read_file".into(), 3)],
            max_rounds: Some(2),
        },
        Case {
            name: "runs git in the working directory".into(),
            // Recent commits are already in the environment snapshot, so the
            // question reaches past them to force a real git call.
            prompt: "What is the subject of the commit that first added \
                     crates/aster-eval/Cargo.toml?"
                .into(),
            must_mention: Some("session-grading".into()),
            calls: vec!["run_command".into()],
            avoids: vec!["edit_file".into()],
            // Finding 4: an invented `cd /Users/...` fails and buys a retry.
            // One git command answers this from the working directory.
            at_most: vec![("run_command".into(), 3)],
            max_rounds: Some(3),
        },
        Case {
            name: "finds a file by name".into(),
            // A name no loaded doc mentions, so the answer has to be looked up.
            prompt: "Which crate has a file named alerts.rs, and what is it for? \
                     One sentence."
                .into(),
            must_mention: Some("aster-remote".into()),
            calls: Vec::new(),
            avoids: vec!["edit_file".into()],
            // Finding 5: a name aimed at the content search comes back empty
            // and the model rephrases it several times.
            at_most: vec![("search_files".into(), 2)],
            max_rounds: Some(3),
        },
    ]
}

/// What one `aster --stream` run did, rebuilt from its events.
#[derive(Debug, Clone, Default)]
struct Transcript {
    text: String,
    tools: Vec<String>,
    rounds: usize,
    cost_usd: Option<f64>,
    failure: Option<String>,
    finished: bool,
    in_round: bool,
}

impl Transcript {
    /// aster emits every tool call of a round before any of its results, so a
    /// call that follows a result opens the next round.
    fn push(&mut self, event: &Value) {
        let text = |key: &str| event.get(key).and_then(Value::as_str);
        match text("type") {
            Some("token" | "text") => self.text.push_str(text("content").unwrap_or_default()),
            Some("tool_call") => {
                if !self.in_round {
                    self.rounds += 1;
                    self.in_round = true;
                }
                self.tools
                    .push(text("name").unwrap_or("unknown").to_string());
            }
            Some("tool_result") => self.in_round = false,
            Some("done") => {
                // Only an endpoint that ignored `stream` leaves the reply out of
                // the tokens, but appending it twice cannot hide a mention.
                self.text.push('\n');
                self.text.push_str(text("reply").unwrap_or_default());
                self.cost_usd = event
                    .pointer("/usage/estimated_cost_usd")
                    .and_then(Value::as_f64);
                self.finished = true;
            }
            Some("error") => {
                self.failure = Some(text("message").unwrap_or("unknown error").to_string());
                self.finished = true;
            }
            _ => {}
        }
    }
}

/// What one run of one case did: enough to see why it passed or failed.
#[derive(Debug, Clone)]
pub struct Outcome {
    pub case: String,
    pub failures: Vec<String>,
    pub tools: Vec<String>,
    pub rounds: usize,
    pub secs: f64,
    pub cost_usd: Option<f64>,
}

impl Outcome {
    pub fn passed(&self) -> bool {
        self.failures.is_empty()
    }

    /// `find_files×2 read_file×1`, in first-call order.
    pub fn tool_summary(&self) -> String {
        let mut order: Vec<&String> = Vec::new();
        for tool in &self.tools {
            if !order.contains(&tool) {
                order.push(tool);
            }
        }
        order
            .iter()
            .map(|tool| {
                let n = self.tools.iter().filter(|t| *t == *tool).count();
                format!("{tool}×{n}")
            })
            .collect::<Vec<_>>()
            .join(" ")
    }
}

#[derive(Debug, Clone)]
pub struct ModelRun {
    pub model: String,
    pub outcomes: Vec<Outcome>,
}

impl ModelRun {
    pub fn passed(&self) -> usize {
        self.outcomes.iter().filter(|o| o.passed()).count()
    }

    pub fn failed(&self) -> usize {
        self.outcomes.len() - self.passed()
    }

    pub fn ok(&self) -> bool {
        self.failed() == 0 && !self.outcomes.is_empty()
    }

    /// Total tool calls across every run: the volume that decides how slow and
    /// expensive a model feels, independent of whether it passed.
    pub fn calls(&self) -> usize {
        self.outcomes.iter().map(|o| o.tools.len()).sum()
    }

    pub fn rounds(&self) -> usize {
        self.outcomes.iter().map(|o| o.rounds).sum()
    }

    pub fn secs(&self) -> f64 {
        self.outcomes.iter().map(|o| o.secs).sum()
    }

    /// `None` when no run reported usage, so an unpriced model is never shown
    /// as the cheap one.
    pub fn cost_usd(&self) -> Option<f64> {
        let priced: Vec<f64> = self.outcomes.iter().filter_map(|o| o.cost_usd).collect();
        (!priced.is_empty()).then(|| priced.iter().sum())
    }
}

/// The binary under test and the checkout it runs in.
#[derive(Debug, Clone)]
pub struct Target {
    pub bin: PathBuf,
    pub repo: PathBuf,
}

fn run_case(target: &Target, case: &Case, model: Option<&str>) -> Result<Outcome> {
    let mut command = Command::new(&target.bin);
    command.args(["--stream", "--permission-mode", "auto"]);
    if let Some(model) = model {
        command.args(["--model", model]);
    }
    let started = Instant::now();
    let mut child = command
        .arg(&case.prompt)
        .current_dir(&target.repo)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("starting {}", target.bin.display()))?;

    let stdout = child.stdout.take().context("aster has no stdout")?;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    // Drained on its own thread: a chatty stderr would otherwise fill the pipe
    // and stall aster mid-turn.
    let mut stderr = child.stderr.take().context("aster has no stderr")?;
    let stderr = std::thread::spawn(move || {
        let mut text = String::new();
        stderr.read_to_string(&mut text).ok();
        text
    });

    let mut transcript = Transcript::default();
    loop {
        let left = CASE_TIMEOUT.saturating_sub(started.elapsed());
        match rx.recv_timeout(left) {
            Ok(line) => {
                if let Ok(event) = serde_json::from_str(&line) {
                    transcript.push(&event);
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                child.kill().ok();
                transcript.failure = Some(format!("no answer after {}s", CASE_TIMEOUT.as_secs()));
                break;
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    let status = child.wait().context("waiting for aster")?;
    let stderr = stderr.join().unwrap_or_default();
    if !transcript.finished && transcript.failure.is_none() {
        transcript.failure = Some(
            stderr
                .lines()
                .rfind(|line| !line.trim().is_empty())
                .map_or_else(|| format!("exited with {status}"), str::to_string),
        );
    }

    Ok(Outcome {
        case: case.name.clone(),
        failures: case.failures(&transcript),
        tools: transcript.tools,
        rounds: transcript.rounds,
        secs: started.elapsed().as_secs_f64(),
        cost_usd: transcript.cost_usd,
    })
}

fn run_model(
    target: &Target,
    cases: &[Case],
    model: Option<&str>,
    runs: usize,
) -> Result<ModelRun> {
    let label = model.unwrap_or("(configured)");
    let mut outcomes = Vec::with_capacity(cases.len() * runs);
    for case in cases {
        for run in 1..=runs {
            let outcome = run_case(target, case, model)?;
            eprintln!(
                "{label}  {} {} ({run}/{runs})  {:.1}s",
                if outcome.passed() { "pass" } else { "FAIL" },
                case.name,
                outcome.secs
            );
            outcomes.push(outcome);
        }
    }
    Ok(ModelRun {
        model: label.to_string(),
        outcomes,
    })
}

/// Run every model in turn, each case `runs` times. Sequential on purpose: a
/// parallel sweep would have models competing for the same checkout and skew
/// every timing.
pub fn sweep(
    target: &Target,
    cases: &[Case],
    models: &[String],
    runs: usize,
) -> Result<Vec<ModelRun>> {
    if cases.is_empty() {
        bail!("no cases to run");
    }
    if runs == 0 {
        bail!("--runs must be at least 1");
    }
    if models.is_empty() {
        return Ok(vec![run_model(target, cases, None, runs)?]);
    }
    models
        .iter()
        .map(|model| run_model(target, cases, Some(model), runs))
        .collect()
}

/// Per-case medians first, then one summary row per model. Single runs are
/// noise, so a case reads as `passed k/n` with the median of its runs.
pub fn render_live(runs: &[ModelRun]) -> String {
    if runs.is_empty() {
        return "no models run\n".into();
    }
    let mut out = String::new();
    for run in runs {
        out.push_str(&format!("\n{}\n", run.model));
        let mut names: Vec<&str> = Vec::new();
        for outcome in &run.outcomes {
            if !names.contains(&outcome.case.as_str()) {
                names.push(&outcome.case);
            }
        }
        let width = names.iter().map(|n| n.len()).max().unwrap_or(0).max(4);
        for name in names {
            let mine: Vec<&Outcome> = run.outcomes.iter().filter(|o| o.case == name).collect();
            let passed = mine.iter().filter(|o| o.passed()).count();
            let median =
                |f: fn(&Outcome) -> f64| Dist::new(mine.iter().map(|o| f(o)).collect()).p50;
            out.push_str(&format!(
                "  {} {passed}/{}  {name:<width$}  {:>6.1}s  rounds {:<3} {}\n",
                if passed == mine.len() { "pass" } else { "FAIL" },
                mine.len(),
                median(|o| o.secs),
                median(|o| o.rounds as f64),
                mine.first().map(|o| o.tool_summary()).unwrap_or_default()
            ));
            for (i, outcome) in mine.iter().enumerate() {
                for failure in &outcome.failures {
                    out.push_str(&format!("         run {}: {failure}\n", i + 1));
                }
            }
        }
    }

    let width = runs.iter().map(|r| r.model.len()).max().unwrap_or(5).max(5);
    out.push_str(&format!(
        "\n{:<width$}  pass  fail  calls  rounds     wall      cost\n",
        "model"
    ));
    for run in runs {
        let cost = match run.cost_usd() {
            Some(cost) => format!("${cost:.4}"),
            None => "     ?".into(),
        };
        out.push_str(&format!(
            "{:<width$}  {:>4}  {:>4}  {:>5}  {:>6}  {:>6.1}s  {:>8}\n",
            run.model,
            run.passed(),
            run.failed(),
            run.calls(),
            run.rounds(),
            run.secs(),
            cost
        ));
    }

    // Cheapest correct model, which is the question a sweep is asked to answer.
    if runs.len() > 1
        && let Some(best) = runs
            .iter()
            .filter(|r| r.ok())
            .min_by_key(|r| (r.rounds(), r.calls(), (r.secs() * 1000.0) as u64))
    {
        out.push_str(&format!(
            "\nfewest rounds with every case passing: {}\n",
            best.model
        ));
    }
    out
}

/// The checkout the cases are written against: the enclosing git worktree, not
/// the current directory, so running from inside a crate still finds the repo.
pub fn repo_root(from: &Path) -> PathBuf {
    from.ancestors()
        .find(|dir| dir.join(".git").exists())
        .unwrap_or(from)
        .to_path_buf()
}

#[cfg(test)]
#[path = "tests/live_test.rs"]
mod tests;
