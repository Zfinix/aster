//! `aster learn`: score the last turn of a session and rewrite the skill for
//! that task, so the next run of it takes fewer rounds than the best so far.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use aster_eval::{RunScore, Turn, turns};
use aster_persist::{MemoryStore, SessionMeta, SessionTranscript};
use chrono::{DateTime, Utc};
use clap::Args;
use serde::{Deserialize, Serialize};

#[derive(Debug, Args)]
pub(crate) struct LearnArgs {
    /// Session id to learn from; the newest session for this directory when omitted.
    #[arg(long)]
    pub session: Option<String>,
    /// Which user turn to score: `last`, or a 1-based index.
    #[arg(long, default_value = "last")]
    pub turn: String,
    /// Turns with fewer tool calls than this are not worth a model call.
    #[arg(long, default_value_t = DEFAULT_MIN_CALLS)]
    pub min_calls: usize,
    /// Learn even when the turn is short or learning is switched off.
    #[arg(long)]
    pub force: bool,
    /// Print the report and write nothing.
    #[arg(long)]
    pub dry_run: bool,
}

pub(crate) const DEFAULT_MIN_CALLS: usize = 6;
const MODEL_TIMEOUT: Duration = Duration::from_secs(60);
const DIGEST_CHARS: usize = 32_000;
const DIGEST_HEAD_CHARS: usize = 8_000;
const ARGS_CHARS: usize = 300;
const RESULT_CHARS: usize = 400;
const TEXT_CHARS: usize = 600;
const BODIES_SHOWN: usize = 6;
const LESSONS_KEPT: usize = 5;
const FACTS_KEPT: usize = 2;
const LEDGER: &str = "runs.jsonl";
const BEST_LINE: &str = "> Best so far:";

const LEARN_SYSTEM: &str = "\
You are the post-run coach for an autonomous agent. You get one finished turn: the \
user's request, every tool call the agent made with its arguments and a clipped \
result, the final reply, and a score line. Your job is to make the NEXT run of the \
same task take fewer model rounds and fewer tool calls, by writing or rewriting a \
skill the agent will load before it starts.

First decide whether this turn was a repeatable task: something the user could ask \
for again in nearly the same words (play a game, post a message, book a thing, run a \
report, check a status). A question, a chat, a one-off explanation, or a turn that \
never got past exploring is NOT a task: return {\"task\": null}.

If it is a task, first check the LEARNED SKILLS listed below. If one of them already \
covers this task, set \"update\" to its name and rewrite that skill's body: keep what \
worked, fold in what this run taught you, drop what did not. Do not create a \
near-duplicate under a new slug. Only when nothing below covers the task, name it with \
a short kebab-case slug (max 4 words) and set \"update\" to null.

Write the skill body as numbered imperative rules, nothing else. It must contain:
1. The exact fastest command sequence found in this run or earlier ones, with literal \
arguments (coordinates, package names, element indices), so the next run can batch \
them without exploring. Prefer one batched call over several rounds.
2. Every pitfall this run hit and how to avoid it: calls that returned an error, taps \
that reported \"changed: +0 -0\", empty results, calls repeated with identical \
arguments, screenshots that added nothing, scripts that took more than a few \
seconds. State each as \"do X, never Y\".
3. The check that proves the task is done, so the agent stops right after it.
Under 1500 words. No essays, no restating the request, no \"Best so far\" line (the \
harness adds it). Never include secrets, tokens, phone numbers, or chat contents.

The description is the trigger the agent scans on every message: one line starting \
\"Use when the user asks to ...\", then the concrete phrasings.

Also list in \"waste\" the 1-4 biggest reasons this run was slower than it needed to be, \
one short sentence each. Optionally up to 2 durable \"facts\" about the environment (not \
about this skill) worth remembering across all tasks; usually none.

Respond with ONLY a JSON object, no fences:
{\"task\": \"<slug>\" | null, \"update\": \"<name of a learned skill to rewrite>\" | null, \
\"title\": \"<3-6 words>\", \"description\": \"Use when the user asks to ...\", \
\"skill\": \"<markdown body: numbered imperative rules>\", \"waste\": [\"...\"], \
\"facts\": [{\"name\": \"<kebab>\", \"description\": \"<one line>\", \"body\": \"<1-2 sentences>\"}]}";

#[derive(Debug, Default, Deserialize)]
pub(crate) struct Reflection {
    pub task: Option<String>,
    /// A learned skill this turn refines. Only skills this loop wrote carry a
    /// ledger, so an update naming anything else is ignored.
    #[serde(default)]
    pub update: Option<String>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub skill: Option<String>,
    #[serde(default)]
    pub waste: Vec<String>,
    #[serde(default)]
    pub facts: Vec<Fact>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct Fact {
    pub name: String,
    pub description: String,
    pub body: String,
}

/// One line of a skill's `runs.jsonl`.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct RunRecord {
    pub session: String,
    pub turn: usize,
    pub ts: DateTime<Utc>,
    pub score: RunScore,
    pub best: bool,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case", tag = "outcome")]
pub(crate) enum Outcome {
    Skipped { reason: String },
    AlreadyLearned,
    New,
    Improved,
    Updated,
    Regressed,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct LearnReport {
    pub ok: bool,
    pub session: String,
    pub turn: usize,
    pub task: Option<String>,
    pub score: RunScore,
    pub best: Option<RunScore>,
    #[serde(flatten)]
    pub outcome: Outcome,
    pub skill_path: Option<String>,
}

pub(crate) async fn run(args: LearnArgs) -> Result<()> {
    let repo_root = std::env::current_dir().context("reading the working directory")?;
    let settings = crate::settings::Settings::load(Some(&repo_root))?;
    let store = crate::persist::store()?;
    let transcript = match &args.session {
        Some(id) => store.resume(&repo_root, id)?,
        None => store
            .latest(&repo_root)?
            .context("no session recorded for this directory")?,
    };
    let (index, turn) = select_turn(&transcript, &args.turn)?;
    let score = RunScore::of(&turn);
    let session = transcript.meta.id.clone();
    let report =
        |outcome: Outcome, best: Option<RunScore>, task: Option<String>, path: Option<PathBuf>| {
            LearnReport {
                ok: true,
                session: session.clone(),
                turn: index,
                task,
                score,
                best,
                outcome,
                skill_path: path.map(|p| p.display().to_string()),
            }
        };

    let switched_off =
        std::env::var("ASTER_LEARN").is_ok_and(|v| v == "0") || settings.agent.learn == Some(false);
    if switched_off && !args.force {
        return emit(report(skipped("learning is off"), None, None, None));
    }
    if score.calls < args.min_calls && !args.force {
        return emit(report(
            skipped(&format!(
                "{} tool calls, under the {} that makes a task",
                score.calls, args.min_calls
            )),
            None,
            None,
            None,
        ));
    }

    let home = crate::persist::home()?;
    let learned = learned_skills(&home.join("skills"));
    let client = session_client(&settings, &transcript.meta)?;
    let user = user_prompt(&score, &learned, &turn_digest(&transcript, index));
    let reply = tokio::time::timeout(MODEL_TIMEOUT, client.complete(LEARN_SYSTEM, &user, 0.2))
        .await
        .context("the reflection took too long")??;
    let reflection = parse_reflection(&reply)?;

    if args.dry_run {
        let root = home.join("skills");
        let best = reflection
            .update
            .as_deref()
            .and_then(|name| learned_skills(&root).into_iter().find(|s| s.name == name))
            .and_then(|s| s.best)
            .or_else(|| {
                reflection
                    .task
                    .as_deref()
                    .and_then(|slug| Ledger::at(&root, slug).best())
            });
        eprintln!("{reply}");
        return emit(report(skipped("dry run"), best, reflection.task, None));
    }
    let memory = store.memory();
    let applied = apply(
        &home.join("skills"),
        &memory,
        &session,
        index,
        score,
        &reflection,
    )?;
    emit(report(
        applied.outcome,
        applied.best,
        reflection.task,
        applied.path,
    ))
}

/// The endpoint and model the turn ran on, with that endpoint's key. The
/// settings decide only for a session that recorded no endpoint.
pub(crate) fn session_client(
    settings: &crate::settings::Settings,
    meta: &SessionMeta,
) -> Result<aster_ai::AiClient> {
    match (&meta.base_url, &meta.model) {
        (Some(base_url), Some(model)) => {
            crate::config::provider::client_for(settings, base_url, model)
        }
        _ => crate::config::provider::resolve_client(settings, meta.model.as_deref()),
    }
}

fn skipped(reason: &str) -> Outcome {
    Outcome::Skipped {
        reason: reason.to_string(),
    }
}

fn emit(report: LearnReport) -> Result<()> {
    if crate::json_mode() {
        println!("{}", serde_json::to_string(&report)?);
        return Ok(());
    }
    let what = match &report.outcome {
        Outcome::Skipped { reason } => format!("skipped: {reason}"),
        Outcome::AlreadyLearned => "already learned".into(),
        Outcome::New => "new skill".into(),
        Outcome::Improved => "procedure updated".into(),
        Outcome::Updated => "procedure refined".into(),
        Outcome::Regressed => "lesson noted".into(),
    };
    match &report.task {
        Some(task) => println!("{task}: {} · {what}", report.score.line()),
        None => println!("{} · {what}", report.score.line()),
    }
    Ok(())
}

/// The turn to score: `last`, or a 1-based index into the user turns.
pub(crate) fn select_turn(transcript: &SessionTranscript, which: &str) -> Result<(usize, Turn)> {
    let mut all = turns(transcript);
    if all.is_empty() {
        bail!("the session has no turns");
    }
    let index = match which {
        "last" => all.len(),
        n => n
            .parse::<usize>()
            .ok()
            .filter(|n| (1..=all.len()).contains(n))
            .with_context(|| format!("--turn must be `last` or 1..={}", all.len()))?,
    };
    Ok((index, all.swap_remove(index - 1)))
}

/// What the model gets to look at: the request, each call with its arguments
/// and a clipped result, and the reply, for the one turn being scored.
pub(crate) fn turn_digest(transcript: &SessionTranscript, index: usize) -> String {
    let mut seen_users = 0;
    let mut out = String::new();
    let mut call_no = 0;
    for message in transcript.messages() {
        if message.role == "user" {
            seen_users += 1;
        }
        if seen_users != index {
            continue;
        }
        let content = message.content.as_deref().unwrap_or_default();
        match message.role.as_str() {
            "user" => out.push_str(&format!("<user>{}</user>\n", request_text(content))),
            "assistant" => {
                if !content.trim().is_empty() {
                    out.push_str(&format!(
                        "<assistant>{}</assistant>\n",
                        clip(content, TEXT_CHARS)
                    ));
                }
                for call in &message.tool_calls {
                    call_no += 1;
                    out.push_str(&format!(
                        "<call n={call_no} tool={}>{}</call>\n",
                        call.function.name,
                        clip(&call.function.arguments, ARGS_CHARS)
                    ));
                }
            }
            "tool" => out.push_str(&format!("<result>{}</result>\n", clip_result(content))),
            _ => {}
        }
    }
    if out.len() > DIGEST_CHARS {
        let tail_from = out.len() - (DIGEST_CHARS - DIGEST_HEAD_CHARS);
        let head = cut_at(&out, DIGEST_HEAD_CHARS);
        let tail = &out[floor_char(&out, tail_from)..];
        out = format!("{head}\n[... middle of the turn omitted ...]\n{tail}");
    }
    out
}

/// The request as the person wrote it: without the chat's steering prefix and
/// the `[msg N]` tag the bridge adds.
fn request_text(content: &str) -> String {
    let text = match content.find("[msg ") {
        Some(at) => &content[at..],
        None => content,
    };
    let text = match text.strip_prefix("[msg ") {
        Some(rest) => rest.split_once('\n').map(|(_, t)| t).unwrap_or(""),
        None => text,
    };
    clip(text.trim(), TEXT_CHARS)
}

/// The receipt lines are the evidence a coach needs; keep them even when the
/// rest of a long map is cut.
fn clip_result(content: &str) -> String {
    let kept: Vec<&str> = content
        .lines()
        .filter(|l| {
            l.starts_with("receipt:") || l.starts_with("changed:") || l.starts_with("error:")
        })
        .collect();
    let head = clip(content, RESULT_CHARS);
    let extra: Vec<&str> = kept.into_iter().filter(|l| !head.contains(l)).collect();
    if extra.is_empty() {
        head
    } else {
        format!("{head} … {}", extra.join(" · "))
    }
}

fn clip(text: &str, max: usize) -> String {
    let flat = text.trim();
    if flat.len() <= max {
        return flat.to_string();
    }
    format!("{}…", cut_at(flat, max))
}

fn cut_at(text: &str, max: usize) -> &str {
    &text[..floor_char(text, max)]
}

fn floor_char(text: &str, mut at: usize) -> usize {
    while at > 0 && !text.is_char_boundary(at) {
        at -= 1;
    }
    at
}

/// A skill this loop wrote earlier: it has a ledger next to it.
pub(crate) struct Learned {
    pub name: String,
    pub description: String,
    pub body: String,
    pub dir: PathBuf,
    pub best: Option<RunScore>,
    pub runs: usize,
    pub touched: u64,
}

pub(crate) fn learned_skills(root: &Path) -> Vec<Learned> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    let mut found: Vec<Learned> = entries
        .flatten()
        .filter_map(|entry| {
            let dir = entry.path();
            let ledger = Ledger::at(root, dir.file_name()?.to_str()?);
            if !ledger.path.exists() {
                return None;
            }
            let raw = fs::read_to_string(dir.join("SKILL.md")).ok()?;
            let (front, body) = split_frontmatter(&raw);
            let runs = ledger.load();
            let dir_name = dir.file_name()?.to_str()?.to_string();
            Some(Learned {
                name: front_value(front, "name").unwrap_or(dir_name),
                description: front_value(front, "description").unwrap_or_default(),
                body: body.to_string(),
                dir,
                best: runs.iter().map(|r| r.score).min_by_key(RunScore::key),
                runs: runs.len(),
                touched: fs::metadata(&ledger.path)
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs())
                    .unwrap_or(0),
            })
        })
        .collect();
    found.sort_by_key(|s| std::cmp::Reverse(s.touched));
    found
}

fn split_frontmatter(raw: &str) -> (&str, &str) {
    let Some(rest) = raw.strip_prefix("---\n") else {
        return ("", raw);
    };
    match rest.split_once("\n---\n") {
        Some((front, body)) => (front, body),
        None => ("", raw),
    }
}

fn front_value(front: &str, key: &str) -> Option<String> {
    front
        .lines()
        .find_map(|l| l.strip_prefix(key)?.strip_prefix(':').map(str::trim))
        .map(|v| v.trim_matches('"').to_string())
        .filter(|v| !v.is_empty())
}

pub(crate) fn user_prompt(score: &RunScore, learned: &[Learned], digest: &str) -> String {
    let mut out = format!("THIS RUN: {}\n\n", score.line());
    out.push_str("LEARNED SKILLS (set \"update\" to one of these when it is the same task):\n");
    if learned.is_empty() {
        out.push_str("- (none yet)\n");
    }
    for skill in learned {
        out.push_str(&format!("- {}: {}\n", skill.name, skill.description));
    }
    for skill in learned.iter().take(BODIES_SHOWN) {
        out.push('\n');
        match &skill.best {
            Some(best) => out.push_str(&format!(
                "PREVIOUS BEST for {}: {} (from {} earlier runs). Rewrite the skill so the next run beats it.\n",
                skill.name,
                best.line(),
                skill.runs
            )),
            None => out.push_str(&format!("No earlier run of {} is recorded.\n", skill.name)),
        }
        out.push_str(&format!(
            "EXISTING SKILL BODY for {} (rewrite it; keep what worked, remove what did not):\n{}\n",
            skill.name,
            skill.body.trim()
        ));
    }
    out.push_str("\nTURN DIGEST:\n");
    out.push_str(digest);
    out
}

pub(crate) fn parse_reflection(raw: &str) -> Result<Reflection> {
    let json = raw
        .find('{')
        .and_then(|start| raw.rfind('}').map(|end| &raw[start..=end]))
        .context("the reflection had no JSON object in it")?;
    let mut reflection: Reflection =
        serde_json::from_str(json).context("the reflection was not the JSON shape asked for")?;
    reflection.task = reflection
        .task
        .map(|t| slugify(&t))
        .filter(|t| !t.is_empty());
    reflection.update = reflection
        .update
        .map(|u| slugify(&u))
        .filter(|u| !u.is_empty());
    Ok(reflection)
}

fn slugify(text: &str) -> String {
    let mut out = String::new();
    for c in text.trim().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_matches('-').chars().take(64).collect()
}

pub(crate) struct Ledger {
    pub path: PathBuf,
}

impl Ledger {
    pub fn at(root: &Path, slug: &str) -> Self {
        Self {
            path: root.join(slug).join(LEDGER),
        }
    }

    pub fn load(&self) -> Vec<RunRecord> {
        fs::read_to_string(&self.path)
            .unwrap_or_default()
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect()
    }

    pub fn best(&self) -> Option<RunScore> {
        self.load()
            .iter()
            .map(|r| r.score)
            .min_by_key(RunScore::key)
    }

    pub fn seen(&self, session: &str, turn: usize) -> bool {
        self.load()
            .iter()
            .any(|r| r.session == session && r.turn == turn)
    }

    pub fn append(&self, record: &RunRecord) -> Result<()> {
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir)?;
        }
        let mut raw = fs::read_to_string(&self.path).unwrap_or_default();
        raw.push_str(&serde_json::to_string(record)?);
        raw.push('\n');
        fs::write(&self.path, raw).with_context(|| format!("writing {}", self.path.display()))
    }
}

pub(crate) struct Applied {
    pub outcome: Outcome,
    pub best: Option<RunScore>,
    pub path: Option<PathBuf>,
}

/// Write what the reflection found under rules the model cannot bend: the body
/// moves only on a run at least as good as the best, and a turn is learned once.
pub(crate) fn apply(
    root: &Path,
    memory: &MemoryStore,
    session: &str,
    turn: usize,
    score: RunScore,
    reflection: &Reflection,
) -> Result<Applied> {
    let Some(slug) = reflection.task.as_deref() else {
        return Ok(Applied {
            outcome: skipped("not a repeatable task"),
            best: None,
            path: None,
        });
    };
    // An update may only name a skill this loop wrote: the ledger is the proof,
    // and it keeps a rewrite away from anything the user authored.
    let target = reflection
        .update
        .as_deref()
        .filter(|name| name != &slug)
        .and_then(|name| learned_skills(root).into_iter().find(|s| s.name == name));
    let name = target.as_ref().map_or(slug, |t| t.name.as_str());
    let ledger = match &target {
        Some(t) => Ledger {
            path: t.dir.join(LEDGER),
        },
        None => Ledger::at(root, slug),
    };
    let best = ledger.best();
    let path = match &target {
        Some(t) => t.dir.join("SKILL.md"),
        None => root.join(slug).join("SKILL.md"),
    };
    if ledger.seen(session, turn) {
        return Ok(Applied {
            outcome: Outcome::AlreadyLearned,
            best,
            path: Some(path),
        });
    }
    let existing = fs::read_to_string(&path).ok();
    let title = reflection
        .title
        .clone()
        .unwrap_or_else(|| name.replace('-', " "));
    let description = reflection
        .description
        .clone()
        .or_else(|| {
            existing
                .as_deref()
                .and_then(|raw| front_value(split_frontmatter(raw).0, "description"))
        })
        .unwrap_or_else(|| format!("Use when the user asks to {}", name.replace('-', " ")));
    let now = Utc::now();
    let is_best = best.is_none_or(|b| score.key() <= b.key());
    let outcome = match (&existing, is_best, best) {
        (None, _, _) => Outcome::New,
        (Some(_), true, Some(b)) if score.key() == b.key() => Outcome::Updated,
        (Some(_), true, _) => Outcome::Improved,
        (Some(_), false, _) => Outcome::Regressed,
    };
    let body = match (&outcome, &reflection.skill, &existing) {
        (Outcome::Regressed, _, Some(raw)) => with_lesson(
            split_frontmatter(raw).1,
            &now,
            &score,
            best.unwrap_or(score),
            &reflection.waste,
        ),
        (_, Some(skill), _) => skill.clone(),
        (_, None, Some(raw)) => split_frontmatter(raw).1.to_string(),
        (_, None, None) => {
            return Ok(Applied {
                outcome: skipped("the reflection named a task but wrote no skill"),
                best,
                path: None,
            });
        }
    };
    let record_best = if is_best {
        score
    } else {
        best.unwrap_or(score)
    };
    let composed = compose_skill(
        name,
        &title,
        &description,
        &body,
        &record_best,
        session,
        &now,
    );
    fs::create_dir_all(path.parent().context("skill path has no parent")?)?;
    ledger.append(&RunRecord {
        session: session.to_string(),
        turn,
        ts: now,
        score,
        best: is_best,
    })?;
    fs::write(&path, composed).with_context(|| format!("writing {}", path.display()))?;
    for fact in reflection.facts.iter().take(FACTS_KEPT) {
        let name = slugify(&fact.name);
        if name.is_empty() {
            continue;
        }
        if let Err(e) = memory.remember_sourced(&name, &fact.description, &fact.body, session) {
            tracing::warn!("could not remember {name}: {e:#}");
        }
    }
    Ok(Applied {
        outcome,
        best,
        path: Some(path),
    })
}

/// The skill file the loader accepts, with the record line the harness owns
/// placed right after the title and any copy the model wrote of it removed.
pub(crate) fn compose_skill(
    slug: &str,
    title: &str,
    description: &str,
    body: &str,
    best: &RunScore,
    session: &str,
    when: &DateTime<Utc>,
) -> String {
    let record = format!(
        "{BEST_LINE} {} rounds, {} calls, {}s active (session {session}, {}). Beat it.",
        best.rounds,
        best.calls,
        best.active_secs,
        when.format("%Y-%m-%d")
    );
    let mut lines: Vec<&str> = body
        .lines()
        .filter(|l| !l.trim_start().starts_with(BEST_LINE))
        .collect();
    let heading = format!("# {title}");
    let has_heading = lines.first().is_some_and(|l| l.starts_with("# "));
    if !has_heading {
        lines.insert(0, &heading);
    }
    let mut out = format!(
        "---\nname: {slug}\ndescription: {}\n---\n",
        description.replace('\n', " ").replace("---", "").trim()
    );
    out.push_str(lines.first().copied().unwrap_or(heading.as_str()));
    out.push_str("\n\n");
    out.push_str(&record);
    out.push_str("\n\n");
    let rest = lines.get(1..).map(|r| r.join("\n")).unwrap_or_default();
    out.push_str(rest.trim_start_matches('\n'));
    out.push('\n');
    out
}

fn with_lesson(
    body: &str,
    when: &DateTime<Utc>,
    score: &RunScore,
    best: RunScore,
    waste: &[String],
) -> String {
    let reasons: Vec<&str> = waste
        .iter()
        .map(|w| w.trim().trim_end_matches('.'))
        .take(2)
        .collect();
    let bullet = format!(
        "- {}: regression, {} rounds vs best {}: {}",
        when.format("%Y-%m-%d"),
        score.rounds,
        best.rounds,
        if reasons.is_empty() {
            "no reason recorded".to_string()
        } else {
            reasons.join("; ")
        }
    );
    let (before, lessons) = match body.find("\n## Lessons") {
        Some(at) => (
            &body[..at],
            body[at..]
                .lines()
                .skip(1)
                .filter(|l| l.starts_with("- "))
                .map(str::to_string)
                .collect::<Vec<_>>(),
        ),
        None => (body, Vec::new()),
    };
    let mut kept = vec![bullet];
    kept.extend(lessons);
    kept.truncate(LESSONS_KEPT);
    format!("{}\n\n## Lessons\n{}\n", before.trim_end(), kept.join("\n"))
}

#[cfg(test)]
#[path = "tests/learn_test.rs"]
mod tests;
