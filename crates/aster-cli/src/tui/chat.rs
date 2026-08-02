//! The chat TUI behind `aster` and `aster chat --tui`. Finished output goes
//! into the terminal's own scrollback ([`super::terminal`]); only the bottom
//! pane (composer, status, modals) is managed, and it draws on demand.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync;
use std::time::Instant;

use anyhow::{Context, Result};
use aster_ai::{AiClient, ChatMessage, Effort};
use aster_persist::{MessageEvent, Store};
use aster_policy::{Grants, Mode, PermissionsConfig, Policy};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};
use serde_json::Value;
use tokio::sync::mpsc;

use super::bottom_pane::{
    BottomPane, CommandDesc, InputResult, ModelPickerView, SelectionItem, scan_mentions,
};
use super::guard::TuiGuard;
use super::helpers::{human_count, short_path};
use super::markdown::{self, MarkdownStream};
use super::render::Renderable;
use super::terminal::{Tui, TuiEvent};
use super::{history, theme};
use crate::chat::{
    Answer, ApprovalRequest, QuestionRequest, Resume, SessionCtx, UiRequest, UiSender,
};
use crate::persist::Recorder;

type ChatTurn = tokio::task::JoinHandle<Result<(String, Vec<String>, Option<Vec<ChatMessage>>)>>;

/// Pressing ctrl-c once arms the quit; a second press within this window exits.
const QUIT_WINDOW: std::time::Duration = std::time::Duration::from_secs(2);

/// How long the YOLO takeover animation holds the screen.
const TAKEOVER: std::time::Duration = std::time::Duration::from_millis(1250);

/// Read-only tools whose consecutive calls collapse into one `Explored` cell.
const READ_ONLY: &[&str] = &[
    "read_file",
    "list_files",
    "search_files",
    "find_files",
    "recall",
    "read_skill",
];

/// Side effects routed back from the bottom pane's views.
#[derive(Clone)]
pub(super) enum AppEvent {
    SetMode(Mode),
    SetEffort(Effort),
    ApprovalDecided {
        answer: Answer,
        scope: Option<PathBuf>,
    },
    QuestionAnswered(String),
    /// The sandbox-bypass prompt came back yes.
    YoloConfirmed,
    SessionPicked(String),
    ModelChanged(String),
    /// The provider's catalog, fetched off the loop so `/model` never blocks
    /// the UI on a round trip.
    ModelsLoaded(Vec<String>),
    /// The catalog request failed; `/model` says so instead of hanging on
    /// "fetching models…" with the reason buried in a log file.
    ModelsFailed(String),
    /// A provider chosen from `/provider`: endpoint plus its example model.
    ProviderPicked {
        base_url: String,
        model: String,
    },
    /// The composer's `@` query changed; the owner searches off the loop.
    MentionQueried(String),
    /// Ranked matches for a query; the pane drops stale ones.
    MentionResults {
        query: String,
        paths: Vec<String>,
    },
    /// A manual `/compact` finished; swap in the folded history.
    Compacted {
        history: Vec<ChatMessage>,
        summary: String,
        replaces_through: usize,
    },
    CompactFailed(String),
}

/// Search for `@`-mention matches off the loop; the result lands as
/// [`AppEvent::MentionResults`]. Typing never blocks on the walk.
fn spawn_mention_search(
    tx: &mpsc::UnboundedSender<AppEvent>,
    root: &std::path::Path,
    query: String,
) {
    let tx = tx.clone();
    let root = root.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let paths = scan_mentions(&root, &query);
        let _ = tx.send(AppEvent::MentionResults { query, paths });
    });
}

#[allow(clippy::too_many_arguments)]
pub async fn run_chat(
    mut client: AiClient,
    repo_root: std::path::PathBuf,
    allow_edits: bool,
    perms: PermissionsConfig,
    seed: Option<String>,
    resume: Resume,
    mcp: Option<crate::mcp::McpRuntime>,
    limits: crate::chat::Limits,
) -> Result<()> {
    if matches!(resume, Resume::Pick) && seed.as_deref().is_some_and(|s| !s.trim().is_empty()) {
        anyhow::bail!("--resume opens a session picker, so it cannot also take a prompt");
    }
    let _guard = TuiGuard::install(super::terminal::restore_raw);
    theme::set(theme::Theme::DEFAULT);
    // Idle layout is four rows: gap, status, composer, footer. Anchoring
    // smaller would make the first draw grow the viewport, and that growth
    // scrolls blank rows into the middle of the transcript.
    let mut tui = Tui::new(4)?;
    // Wipe the terminal so Aster owns the full screen from the start.
    tui.clear_screen()?;

    // Depth 1: the agent awaits each request before proposing the next.
    let (approval_tx, mut approval_rx) = mpsc::channel::<UiRequest>(1);
    let (events_tx, mut events_rx) = mpsc::channel::<TurnEvent>(64);
    let (app_tx, mut app_rx) = mpsc::unbounded_channel::<AppEvent>();

    let policy_for = |mode: Mode| -> Result<sync::Arc<Policy>> {
        let mut c = perms.clone();
        c.mode = mode;
        Ok(sync::Arc::new(Policy::compile(&c).context(
            "invalid `permissions` config in aster.yaml (bad glob?)",
        )?))
    };
    // A config that forbids edits, or a run started read-only, pins the session
    // to `plan`; the mode picker cannot leave it.
    let edits_locked = !allow_edits || !perms.mode.can_edit();
    let mode = if edits_locked { Mode::Plan } else { perms.mode };

    let mut app = ChatApp::new(
        mode,
        client.effort(),
        edits_locked,
        client.model.clone(),
        SessionPermissions {
            plan: policy_for(Mode::Plan)?,
            manual: policy_for(Mode::Manual)?,
            auto: policy_for(Mode::Auto)?,
            edit: policy_for(Mode::Edit)?,
            grants: sync::Arc::new(crate::chat::configured_grants(&perms, &repo_root)),
        },
        approval_tx,
        events_tx,
    );
    app.repo_root = repo_root.clone();
    app.width = tui.width() as usize;
    app.instructions = sync::Arc::new(crate::instructions::discover(&repo_root));
    app.mcp = mcp;
    app.limits = limits;
    app.provider_base_url = client.base_url().to_string();

    let mut pane: BottomPane<AppEvent> = BottomPane::new(
        CHAT_COMMANDS,
        "Message Aster…  (/ for commands)",
        tui.frame_requester(),
        app_tx.clone(),
        |answer, scope| AppEvent::ApprovalDecided { answer, scope },
        AppEvent::MentionQueried,
    );

    let endpoint = crate::init::provider_label(client.base_url());
    app.emit(history::welcome(
        &[
            ("model", app.model.clone()),
            ("provider", endpoint),
            ("cwd", short_path(&repo_root)),
            ("mode", app.mode.as_str().to_string()),
            ("effort", client.effort().to_string()),
        ],
        app.width,
    ));

    if let Ok(store) = crate::persist::store() {
        match resume_or_new(&store, &repo_root, &resume) {
            Ok(Some((recorder, seeded))) => {
                app.recorder = Some(recorder);
                app.load_history(seeded);
            }
            // `Pick`: the picker opens below, and the choice arrives as an event.
            Ok(None) => {}
            // A named session that does not exist is the user's mistake, not a
            // store problem to log and carry on from.
            Err(e) if matches!(resume, Resume::Id(_)) => return Err(e),
            Err(e) => tracing::warn!("could not open session store: {e:#}"),
        }
        app.store = Some(store);
    }

    if matches!(resume, Resume::Pick) {
        app.open_session_picker(&mut pane);
    }

    let mut turn: Option<ChatTurn> = None;
    if let Some(seed) = seed.filter(|s| !s.trim().is_empty()) {
        turn = Some(app.submit(&seed, &[], &client, &repo_root));
        pane.set_task_running(true);
    }

    let frames = tui.frame_requester();
    frames.schedule_now();

    loop {
        if app.clear_requested {
            app.clear_requested = false;
            tui.clear_screen()?;
            frames.schedule_now();
        }
        while let Some(block) = app.queue.pop_front() {
            tui.insert_history(block)?;
            frames.schedule_now();
        }
        if app.should_quit {
            break;
        }
        // Only grab the mouse while a menu or picker is up; the rest of the
        // time the terminal keeps its own selection and copy.
        tui.set_mouse(pane.wants_mouse());

        tokio::select! {
            ev = tui.next_event() => match ev {
                TuiEvent::Key(key) => {
                    if let Flow::Quit =
                        on_key(&mut app, &mut pane, key, &mut client, &mut turn, &repo_root)
                    {
                        break;
                    }
                    frames.schedule_now();
                }
                TuiEvent::Mouse(m) => {
                    if let InputResult::Command(cmd) = pane.handle_mouse(m) {
                        app.handle_command(&cmd, &mut client, &mut pane);
                    }
                    frames.schedule_now();
                }
                TuiEvent::Paste(text) => {
                    pane.handle_paste(text);
                    frames.schedule_now();
                }
                TuiEvent::Resize => {
                    tui.resized()?;
                    app.width = tui.width() as usize;
                    frames.schedule_now();
                }
                TuiEvent::Draw => {
                    if app
                        .takeover
                        .as_ref()
                        .is_some_and(|t| t.start.elapsed() >= TAKEOVER)
                    {
                        app.finish_takeover();
                        continue;
                    }
                    app.usage = Some(client.usage_snapshot());
                    if let Some(flash) = app.usage_flash() {
                        app.flash = Some(flash);
                    }
                    draw(&mut tui, &app, &pane)?;
                    if app.takeover.is_some() || theme::is_transitioning() {
                        frames.schedule_in(std::time::Duration::from_millis(16));
                    }
                }
            },
            Some(ev) = events_rx.recv() => {
                app.on_turn_event(ev);
                let queue_label = match app.running.len() {
                    0 => None,
                    1 => app.running.last().map(|t| t.label.to_lowercase()),
                    n => app.running.last().map(|t| format!("{} (+{} queued)", t.label.to_lowercase(), n - 1)),
                };
                pane.set_status_detail(queue_label);
                frames.schedule_now();
            }
            Some(req) = approval_rx.recv() => {
                match req {
                    UiRequest::Approval(req) => app.on_approval_request(req, &mut pane),
                    UiRequest::PlanApproval(req) => app.on_plan_approval_request(req, &mut pane),
                    UiRequest::Question(req) => app.on_question_request(req, &mut pane),
                }
                frames.schedule_now();
            }
            Some(ev) = app_rx.recv() => {
                match ev {
                    AppEvent::ModelsLoaded(models) => app.open_model_picker(models, &mut pane),
                    AppEvent::MentionQueried(query) => {
                        spawn_mention_search(&app_tx, &repo_root, query)
                    }
                    AppEvent::MentionResults { query, paths } => {
                        pane.set_mention_results(&query, paths)
                    }
                    AppEvent::SetMode(Mode::Yolo) => app.confirm_yolo(&mut pane),
                    ev => app.on_app_event(ev, &mut client),
                }
                // A mode change swaps the theme here; without a frame the
                // transition would expire before anything redrew.
                frames.schedule_now();
            }
            res = wait_turn(&mut turn) => {
                match res {
                    Ok(Ok((reply, edited, compacted))) => {
                        app.finish_turn(&reply, &edited, compacted);
                    }
                    Ok(Err(e)) => app.fail_turn(&format!("{e:#}")),
                    Err(e) => app.fail_turn(&format!("chat failed: {e}")),
                }
                pane.set_task_running(false);
                frames.schedule_now();
            }
        }
    }

    // Leave the last of the conversation in the scrollback on the way out.
    while let Some(block) = app.queue.pop_front() {
        tui.insert_history(block)?;
    }
    Ok(())
}

/// Resolve a finished turn; parks forever while no turn is running.
async fn wait_turn(
    turn: &mut Option<ChatTurn>,
) -> std::result::Result<
    Result<(String, Vec<String>, Option<Vec<ChatMessage>>)>,
    tokio::task::JoinError,
> {
    match turn {
        Some(t) => {
            let res = t.await;
            *turn = None;
            res
        }
        None => std::future::pending().await,
    }
}

/// One YOLO animation frame: a shockwave with a fading banner and color gradient.
fn takeover_frame(
    entering: bool,
    elapsed: std::time::Duration,
    width: usize,
) -> Vec<Line<'static>> {
    const ROWS: usize = 12;
    let t = (elapsed.as_secs_f32() / TAKEOVER.as_secs_f32()).clamp(0.0, 1.0);
    let frame_n = (elapsed.as_millis() / 45) as u64;
    let w = width.clamp(20, 240);

    let (bright, hot, warm, ember) = match entering {
        true => (
            Color::Rgb(0xff, 0x45, 0x45),
            Color::Rgb(0xf0, 0x38, 0x38),
            Color::Rgb(0x8a, 0x1f, 0x1f),
            Color::Rgb(0x4a, 0x12, 0x12),
        ),
        false => (
            Color::Rgb(0xf8, 0xcb, 0x66),
            Color::Rgb(0xf2, 0x76, 0x4f),
            Color::Rgb(0x8a, 0x4a, 0x2a),
            Color::Rgb(0x3f, 0x2a, 0x1a),
        ),
    };

    let cx = w as f32 / 2.0;
    let cy = ROWS as f32 / 2.0;
    // x squashed: a terminal cell is over twice as tall as it is wide, so the
    // wave reads as a circle rather than a flat ellipse.
    let max_r = ((cx / 2.2).powi(2) + cy * cy).sqrt();
    let front = t * 1.45 * max_r;

    let banner: Vec<char> = match entering {
        true => "☠  Y O L O   M O D E  ☠",
        false => "✳  G U A R D R A I L S   O N  ✳",
    }
    .chars()
    .collect();

    (0..ROWS)
        .map(|row| {
            let spans = (0..w)
                .map(|col| {
                    let dx = (col as f32 - cx) / 2.2;
                    let dy = row as f32 - cy + 0.5;
                    let d = front - (dx * dx + dy * dy).sqrt();
                    if row == ROWS / 2 && d > 3.0 {
                        let start = w.saturating_sub(banner.len()) / 2;
                        if col >= start && col < start + banner.len() {
                            let ch = banner[col - start];
                            let lit = t > 0.55 || noise(frame_n, u64::MAX, col as u64) > 0.25;
                            if ch != ' ' && lit {
                                return Span::styled(
                                    ch.to_string(),
                                    Style::default().fg(bright).add_modifier(Modifier::BOLD),
                                );
                            }
                        }
                    }
                    let jitter = noise(frame_n, row as u64, col as u64);
                    let (ch, fg) = if d < 0.0 {
                        (' ', ember)
                    } else if d < 1.3 {
                        ('█', bright)
                    } else if d < 2.6 {
                        ('▓', hot)
                    } else if d < 4.2 {
                        (if jitter > 0.5 { '▒' } else { '▓' }, warm)
                    } else if d < 6.5 {
                        (if jitter > 0.6 { '░' } else { '▒' }, ember)
                    } else if jitter > 0.93 {
                        ('·', ember)
                    } else {
                        (' ', ember)
                    };
                    match ch {
                        ' ' => Span::raw(" "),
                        _ => Span::styled(ch.to_string(), Style::default().fg(fg)),
                    }
                })
                .collect::<Vec<_>>();
            Line::from(spans)
        })
        .collect()
}

/// Deterministic per-cell noise, so a frame is stable within itself but
/// flickers frame to frame.
fn noise(a: u64, b: u64, c: u64) -> f32 {
    let mut x = a
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add(b.wrapping_mul(0xBF58_476D_1CE4_E5B9))
        .wrapping_add(c.wrapping_mul(0x94D0_49BB_1331_11EB));
    x ^= x >> 31;
    x = x.wrapping_mul(0xD6E8_FEB8_6659_FD93);
    x ^= x >> 27;
    (x & 0xFFFF) as f32 / 65535.0
}

fn draw(tui: &mut Tui, app: &ChatApp, pane: &BottomPane<AppEvent>) -> Result<()> {
    let width = tui.width();
    if let Some(t) = &app.takeover {
        let lines = takeover_frame(t.entering, t.start.elapsed(), width as usize);
        let h = lines.len() as u16;
        tui.draw(h, |frame| {
            Paragraph::new(lines).render(frame.area(), frame.buffer_mut());
        })?;
        return Ok(());
    }
    let pane_h = pane.desired_height(width);
    let footer = app.footer_line();
    tui.draw(pane_h + 1, |frame| {
        let area = frame.area();
        let pane_area = Rect {
            height: pane_h.min(area.height),
            ..area
        };
        let footer_area = Rect {
            y: area.y + pane_area.height,
            height: area.height.saturating_sub(pane_area.height).min(1),
            ..area
        };
        pane.render(pane_area, frame.buffer_mut());
        footer.render(footer_area, frame.buffer_mut());
        match pane.cursor_pos(pane_area) {
            Some((x, y)) => frame.set_cursor_position(Position::new(x, y)),
            None => frame.set_cursor_position(Position::new(area.x, area.y)),
        }
    })?;
    Ok(())
}

enum Flow {
    Continue,
    Quit,
}

/// App-level keys first (interrupt, quit, the modes panel); everything else
/// belongs to the bottom pane.
fn on_key(
    app: &mut ChatApp,
    pane: &mut BottomPane<AppEvent>,
    key: KeyEvent,
    client: &mut AiClient,
    turn: &mut Option<ChatTurn>,
    repo_root: &std::path::Path,
) -> Flow {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let interrupt = (ctrl && key.code == KeyCode::Char('c')) || key.code == KeyCode::Esc;

    if !pane.has_active_view() {
        if interrupt {
            if turn.is_some() {
                abort(app, turn, pane);
                return Flow::Continue;
            }
            if !pane.composer.is_empty() {
                pane.composer.clear();
                app.quit_armed = None;
                return Flow::Continue;
            }
            // Quitting takes two presses: esc is muscle memory for "dismiss",
            // and one stray keystroke should not end the session.
            if app.quit_armed.is_some_and(|at| at.elapsed() < QUIT_WINDOW) {
                return Flow::Quit;
            }
            app.quit_armed = Some(Instant::now());
            app.flash = Some("press again to quit".into());
            return Flow::Continue;
        }
        app.quit_armed = None;
        // Shift+tab steps to the next mode, unless it is standing in for
        // shift+enter mid-composition. `/mode` opens the full panel.
        if key.code == KeyCode::BackTab && pane.composer.is_empty() {
            app.cycle_mode();
            return Flow::Continue;
        }
    } else if ctrl && key.code == KeyCode::Char('c') {
        pane.handle_key(key, app.width as u16);
        return Flow::Quit;
    }

    match pane.handle_key(key, app.width as u16) {
        InputResult::Submitted { text, refs } => {
            *turn = Some(app.submit(&text, &refs, client, repo_root));
            pane.set_task_running(true);
            app.flash = None;
        }
        InputResult::Command(cmd) => {
            app.handle_command(&cmd, client, pane);
        }
        InputResult::Busy => {
            app.flash = Some("still working · esc to interrupt, then send".into());
            return Flow::Continue;
        }
        InputResult::None => {
            app.flash = None;
        }
    }
    Flow::Continue
}

/// The user message, plus a `[@name]: /full/path` block listing every path
/// folded out of it. The model resolves a `[@name]` token from this block, and
/// the block stays in the transcript so a resumed session still resolves it.
fn render_user_content(text: &str, refs: &[(String, String)]) -> String {
    if refs.is_empty() {
        return text.to_string();
    }
    let mut out = String::with_capacity(
        text.len()
            + refs
                .iter()
                .map(|(m, p)| m.len() + p.len() + 3)
                .sum::<usize>(),
    );
    out.push_str(text);
    out.push_str("\n\n");
    for (mark, path) in refs {
        out.push_str(mark);
        out.push_str(": ");
        out.push_str(path);
        out.push('\n');
    }
    out
}

fn abort(app: &mut ChatApp, turn: &mut Option<ChatTurn>, pane: &mut BottomPane<AppEvent>) {
    if let Some(t) = turn.take() {
        t.abort();
    }
    app.end_message();
    app.running.clear();
    pane.set_task_running(false);
    let width = app.width;
    app.emit(history::notice("turn stopped", width));
}

/// Open the transcript this run records into. Sessions always start clean;
/// only an explicit `--continue` reopens the repo's latest session and seeds
/// its prior turns. Returns the live transcript handle and the seeded
/// user/assistant turns to replay into the view.
/// `New` and `Pick` return nothing: a transcript opened before the first
/// message would leave an empty file behind every time aster is started.
fn resume_or_new(
    store: &Store,
    repo_root: &std::path::Path,
    resume: &Resume,
) -> Result<Option<(Recorder, Vec<ChatMessage>)>> {
    let prev = match resume {
        Resume::New | Resume::Pick => return Ok(None),
        Resume::Latest => store.latest(repo_root)?,
        Resume::Id(id) => Some(
            store
                .resume(repo_root, id)
                .with_context(|| format!("no session {id:?} for this repo"))?,
        ),
    };
    let Some(prev) = prev else {
        return Ok(None);
    };
    let messages = prev.to_chat_messages();
    let writer = store.resume_writer(repo_root, &prev.meta.id)?;
    Ok(Some((sync::Arc::new(sync::Mutex::new(writer)), messages)))
}

/// Decode one event from the agent's `ChatEventSink` NDJSON into a UI event.
fn decode_turn_event(event: &Value) -> Option<TurnEvent> {
    match event.get("type")?.as_str()? {
        "token" | "text" => Some(TurnEvent::Token(
            event.get("content")?.as_str()?.to_string(),
        )),
        "tool_call" => Some(TurnEvent::ToolCall {
            id: event.get("id")?.as_str()?.to_string(),
            name: event.get("name")?.as_str()?.to_string(),
            args: event
                .get("arguments")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
        }),
        "tool_result" => Some(TurnEvent::ToolResult {
            id: event.get("id")?.as_str()?.to_string(),
            result: event
                .get("result")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            error: event.get("error").and_then(Value::as_bool).unwrap_or(false),
        }),
        "notice" => Some(TurnEvent::Notice(
            event.get("message")?.as_str()?.to_string(),
        )),
        _ => None,
    }
}

/// Friendly one-line label for a tool call, matching the desktop's stepLabel.
fn step_label(name: &str, args: &str) -> String {
    let parsed: Value = serde_json::from_str(args).unwrap_or(Value::Null);
    let s = |key: &str| parsed.get(key).and_then(Value::as_str).unwrap_or("");
    match name {
        "read_file" => match s("path") {
            "" => "Read file".to_string(),
            path => format!("Read {path}"),
        },
        "list_files" => match s("dir") {
            "" => "Listed the project root".to_string(),
            dir => format!("Listed {dir}"),
        },
        "search_files" => format!("Searched \u{201c}{}\u{201d}", s("query")),
        "find_files" => format!("Found files matching {}", s("pattern")),
        "run_command" => match command_line(&parsed) {
            None => "Ran a command".to_string(),
            Some(line) => format!("Ran {line}"),
        },
        "edit_file" => match s("path") {
            "" => "Edited file".to_string(),
            path => format!("Edited {path}"),
        },
        "remember" => "Saved to memory".to_string(),
        "recall" => format!("Recalled {}", s("name")),
        "read_skill" => format!("Read skill {}", s("name")),
        other => other.replace('_', " "),
    }
}

/// The command line as invoked, so the label says what actually ran rather than
/// just naming the binary.
fn command_line(args: &Value) -> Option<String> {
    let binary = args.get("command").and_then(Value::as_str)?;
    let rest = args
        .get("args")
        .and_then(Value::as_array)
        .map(|list| list.iter().filter_map(Value::as_str).collect::<Vec<_>>())
        .unwrap_or_default();
    match rest.is_empty() {
        true => Some(binary.to_string()),
        false => Some(format!("{binary} {}", rest.join(" "))),
    }
}

/// A tool that answered with a hint instead of results: the path was a wrong
/// guess. Worth a mark on the collapsed label, not a red failure cell.
fn missed(result: &str) -> bool {
    result.starts_with("note: ") && result.contains("does not exist")
}

fn arg_str(args: &str, key: &str) -> String {
    serde_json::from_str::<Value>(args)
        .ok()
        .and_then(|v| v.get(key).and_then(Value::as_str).map(str::to_string))
        .unwrap_or_default()
}

/// Live progress from the running turn, decoded from the agent's
/// `ChatEventSink` NDJSON (the same wire the `--stream` front-ends consume).
enum TurnEvent {
    Token(String),
    ToolCall {
        id: String,
        name: String,
        args: String,
    },
    ToolResult {
        id: String,
        result: String,
        error: bool,
    },
    /// Something the harness did that the user has to know about, e.g. the
    /// turn being cut short at the tool-round cap.
    Notice(String),
}

/// A tool call the agent has made but not yet finished.
struct RunningTool {
    id: String,
    name: String,
    label: String,
    path: String,
}

/// One compiled policy per gating mode, since the picker switches between
/// them per turn, plus the shared out-of-repo grants.
struct SessionPermissions {
    plan: sync::Arc<Policy>,
    manual: sync::Arc<Policy>,
    auto: sync::Arc<Policy>,
    edit: sync::Arc<Policy>,
    grants: sync::Arc<Grants>,
}

impl SessionPermissions {
    fn policy(&self, mode: Mode) -> sync::Arc<Policy> {
        match mode {
            Mode::Plan => self.plan.clone(),
            Mode::Manual => self.manual.clone(),
            Mode::Auto => self.auto.clone(),
            Mode::Edit | Mode::Yolo => self.edit.clone(),
        }
    }
}

/// Picker order: plan → manual → auto → edit → yolo.
const MODE_ORDER: [Mode; 5] = [Mode::Plan, Mode::Manual, Mode::Auto, Mode::Edit, Mode::Yolo];

fn mode_color(mode: Mode) -> Color {
    theme::get().mode_color(mode)
}

/// How far the agent runs on its own: paused, one step at a time, or ahead.
fn mode_glyph(mode: Mode) -> &'static str {
    match mode {
        Mode::Plan => "⏸",
        Mode::Manual => "⏵",
        Mode::Auto => "⏵⏵",
        Mode::Edit => "⏵⏵⏵",
        Mode::Yolo => "☠",
    }
}

/// Opens every mid-conversation note about the edit tool, so a later toggle can
/// find and replace the previous one.
const EDIT_NOTE_PREFIX: &str = "Edits are now ";

fn is_edit_note(msg: &ChatMessage) -> bool {
    msg.role == "system" && msg.content.starts_with(EDIT_NOTE_PREFIX)
}

/// Keys the composer answers to. None of them are visible on screen, so
/// `/help` is where they get said.
const KEY_HELP: &[(&str, &str)] = &[
    ("enter", "send · esc interrupts a running turn"),
    ("esc esc", "quit (twice, so a stray press does not)"),
    ("shift+tab", "step to the next mode"),
    ("ctrl+j", "newline without sending"),
    ("@", "mention a file from this repo"),
    ("↑ ↓", "move the cursor, then step through past messages"),
];

pub(super) const CHAT_COMMANDS: &[CommandDesc] = &[
    CommandDesc {
        name: "model",
        takes_arg: true,
        desc: "Switch the active model, or pick one with no argument",
    },
    CommandDesc {
        name: "provider",
        takes_arg: false,
        desc: "Switch the endpoint Aster talks to, then pick a model",
    },
    CommandDesc {
        name: "resume",
        takes_arg: false,
        desc: "Reopen one of this repo's saved sessions",
    },
    CommandDesc {
        name: "mode",
        takes_arg: false,
        desc: "Choose how the agent acts (also shift+tab), or /mode <name>",
    },
    CommandDesc {
        name: "effort",
        takes_arg: true,
        desc: "Set the reasoning budget (off, low, medium, high), or cycle it",
    },
    CommandDesc {
        name: "yolo",
        takes_arg: false,
        desc: "Toggle YOLO mode — guardrails off, red theme",
    },
    CommandDesc {
        name: "compact",
        takes_arg: false,
        desc: "Fold earlier turns into a summary to free context",
    },
    CommandDesc {
        name: "status",
        takes_arg: false,
        desc: "Show session, model, context, and token usage",
    },
    CommandDesc {
        name: "diff",
        takes_arg: false,
        desc: "Show uncommitted changes in the repository",
    },
    CommandDesc {
        name: "mcp",
        takes_arg: false,
        desc: "List connected MCP servers and their tools",
    },
    CommandDesc {
        name: "skills",
        takes_arg: false,
        desc: "List the skills the agent can load",
    },
    CommandDesc {
        name: "memory",
        takes_arg: false,
        desc: "List what Aster remembers about this project",
    },
    CommandDesc {
        name: "clear",
        takes_arg: false,
        desc: "Clear the conversation and start fresh",
    },
    CommandDesc {
        name: "help",
        takes_arg: false,
        desc: "List the available commands",
    },
    CommandDesc {
        name: "quit",
        takes_arg: false,
        desc: "Exit the chat",
    },
];

struct ChatApp {
    /// Finished blocks waiting to be pushed into the terminal's scrollback.
    queue: VecDeque<Vec<Line<'static>>>,
    /// Assistant text is rendered a source line at a time as it streams.
    markdown: MarkdownStream,
    /// True between the first and last chunk of one assistant message, so
    /// continuation lines hang under the bullet instead of starting a new cell.
    speaking: bool,
    /// Everything the model streamed this turn, to tell a quiet endpoint (one
    /// that sends no deltas) from one that already rendered its reply.
    streamed: String,
    /// An `Explored` cell is open: further read-only rows hang off it instead
    /// of opening a new one.
    exploring: bool,
    running: Vec<RunningTool>,
    /// Blank lines streamed mid-message, held back until real content follows
    /// so a message never opens or closes with empty rows.
    pending_blanks: usize,

    thinking: bool,
    started: Option<Instant>,
    usage: Option<aster_ai::UsageSnapshot>,
    /// Terminal width from the last draw; every cell is wrapped to it.
    width: usize,

    mode: Mode,
    effort: Effort,
    /// `true` when the run is read-only; the picker cannot leave `plan`.
    edits_locked: bool,
    model: String,
    history: Vec<ChatMessage>,
    store: Option<Store>,
    recorder: Option<Recorder>,
    repo_root: std::path::PathBuf,
    perms: SessionPermissions,
    approval_tx: UiSender,
    events_tx: mpsc::Sender<TurnEvent>,
    should_quit: bool,
    /// Transient footer status, cleared on the next keystroke.
    flash: Option<String>,
    /// Set by `/clear`; the run loop wipes the screen on the next pass.
    clear_requested: bool,
    /// Owned here rather than per turn, so a plan survives the turn that built it.
    plan: sync::Arc<sync::Mutex<crate::chat::PlanState>>,
    /// Where the answer to the open `ask_user` question goes. A picker event
    /// carries only the chosen text, since the responder cannot be cloned.
    pending_question: Option<tokio::sync::oneshot::Sender<Option<String>>>,
    /// The open approval is a plan; granting it promotes the session.
    pending_plan_approval: bool,
    /// `AGENTS.md` and friends, read once at startup rather than per turn.
    instructions: sync::Arc<crate::instructions::Instructions>,
    /// Connected MCP servers, cloned into each turn's context.
    mcp: Option<crate::mcp::McpRuntime>,
    /// Per-turn caps, cloned into each turn's context.
    limits: crate::chat::Limits,
    /// The endpoint in use, so `/provider` can mark the current row.
    provider_base_url: String,
    /// When the last interrupt key landed, so quitting takes two presses.
    quit_armed: Option<Instant>,
    /// A YOLO switch is playing its full-screen animation; cleared by
    /// `finish_takeover`, which repaints the world in the new palette.
    takeover: Option<Takeover>,
}

struct Takeover {
    start: Instant,
    entering: bool,
}

impl ChatApp {
    fn new(
        mode: Mode,
        effort: Effort,
        edits_locked: bool,
        model: String,
        perms: SessionPermissions,
        approval_tx: UiSender,
        events_tx: mpsc::Sender<TurnEvent>,
    ) -> Self {
        Self {
            queue: VecDeque::new(),
            markdown: MarkdownStream::default(),
            speaking: false,
            streamed: String::new(),
            exploring: false,
            running: Vec::new(),
            pending_blanks: 0,
            thinking: false,
            started: None,
            usage: None,
            width: 80,
            mode,
            effort,
            edits_locked,
            model,
            history: Vec::new(),
            store: None,
            recorder: None,
            repo_root: std::path::PathBuf::new(),
            perms,
            approval_tx,
            events_tx,
            should_quit: false,
            flash: None,
            clear_requested: false,
            plan: sync::Arc::default(),
            pending_question: None,
            pending_plan_approval: false,
            instructions: sync::Arc::default(),
            mcp: None,
            limits: crate::chat::Limits::default(),
            provider_base_url: String::new(),
            quit_armed: None,
            takeover: None,
        }
    }

    fn emit(&mut self, block: Vec<Line<'static>>) {
        if !block.is_empty() {
            self.queue.push_back(block);
        }
    }

    fn note(&mut self, text: &str) {
        let block = history::notice(text, self.width);
        self.emit(block);
    }

    fn on_turn_event(&mut self, ev: TurnEvent) {
        match ev {
            TurnEvent::Token(delta) => {
                self.end_explored();
                self.streamed.push_str(&delta);
                let lines = self.markdown.push(&delta);
                let lines = self.hold_blank_edges(lines);
                if !lines.is_empty() {
                    let block = history::assistant(lines, !self.speaking, self.width);
                    self.speaking = true;
                    self.emit(block);
                }
            }
            TurnEvent::ToolCall { id, name, args } => {
                // Text before a tool call is a finished thought; close it so the
                // steps it produced read as coming after it.
                self.end_message();
                self.running.push(RunningTool {
                    id,
                    label: step_label(&name, &args),
                    path: arg_str(&args, "path"),
                    name,
                });
            }
            TurnEvent::ToolResult { id, result, error } => {
                let Some(i) = self.running.iter().position(|t| t.id == id) else {
                    return;
                };
                let tool = self.running.remove(i);
                self.on_tool_result(tool, &result, error);
            }
            TurnEvent::Notice(message) => {
                self.end_message();
                self.end_explored();
                self.note(&message);
            }
        }
    }

    fn on_tool_result(&mut self, tool: RunningTool, result: &str, failed: bool) {
        if !failed && READ_ONLY.contains(&tool.name.as_str()) {
            let label = match missed(result) {
                true => format!("{} (not found)", tool.label),
                false => tool.label,
            };
            let block = history::explored_row(&label, self.exploring, self.width);
            self.exploring = true;
            self.emit(block);
            return;
        }
        self.end_explored();
        let block = if failed {
            history::tool(&tool.label, result, true, self.width)
        } else if tool.name == "update_plan" {
            // The plan itself is the output; the tool's text just repeats it.
            history::plan(&self.plan_steps(), self.width)
        } else if tool.name == "edit_file" {
            // `edit_file` answers with "edited <path>:\n<patch>".
            let (head, patch) = result.split_once('\n').unwrap_or((result, ""));
            let verb = if head.starts_with("created") {
                "Created"
            } else {
                "Edited"
            };
            history::patch(verb, &tool.path, patch, self.width)
        } else {
            history::tool(&tool.label, result, false, self.width)
        };
        self.emit(block);
    }

    /// The plan as the agent last left it. Read from the shared state rather
    /// than parsed back out of the tool's text.
    fn plan_steps(&self) -> Vec<(crate::chat::PlanStepStatus, String)> {
        let Ok(plan) = self.plan.lock() else {
            return Vec::new();
        };
        plan.steps
            .iter()
            .map(|step| (step.status, step.label.clone()))
            .collect()
    }

    /// Close the assistant message in flight, emitting its trailing partial line.
    fn end_message(&mut self) {
        if !self.markdown.is_empty() {
            let lines = self.markdown.flush();
            let lines = self.hold_blank_edges(lines);
            if !lines.is_empty() {
                let block = history::assistant(lines, !self.speaking, self.width);
                self.emit(block);
            }
        }
        self.pending_blanks = 0;
        self.speaking = false;
    }

    /// Drop blank lines at a message's edges: leading ones vanish, interior
    /// runs are held in `pending_blanks` until real content follows.
    fn hold_blank_edges(&mut self, lines: Vec<Line<'static>>) -> Vec<Line<'static>> {
        let mut out = Vec::new();
        for line in lines {
            let blank = line.spans.iter().all(|s| s.content.trim().is_empty());
            if blank {
                if self.speaking || !out.is_empty() {
                    self.pending_blanks += 1;
                }
            } else {
                out.extend(std::iter::repeat_with(|| Line::from("")).take(self.pending_blanks));
                self.pending_blanks = 0;
                out.push(line);
            }
        }
        out
    }

    /// Close the open `Explored` cell so the next read-only run opens its own.
    fn end_explored(&mut self) {
        self.exploring = false;
    }

    fn submit(
        &mut self,
        text: &str,
        refs: &[(String, String)],
        client: &AiClient,
        repo_root: &std::path::Path,
    ) -> ChatTurn {
        // A dismissed session picker leaves no transcript open; start one now
        // rather than dropping the conversation on the floor.
        if self.recorder.is_none() {
            self.start_new_session();
        }
        let content = render_user_content(text, refs);
        let block = history::user(&content, self.width);
        self.emit(block);
        self.history.push(ChatMessage {
            role: "user".into(),
            content: content.clone(),
        });
        self.record_user(&content);
        self.thinking = true;
        self.started = Some(Instant::now());
        self.streamed.clear();

        let client = client.clone();
        let repo_root = repo_root.to_path_buf();
        let history = self.history.clone();
        let allow_edits = self.mode.can_edit();
        let policy = self.perms.policy(self.mode);
        let grants = self.perms.grants.clone();
        let approver = Some(self.approval_tx.clone());
        let events_tx = self.events_tx.clone();
        let ctx = SessionCtx {
            recorder: self.recorder.clone(),
            store: self.store.clone(),
            skills: crate::chat::discover_skills(&repo_root),
            instructions: self.instructions.clone(),
            probe: std::sync::Arc::new(bash_tools::ToolProbe::detect()),
            plan: self.plan.clone(),
            mcp: self.mcp.clone(),
            limits: self.limits,
            environment: crate::chat::environment_note(&repo_root),
            yolo: self.mode == Mode::Yolo,
        };
        tokio::spawn(async move {
            let sink: crate::chat::ChatEventSink = Box::new(move |event| {
                let Some(ev) = decode_turn_event(&event) else {
                    return;
                };
                // Dropping events when the UI lags beats blocking the turn.
                let _ = events_tx.try_send(ev);
            });
            crate::chat::agent_turn_streaming(
                client,
                repo_root,
                history,
                allow_edits,
                policy,
                grants,
                approver,
                ctx,
                sink,
            )
            .await
        })
    }

    fn finish_turn(
        &mut self,
        reply: &str,
        _edited: &[String],
        compacted: Option<Vec<ChatMessage>>,
    ) {
        self.end_message();
        self.end_explored();
        self.started = None;
        self.thinking = false;

        if let Some(compacted) = compacted {
            self.history = compacted;
            self.note("compacted earlier turns to save context");
        }
        self.history.push(ChatMessage {
            role: "assistant".into(),
            content: reply.into(),
        });

        // A streamed reply is already on screen; only a quiet endpoint (one that
        // sends no deltas) still needs rendering.
        if self.streamed.trim().is_empty() && !reply.trim().is_empty() {
            let block = history::assistant(markdown::render(reply), true, self.width);
            self.emit(block);
        }
        self.streamed.clear();
    }

    /// Drop the unanswered question from history so a retry resends it instead
    /// of stacking a duplicate user turn.
    fn fail_turn(&mut self, msg: &str) {
        self.end_message();
        self.end_explored();
        self.started = None;
        self.thinking = false;
        if self.history.last().is_some_and(|m| m.role == "user") {
            self.history.pop();
        }
        let block = history::error(msg, self.width);
        self.emit(block);
    }

    fn record_user(&self, text: &str) {
        let Some(recorder) = &self.recorder else {
            return;
        };
        if let Ok(mut writer) = recorder.lock()
            && let Err(e) = writer.append_message(MessageEvent::user(text))
        {
            tracing::warn!("failed to record user turn: {e:#}");
        }
    }

    /// Replay a resumed transcript into the view and in-memory history. The
    /// assistant turns are already recorded on disk, so nothing is re-appended.
    fn load_history(&mut self, messages: Vec<ChatMessage>) {
        if messages.is_empty() {
            return;
        }
        let turns = messages.iter().filter(|m| m.role == "user").count();
        for m in &messages {
            let block = match m.role.as_str() {
                "user" => history::user(&m.content, self.width),
                "assistant" => history::assistant(markdown::render(&m.content), true, self.width),
                _ => continue,
            };
            self.emit(block);
        }
        self.history = messages;
        self.note(&format!(
            "resumed {turns} previous turn(s) · /clear to start fresh"
        ));
    }

    fn start_new_session(&mut self) {
        let Some(store) = &self.store else {
            return;
        };
        match store.new_session(&self.repo_root, &self.repo_root, Some(self.model.clone())) {
            Ok(writer) => self.recorder = Some(sync::Arc::new(sync::Mutex::new(writer))),
            Err(e) => tracing::warn!("failed to start a new session: {e:#}"),
        }
    }

    /// The running turn cloned the older policy, so it keeps asking after a
    /// promotion to `edit`; honour the newer mode. Out-of-repo requests
    /// (`scope`) are a separate question `edit` does not answer.
    fn on_approval_request(&mut self, req: ApprovalRequest, pane: &mut BottomPane<AppEvent>) {
        if req.scope.is_none() && self.mode == Mode::Edit {
            let _ = req.respond.send(Answer::Yes);
            return;
        }
        pane.push_approval(req);
    }

    /// `submit` recomputes the edit gate from `self.mode`, so approving a plan
    /// has to move the session, not just the turn.
    fn on_plan_approval_request(&mut self, req: ApprovalRequest, pane: &mut BottomPane<AppEvent>) {
        if self.mode == Mode::Edit {
            let _ = req.respond.send(Answer::Yes);
            return;
        }
        self.pending_plan_approval = true;
        pane.push_approval(req);
    }

    /// Offer the agent's question as a picker. Without options there is nothing
    /// to pick, so the question is printed and the agent told to decide.
    fn on_question_request(&mut self, req: QuestionRequest, pane: &mut BottomPane<AppEvent>) {
        if req.options.is_empty() {
            self.note(&format!("{}: {}", req.header, req.question));
            let _ = req.respond.send(None);
            return;
        }

        // A question that arrives while one is open replaces it; the dropped
        // responder resolves to "declined" on the agent's side.
        self.pending_question = Some(req.respond);
        let items = req
            .options
            .iter()
            .map(|option| SelectionItem {
                name: option.clone(),
                description: String::new(),
                is_current: false,
                event: AppEvent::QuestionAnswered(option.clone()),
            })
            .collect();
        self.note(&req.question);
        pane.push_picker(&req.header, items);
    }

    /// YOLO drops the sandbox, so it is the one mode the user is asked about
    /// rather than switched into. The prompt is the same picker every other
    /// question uses; declining is the resting choice.
    fn confirm_yolo(&mut self, pane: &mut BottomPane<AppEvent>) {
        if self.mode == Mode::Yolo {
            return;
        }
        // Asking first and refusing after would be the worst of both.
        if self.edits_locked {
            self.flash = Some("edits are off for this run; yolo is unavailable".into());
            return;
        }
        self.note(
            "YOLO mode gives Aster unrestricted access: any path, full network, \
             your environment as-is.",
        );
        pane.push_picker(
            "Go unrestricted?",
            vec![
                SelectionItem {
                    name: format!("No, stay in {}", self.mode.as_str()),
                    description: "keep the guardrails".into(),
                    is_current: true,
                    event: AppEvent::SetMode(self.mode),
                },
                SelectionItem {
                    name: "Yes, enable YOLO mode".into(),
                    description: "run unrestricted".into(),
                    is_current: false,
                    event: AppEvent::YoloConfirmed,
                },
            ],
        );
    }

    fn on_app_event(&mut self, ev: AppEvent, client: &mut AiClient) {
        match ev {
            // Entering YOLO goes through `confirm_yolo`, never straight here.
            AppEvent::SetMode(Mode::Yolo) => {}
            AppEvent::SetMode(mode) => self.select_mode(mode),
            AppEvent::YoloConfirmed => self.select_mode(Mode::Yolo),
            AppEvent::SetEffort(effort) => self.set_effort(effort, client),
            AppEvent::ApprovalDecided { answer, scope } => {
                let plan = std::mem::take(&mut self.pending_plan_approval);
                let note = match (answer, &scope) {
                    (Answer::No, _) if plan => Some("plan rejected".to_string()),
                    (Answer::No, _) => Some("edit rejected".to_string()),
                    (Answer::Always, Some(dir)) => {
                        Some(format!("always allowing {}", short_path(dir)))
                    }
                    _ => None,
                };
                // An approved plan, or "always" on an in-repo edit, both mean
                // "stop asking": promote the session so it outlives the turn.
                let promotes =
                    (plan && answer.allowed()) || (answer == Answer::Always && scope.is_none());
                if promotes && !self.edits_locked {
                    self.select_mode(Mode::Edit);
                }
                if let Some(note) = note {
                    self.note(&note);
                }
            }
            AppEvent::QuestionAnswered(answer) => {
                if let Some(respond) = self.pending_question.take() {
                    let _ = respond.send(Some(answer.clone()));
                }
                self.note(&format!("answered: {answer}"));
            }
            AppEvent::SessionPicked(id) => self.resume_session(&id),
            AppEvent::ModelChanged(model) => self.set_model(model, client),
            AppEvent::ProviderPicked { base_url, model } => {
                self.switch_provider(base_url, model, client)
            }
            AppEvent::ModelsLoaded(_) => {}
            AppEvent::MentionQueried(_) | AppEvent::MentionResults { .. } => {}
            AppEvent::ModelsFailed(e) => self.note(&format!("failed to load model list: {e}")),
            AppEvent::Compacted {
                history,
                summary,
                replaces_through,
            } => {
                let ctx = SessionCtx {
                    recorder: self.recorder.clone(),
                    ..SessionCtx::default()
                };
                ctx.record_summary(&summary, replaces_through);
                self.history = history;
                self.flash = None;
                self.note("compacted earlier turns to save context");
            }
            AppEvent::CompactFailed(e) => {
                self.flash = None;
                self.note(&format!("compact failed: {e}"));
            }
        }
    }

    /// One row per saved session, newest first. With nothing saved there is
    /// nothing to choose, so the run just starts clean.
    fn open_session_picker(&mut self, pane: &mut BottomPane<AppEvent>) {
        let Some(store) = &self.store else {
            self.note("no session store available; starting fresh");
            return;
        };
        let metas = match store.list_sessions(&self.repo_root) {
            Ok(metas) => metas,
            Err(e) => {
                self.note(&format!("could not list sessions: {e:#}"));
                return;
            }
        };

        let items: Vec<SelectionItem<AppEvent>> = metas
            .iter()
            .filter_map(|meta| {
                let transcript = store.resume(&self.repo_root, &meta.id).ok()?;
                let turns = transcript.user_turn_count();
                // An empty transcript is a stray from a session nobody typed
                // into; offering it is the trap `--continue` already falls into.
                if turns == 0 {
                    return None;
                }
                let title = transcript
                    .first_user_text()
                    .map(|s| super::helpers::truncate_label(s.trim(), 60))
                    .unwrap_or_else(|| meta.id.clone());
                Some(SelectionItem {
                    name: title,
                    description: format!(
                        "{}  ·  {turns} turn{}",
                        meta.created_at.format("%Y-%m-%d %H:%M"),
                        if turns == 1 { "" } else { "s" }
                    ),
                    is_current: false,
                    event: AppEvent::SessionPicked(meta.id.clone()),
                })
            })
            .collect();

        if items.is_empty() {
            self.note("no saved sessions for this repo yet");
            return;
        }
        pane.push_picker("Resume a session", items);
    }

    /// Adopt a chosen session: its history seeds the view and later turns append
    /// to its transcript rather than to a fresh one.
    fn resume_session(&mut self, id: &str) {
        let Some(store) = &self.store else {
            return;
        };
        let transcript = match store.resume(&self.repo_root, id) {
            Ok(t) => t,
            Err(e) => {
                self.note(&format!("could not resume {id}: {e:#}"));
                return;
            }
        };
        match store.resume_writer(&self.repo_root, id) {
            Ok(writer) => self.recorder = Some(sync::Arc::new(sync::Mutex::new(writer))),
            Err(e) => {
                self.note(&format!("could not reopen {id} for writing: {e:#}"));
                return;
            }
        }
        self.load_history(transcript.to_chat_messages());
    }

    fn open_mode_picker(&self, pane: &mut BottomPane<AppEvent>) {
        let items = MODE_ORDER
            .iter()
            .map(|mode| SelectionItem {
                name: mode.as_str().to_string(),
                description: mode.description().to_string(),
                is_current: *mode == self.mode,
                event: AppEvent::SetMode(*mode),
            })
            .collect();
        pane.push_picker("Mode", items);
    }

    /// Shift+tab: step to the next mode in the footer order, wrapping around.
    fn cycle_mode(&mut self) {
        let at = MODE_ORDER.iter().position(|m| *m == self.mode).unwrap_or(0);
        self.select_mode(MODE_ORDER[(at + 1) % MODE_ORDER.len()]);
    }

    fn open_effort_picker(&self, pane: &mut BottomPane<AppEvent>) {
        let items = Effort::ALL
            .iter()
            .map(|effort| SelectionItem {
                name: effort.as_str().to_string(),
                description: String::new(),
                is_current: *effort == self.effort,
                event: AppEvent::SetEffort(*effort),
            })
            .collect();
        pane.push_picker("Effort", items);
    }

    /// The session header, in the palette current when it is called.
    fn welcome_block(&self) -> Vec<Line<'static>> {
        history::welcome(
            &[
                ("model", self.model.clone()),
                (
                    "provider",
                    crate::init::provider_label(&self.provider_base_url),
                ),
                ("cwd", self.repo_root.display().to_string()),
                ("mode", self.mode.as_str().to_string()),
                ("effort", self.effort.to_string()),
            ],
            self.width,
        )
    }

    /// The takeover has played out: land the screen in the new palette.
    /// Scrollback cannot be repainted, so everything is cleared and the header
    /// rebuilt after the theme settles, never mid-fade in neither colour.
    fn finish_takeover(&mut self) {
        let Some(t) = self.takeover.take() else {
            return;
        };
        theme::settle();
        self.queue.clear();
        self.clear_requested = true;
        let welcome = self.welcome_block();
        self.emit(welcome);
        self.note(match t.entering {
            true => "YOLO mode ON — guardrails off, red theme",
            false => "YOLO mode OFF — guardrails back on",
        });
    }

    /// Apply a picker choice. A locked run stays in `plan` and says why.
    fn select_mode(&mut self, mode: Mode) {
        if self.edits_locked && mode.can_edit() {
            self.flash = Some("edits are off for this run (mode: plan)".into());
            return;
        }
        if mode == self.mode {
            return;
        }
        let recolours = (mode == Mode::Yolo) != (self.mode == Mode::Yolo);
        self.mode = mode;
        theme::set(match mode {
            Mode::Yolo => theme::Theme::YOLO,
            Mode::Plan | Mode::Manual | Mode::Auto | Mode::Edit => theme::Theme::DEFAULT,
        });
        if recolours {
            self.takeover = Some(Takeover {
                start: Instant::now(),
                entering: mode == Mode::Yolo,
            });
        }
        self.note_edit_mode();
        // A footer flash, not a scrollback line, so the transcript stays a
        // record of the conversation rather than of settings.
        self.flash = Some(if self.thinking {
            // The running turn cloned its tool list already.
            format!("mode {} · applies to your next message", mode.as_str())
        } else {
            format!("mode {}", mode.as_str())
        });
    }

    /// An effort change takes effect next turn, since each turn clones the client.
    fn set_effort(&mut self, next: Effort, client: &mut AiClient) {
        client.set_effort(next);
        self.effort = next;
        self.flash = Some(if self.thinking {
            format!("effort {next} · applies to your next message")
        } else {
            format!("effort {next}")
        });
    }

    /// A model change takes effect next turn, since each turn clones the client.
    fn set_model(&mut self, model: String, client: &mut AiClient) {
        if model == self.model {
            return;
        }
        client.model = model.clone();
        // Saved as well as applied, or the choice would silently reset next run.
        if let Err(e) = crate::settings::persist_review(Some(&self.repo_root), &[("model", &model)])
        {
            self.note(&format!("could not save the model choice: {e:#}"));
        }
        self.flash = Some(if self.thinking {
            format!("model {model} · applies to your next message")
        } else {
            format!("model {model}")
        });
        self.model = model;
    }

    /// Ask the provider what it serves. The picker opens when the list lands,
    /// so a slow endpoint stalls nothing but itself.
    fn request_models(&mut self, client: &AiClient, tx: mpsc::UnboundedSender<AppEvent>) {
        self.flash = Some("fetching models…".into());
        let client = client.clone();
        tokio::spawn(async move {
            match client.fetch_models().await {
                Ok(models) => {
                    let _ = tx.send(AppEvent::ModelsLoaded(models));
                }
                Err(e) => {
                    let _ = tx.send(AppEvent::ModelsFailed(format!("{e:#}")));
                }
            }
        });
    }

    fn open_model_picker(&mut self, models: Vec<String>, pane: &mut BottomPane<AppEvent>) {
        self.flash = None;
        if models.is_empty() {
            self.note("the provider returned no models; use /model <id>");
            return;
        }
        let view = ModelPickerView::new(&self.model, models, pane.sender());
        pane.push_view(Box::new(view));
    }

    /// Switch endpoint mid-session. The catalog is the same `providers.json`
    /// the init wizard reads, so the two agree on names and defaults.
    fn open_provider_picker(&mut self, pane: &mut BottomPane<AppEvent>) {
        let current = self.provider_base_url.clone();
        let items: Vec<SelectionItem<AppEvent>> = crate::init::provider_choices()
            .into_iter()
            .map(|(name, base_url, model)| SelectionItem {
                name,
                description: base_url.clone(),
                is_current: base_url.trim_end_matches('/') == current.trim_end_matches('/'),
                event: AppEvent::ProviderPicked {
                    base_url,
                    model: model.clone(),
                },
            })
            .collect();
        if items.is_empty() {
            self.note("no providers in the catalog");
            return;
        }
        pane.push_picker("Provider", items);
    }

    /// Repoint the client, then offer that endpoint's models. A missing key is
    /// said now rather than at the next turn's failure.
    fn switch_provider(&mut self, base_url: String, model: String, client: &mut AiClient) {
        let key = crate::init::provider_key(&base_url);
        match key {
            Some(key) => client.set_endpoint(&base_url, key),
            None => {
                // The shared key travels with the endpoint; providers that need
                // their own are named above.
                client.set_endpoint(&base_url, client.api_key().to_string());
                self.note(&format!(
                    "no provider-specific key found for {}; using ASTER_API_KEY",
                    crate::init::provider_label(&base_url)
                ));
            }
        }
        self.provider_base_url = base_url.clone();
        // The endpoint is saved with the model: a restart pairing the new
        // model with the old provider would be worse than either alone.
        if let Err(e) =
            crate::settings::persist_review(Some(&self.repo_root), &[("base_url", &base_url)])
        {
            self.note(&format!("could not save the provider choice: {e:#}"));
        }
        self.set_model(model, client);
        self.flash = Some(format!(
            "provider {}",
            crate::init::provider_label(&base_url)
        ));
    }

    fn handle_command(
        &mut self,
        cmd: &str,
        client: &mut AiClient,
        pane: &mut BottomPane<AppEvent>,
    ) {
        let mut parts = cmd.splitn(2, char::is_whitespace);
        let name = parts.next().unwrap_or("");
        let arg = parts.next().map(str::trim).filter(|s| !s.is_empty());
        match name {
            "model" | "m" => match arg {
                Some(model) => self.set_model(model.to_string(), client),
                None => {
                    let tx = pane.sender();
                    self.request_models(client, tx);
                }
            },
            "mode" => match arg.map(|a| MODE_ORDER.iter().find(|m| m.as_str() == a)) {
                Some(Some(mode)) => self.select_mode(*mode),
                Some(None) => {
                    self.flash = Some("unknown mode (expected plan, manual, auto, or edit)".into());
                }
                None => self.open_mode_picker(pane),
            },
            "provider" | "p" => self.open_provider_picker(pane),
            "resume" | "r" => self.open_session_picker(pane),
            "effort" => match arg.map(str::parse::<Effort>) {
                Some(Ok(effort)) => self.set_effort(effort, client),
                Some(Err(e)) => self.flash = Some(e),
                None => self.open_effort_picker(pane),
            },
            "yolo" => match self.mode {
                Mode::Yolo => self.select_mode(Mode::Edit),
                _ => self.confirm_yolo(pane),
            },
            "clear" | "c" => {
                self.history.clear();
                self.start_new_session();
                self.queue.clear();
                self.clear_requested = true;
                let welcome = self.welcome_block();
                self.emit(welcome);
            }
            "help" | "h" => {
                let width = self.width;
                let mut lines = vec![Line::from(Span::styled(
                    "Commands",
                    Style::default().add_modifier(Modifier::BOLD),
                ))];
                for c in CHAT_COMMANDS {
                    lines.push(Line::from(vec![
                        Span::styled(format!("/{:<9}", c.name), theme::get().accent_style()),
                        Span::styled(format!("  {}", c.desc), theme::get().dim_style()),
                    ]));
                }
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    "Keys",
                    Style::default().add_modifier(Modifier::BOLD),
                )));
                for (key, what) in KEY_HELP {
                    lines.push(Line::from(vec![
                        Span::styled(format!("{key:<10}"), theme::get().accent_style()),
                        Span::styled(format!("  {what}"), theme::get().dim_style()),
                    ]));
                }
                let block = history::assistant(lines, true, width);
                self.emit(block);
            }
            "compact" => self.start_compact(client, pane.sender()),
            "status" => self.show_status(),
            "diff" | "d" => self.show_diff(),
            "mcp" => self.show_mcp(),
            "skills" => self.show_skills(),
            "memory" => self.show_memory(),
            "quit" | "q" | "exit" => self.should_quit = true,
            other => self.note(&format!("unknown command: /{other} (try /help)")),
        }
    }

    /// The tool list is rebuilt per turn, so tell the model its tools changed.
    /// Without this it keeps trusting whatever it said about edits earlier.
    fn note_edit_mode(&mut self) {
        let content = match self.mode {
            Mode::Plan => format!(
                "{EDIT_NOTE_PREFIX}disabled: `edit_file` is unavailable. \
                 Explore the code and present a plan instead."
            ),
            mode => format!(
                "{EDIT_NOTE_PREFIX}enabled ({}): `edit_file` is available.",
                mode.as_str()
            ),
        };
        // Cycling through modes would otherwise stack a note per keystroke.
        if self.history.last().is_some_and(is_edit_note) {
            self.history.pop();
        }
        self.history.push(ChatMessage {
            role: "system".into(),
            content,
        });
    }

    fn footer_line(&self) -> Line<'static> {
        let dark = theme::get().faint_style();
        let mut spans = vec![
            Span::styled(
                format!("  {} {}", mode_glyph(self.mode), self.mode.as_str()),
                Style::default().fg(mode_color(self.mode)),
            ),
            Span::styled(format!("  ·  {}", self.model), dark),
            Span::styled(format!("  ⌁ {}", self.effort), dark),
        ];
        if let Some(msg) = &self.flash {
            spans.push(Span::styled("  ·  ", dark));
            spans.push(Span::styled(msg.clone(), theme::get().accent_style()));
        }
        Line::from(spans)
    }

    fn start_compact(&mut self, client: &AiClient, tx: mpsc::UnboundedSender<AppEvent>) {
        if self.thinking {
            self.note("wait for the current turn to finish before compacting");
            return;
        }
        self.flash = Some("compacting…".into());
        let client = client.clone();
        let history = self.history.clone();
        tokio::spawn(async move {
            match crate::chat::compact_now(&client, &history).await {
                Ok((history, summary, replaces_through)) => {
                    let _ = tx.send(AppEvent::Compacted {
                        history,
                        summary,
                        replaces_through,
                    });
                }
                Err(e) => {
                    let _ = tx.send(AppEvent::CompactFailed(format!("{e:#}")));
                }
            }
        });
    }

    /// Bold-label rows rendered like the /help block.
    fn emit_rows(&mut self, title: &str, rows: Vec<(String, String)>) {
        let width = self.width;
        let mut lines = vec![Line::from(Span::styled(
            title.to_string(),
            Style::default().add_modifier(Modifier::BOLD),
        ))];
        let pad = rows.iter().map(|(k, _)| k.len()).max().unwrap_or(0);
        for (key, value) in rows {
            lines.push(Line::from(vec![
                Span::styled(format!("{key:<pad$}"), theme::get().accent_style()),
                Span::styled(format!("  {value}"), theme::get().dim_style()),
            ]));
        }
        let block = history::assistant(lines, true, width);
        self.emit(block);
    }

    fn show_status(&mut self) {
        let chars: usize = self.history.iter().map(|m| m.content.len()).sum();
        let session = self
            .recorder
            .as_ref()
            .and_then(|r| r.lock().ok().map(|w| w.id().to_string()))
            .unwrap_or_else(|| "not saved".into());
        let mcp = match &self.mcp {
            Some(rt) => format!(
                "{} ({} tools)",
                rt.server_names().join(", "),
                rt.tool_count()
            ),
            None => "none".into(),
        };
        let usage = self.usage_flash().unwrap_or_else(|| "none yet".into());
        self.emit_rows(
            "Status",
            vec![
                ("model".into(), self.model.clone()),
                (
                    "provider".into(),
                    crate::init::provider_label(&self.provider_base_url),
                ),
                ("mode".into(), self.mode.as_str().into()),
                ("effort".into(), self.effort.to_string()),
                ("session".into(), session),
                (
                    "context".into(),
                    format!(
                        "{} messages · {} of {} chars before auto-compact",
                        self.history.len(),
                        human_count(chars),
                        human_count(self.limits.compact_budget_chars),
                    ),
                ),
                ("mcp".into(), mcp),
                ("usage".into(), usage),
            ],
        );
    }

    fn show_diff(&mut self) {
        let out = std::process::Command::new("git")
            .args(["diff", "HEAD"])
            .current_dir(&self.repo_root)
            .output();
        let body = match out {
            Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout).into_owned(),
            Ok(out) => {
                self.note(&format!(
                    "git diff failed: {}",
                    String::from_utf8_lossy(&out.stderr).trim()
                ));
                return;
            }
            Err(e) => {
                self.note(&format!("could not run git: {e}"));
                return;
            }
        };
        if body.trim().is_empty() {
            self.note("no uncommitted changes");
            return;
        }
        const MAX_DIFF_LINES: usize = 400;
        let width = self.width;
        let total = body.lines().count();
        let shown: String = body
            .lines()
            .take(MAX_DIFF_LINES)
            .collect::<Vec<_>>()
            .join("\n");
        let mut lines = history::diff_lines(&shown, width);
        if total > MAX_DIFF_LINES {
            lines.extend(history::notice(
                &format!("… {} more lines (run `git diff`)", total - MAX_DIFF_LINES),
                width,
            ));
        }
        self.emit(lines);
    }

    fn show_mcp(&mut self) {
        let Some(rt) = &self.mcp else {
            self.note("no MCP servers configured (add them under `mcp:` in aster.yaml)");
            return;
        };
        let rows: Vec<(String, String)> = rt
            .server_names()
            .into_iter()
            .map(|name| (name, String::new()))
            .collect();
        if rows.is_empty() {
            self.note("no MCP servers connected");
            return;
        }
        let title = format!("MCP servers ({} tools)", rt.tool_count());
        self.emit_rows(&title, rows);
    }

    fn show_skills(&mut self) {
        let skills = crate::chat::discover_skills(&self.repo_root);
        if skills.is_empty() {
            self.note("no skills installed (put SKILL.md folders under .aster/skills/)");
            return;
        }
        let rows = skills
            .iter()
            .map(|s| (s.name.clone(), s.description.clone()))
            .collect();
        self.emit_rows("Skills", rows);
    }

    fn show_memory(&mut self) {
        let Some(store) = &self.store else {
            self.note("no store open, so nothing is remembered");
            return;
        };
        let blocks = match store.memory().list() {
            Ok(blocks) => blocks,
            Err(e) => {
                self.note(&format!("could not list memory: {e:#}"));
                return;
            }
        };
        if blocks.is_empty() {
            self.note("nothing remembered yet (the agent saves facts with its remember tool)");
            return;
        }
        let rows = blocks
            .into_iter()
            .map(|b| (b.name, b.description))
            .collect();
        self.emit_rows("Memory", rows);
    }

    /// Build a token-usage flash string for a post-turn status update.
    fn usage_flash(&self) -> Option<String> {
        let usage = self.usage.filter(|u| u.total_tokens > 0)?;
        let approx = if usage.estimated { "~" } else { "" };
        let cost = usage
            .estimated_cost_usd
            .map(|c| format!("  ·  ~${c:.4}"))
            .unwrap_or_default();
        Some(format!(
            "↑{approx}{} ↓{approx}{}{cost}",
            human_count(usage.prompt_tokens as usize),
            human_count(usage.completion_tokens as usize),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chat_app(model: String) -> ChatApp {
        let (tx, _rx) = mpsc::channel(1);
        let (events_tx, _events_rx) = mpsc::channel(1);
        ChatApp::new(
            Mode::Plan,
            Effort::Low,
            false,
            model,
            SessionPermissions {
                plan: sync::Arc::new(Policy::permissive()),
                manual: sync::Arc::new(Policy::permissive()),
                auto: sync::Arc::new(Policy::permissive()),
                edit: sync::Arc::new(Policy::permissive()),
                grants: sync::Arc::new(Grants::default()),
            },
            tx,
            events_tx,
        )
    }

    fn pane() -> (BottomPane<AppEvent>, mpsc::UnboundedReceiver<AppEvent>) {
        let frames = super::super::terminal::FrameRequester::noop();
        let (tx, rx) = mpsc::unbounded_channel();
        (
            BottomPane::new(
                CHAT_COMMANDS,
                "hint",
                frames,
                tx,
                |answer, scope| AppEvent::ApprovalDecided { answer, scope },
                AppEvent::MentionQueried,
            ),
            rx,
        )
    }

    fn rendered(app: &ChatApp) -> String {
        app.queue
            .iter()
            .flatten()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn command_model_switches_client_and_app() {
        let mut client = AiClient::new("http://localhost", "k", "openai/gpt-4o-mini");
        let mut app = chat_app(client.model.clone());
        let (mut p, _rx) = pane();
        app.handle_command("model anthropic/claude-sonnet-5", &mut client, &mut p);
        assert_eq!(client.model, "anthropic/claude-sonnet-5");
        assert_eq!(app.model, "anthropic/claude-sonnet-5");
    }

    #[test]
    fn command_mode_with_name_switches_and_notes_the_model() {
        let mut client = AiClient::new("http://localhost", "k", "m1");
        let mut app = chat_app(client.model.clone());
        let (mut p, _rx) = pane();
        app.handle_command("mode auto", &mut client, &mut p);
        assert_eq!(app.mode, Mode::Auto);
        assert!(app.history.last().is_some_and(is_edit_note));

        app.handle_command("mode edit", &mut client, &mut p);
        assert_eq!(app.mode, Mode::Edit);
        assert_eq!(app.history.iter().filter(|m| is_edit_note(m)).count(), 1);
    }

    #[test]
    fn command_mode_bare_opens_the_picker() {
        let mut client = AiClient::new("http://localhost", "k", "m1");
        let mut app = chat_app(client.model.clone());
        let (mut p, _rx) = pane();
        app.handle_command("mode", &mut client, &mut p);
        assert!(p.has_active_view());
    }

    #[test]
    fn a_locked_run_cannot_leave_plan() {
        let mut client = AiClient::new("http://localhost", "k", "m1");
        let mut app = chat_app(client.model.clone());
        app.edits_locked = true;
        let (mut p, _rx) = pane();
        app.handle_command("mode edit", &mut client, &mut p);
        assert_eq!(app.mode, Mode::Plan);
        assert!(app.flash.unwrap().contains("edits are off"));
    }

    #[test]
    fn mode_change_mid_turn_says_it_waits() {
        let mut client = AiClient::new("http://localhost", "k", "m1");
        let mut app = chat_app(client.model.clone());
        app.thinking = true;
        let (mut p, _rx) = pane();
        app.handle_command("mode auto", &mut client, &mut p);
        assert!(app.flash.unwrap().contains("next message"));
    }

    #[test]
    fn approval_auto_approves_in_edit_mode() {
        let mut app = chat_app("m1".into());
        app.mode = Mode::Edit;
        let (mut p, _rx) = pane();
        let (respond, rx) = tokio::sync::oneshot::channel();
        app.on_approval_request(
            ApprovalRequest {
                preview: "edit a.rs".into(),
                scope: None,
                respond,
            },
            &mut p,
        );
        assert!(!p.has_active_view());
        assert_eq!(rx.blocking_recv(), Ok(Answer::Yes));
    }

    fn plan_request() -> (ApprovalRequest, tokio::sync::oneshot::Receiver<Answer>) {
        let (respond, rx) = tokio::sync::oneshot::channel();
        (
            ApprovalRequest {
                preview: "Approve this plan and start editing?\n\n[ ] ship it".into(),
                scope: None,
                respond,
            },
            rx,
        )
    }

    #[test]
    fn a_plan_approval_asks_rather_than_passing_silently() {
        let mut app = chat_app("m1".into());
        app.mode = Mode::Plan;
        let (mut p, _rx) = pane();
        let (req, _answer) = plan_request();
        app.on_plan_approval_request(req, &mut p);
        assert!(p.has_active_view(), "the user must see the plan");
    }

    /// The turn-local edit gate dies with the turn, so approval has to land on
    /// the session or the next message drops back to plan.
    #[test]
    fn an_approved_plan_promotes_the_session_not_just_the_turn() {
        let mut app = chat_app("m1".into());
        app.mode = Mode::Plan;
        app.edits_locked = false;
        let mut client = AiClient::new("http://localhost", "k", "m1");
        let (mut p, _rx) = pane();

        let (req, _answer) = plan_request();
        app.on_plan_approval_request(req, &mut p);
        app.on_app_event(
            AppEvent::ApprovalDecided {
                answer: Answer::Yes,
                scope: None,
            },
            &mut client,
        );

        assert_eq!(app.mode, Mode::Edit, "the footer and the next turn agree");
        assert!(app.mode.can_edit(), "the next submit() keeps edits on");
    }

    #[test]
    fn a_rejected_plan_leaves_the_session_in_plan() {
        let mut app = chat_app("m1".into());
        app.mode = Mode::Plan;
        let mut client = AiClient::new("http://localhost", "k", "m1");
        let (mut p, _rx) = pane();

        let (req, _answer) = plan_request();
        app.on_plan_approval_request(req, &mut p);
        app.on_app_event(
            AppEvent::ApprovalDecided {
                answer: Answer::No,
                scope: None,
            },
            &mut client,
        );

        assert_eq!(app.mode, Mode::Plan);
    }

    /// A plain edit approval must not promote; only "always" and plans do.
    #[test]
    fn a_one_off_edit_approval_does_not_promote_the_session() {
        let mut app = chat_app("m1".into());
        app.mode = Mode::Manual;
        let mut client = AiClient::new("http://localhost", "k", "m1");
        app.on_app_event(
            AppEvent::ApprovalDecided {
                answer: Answer::Yes,
                scope: None,
            },
            &mut client,
        );
        assert_eq!(app.mode, Mode::Manual);
    }

    #[test]
    fn a_locked_run_cannot_be_promoted_by_approving_a_plan() {
        let mut app = chat_app("m1".into());
        app.mode = Mode::Plan;
        app.edits_locked = true;
        let mut client = AiClient::new("http://localhost", "k", "m1");
        let (mut p, _rx) = pane();

        let (req, _answer) = plan_request();
        app.on_plan_approval_request(req, &mut p);
        app.on_app_event(
            AppEvent::ApprovalDecided {
                answer: Answer::Yes,
                scope: None,
            },
            &mut client,
        );

        assert_eq!(app.mode, Mode::Plan, "a read-only run stays read-only");
    }

    #[test]
    fn approval_always_promotes_the_session_to_edit() {
        let mut app = chat_app("m1".into());
        app.mode = Mode::Manual;
        let mut client = AiClient::new("http://localhost", "k", "m1");
        let (mut p, _rx) = pane();
        app.on_app_event(
            AppEvent::ApprovalDecided {
                answer: Answer::Always,
                scope: None,
            },
            &mut client,
        );
        assert_eq!(app.mode, Mode::Edit);

        // The next request needs no prompt at all.
        let (respond, rx) = tokio::sync::oneshot::channel();
        app.on_approval_request(
            ApprovalRequest {
                preview: "edit b.rs".into(),
                scope: None,
                respond,
            },
            &mut p,
        );
        assert!(!p.has_active_view());
        assert_eq!(rx.blocking_recv(), Ok(Answer::Yes));
    }

    #[test]
    fn approval_always_stays_locked_when_permissions_deny() {
        let mut app = chat_app("m1".into());
        app.edits_locked = true;
        let mut client = AiClient::new("http://localhost", "k", "m1");
        app.on_app_event(
            AppEvent::ApprovalDecided {
                answer: Answer::Always,
                scope: None,
            },
            &mut client,
        );
        assert_eq!(app.mode, Mode::Plan);
    }

    #[test]
    fn streamed_text_is_emitted_a_line_at_a_time() {
        let mut app = chat_app("m1".into());
        app.on_turn_event(TurnEvent::Token("hello ".into()));
        assert!(app.queue.is_empty(), "an unfinished line stays buffered");
        app.on_turn_event(TurnEvent::Token("there\n".into()));
        assert!(rendered(&app).contains("hello there"));
    }

    #[test]
    fn consecutive_reads_stream_into_one_explored_cell() {
        let mut app = chat_app("m1".into());
        for (i, path) in ["a.rs", "b.rs"].iter().enumerate() {
            let args = format!("{{\"path\":\"{path}\"}}");
            app.on_turn_event(TurnEvent::ToolCall {
                id: i.to_string(),
                name: "read_file".into(),
                args: args.clone(),
            });
            app.on_turn_event(TurnEvent::ToolResult {
                id: i.to_string(),
                result: "contents".into(),
                error: false,
            });
        }
        assert!(
            !app.queue.is_empty(),
            "each read prints as it lands, not once the group closes"
        );

        app.on_turn_event(TurnEvent::Token("done\n".into()));
        let out = rendered(&app);
        assert!(out.contains("Explored"), "{out}");
        assert_eq!(out.matches("Read ").count(), 2);
        assert_eq!(out.matches("Explored").count(), 1);
    }

    #[test]
    fn an_edit_renders_as_a_counted_patch() {
        let mut app = chat_app("m1".into());
        app.on_turn_event(TurnEvent::ToolCall {
            id: "1".into(),
            name: "edit_file".into(),
            args: "{\"path\":\"src/lib.rs\"}".into(),
        });
        app.on_turn_event(TurnEvent::ToolResult {
            id: "1".into(),
            result: "edited src/lib.rs:\n- old\n+ new\n".into(),
            error: false,
        });
        let out = rendered(&app);
        assert!(out.contains("Edited"), "{out}");
        assert!(out.contains("src/lib.rs"));
        assert!(out.contains("+1 −1"), "{out}");
    }

    #[test]
    fn a_failing_tool_shows_its_output_instead_of_being_collapsed() {
        let mut app = chat_app("m1".into());
        app.on_turn_event(TurnEvent::ToolCall {
            id: "1".into(),
            name: "read_file".into(),
            args: "{\"path\":\"missing.rs\"}".into(),
        });
        app.on_turn_event(TurnEvent::ToolResult {
            id: "1".into(),
            result: "no such file".into(),
            error: true,
        });
        assert!(rendered(&app).contains("no such file"));
    }

    #[test]
    fn a_missing_path_is_marked_on_the_explored_line_not_raised_as_an_error() {
        let mut app = chat_app("m1".into());
        app.on_turn_event(TurnEvent::ToolCall {
            id: "1".into(),
            name: "read_file".into(),
            args: "{\"path\":\"crates/ui/src/chat.rs\"}".into(),
        });
        app.on_turn_event(TurnEvent::ToolResult {
            id: "1".into(),
            result: "note: crates/ui/src/chat.rs does not exist. Nearest paths:\n  a.rs".into(),
            error: false,
        });
        app.on_turn_event(TurnEvent::Token("done\n".into()));

        let out = rendered(&app);
        assert!(out.contains("Explored"), "{out}");
        assert!(out.contains("(not found)"), "{out}");
        assert!(!out.contains("Nearest paths"), "{out}");
    }

    #[test]
    fn a_command_step_names_the_command_it_ran() {
        assert_eq!(
            step_label(
                "run_command",
                r#"{"command":"cargo","args":["test","--all"]}"#
            ),
            "Ran cargo test --all"
        );
        assert_eq!(
            step_label("run_command", r#"{"command":"cargo"}"#),
            "Ran cargo"
        );
    }

    #[test]
    fn a_half_streamed_command_label_does_not_invent_a_name() {
        assert_eq!(
            step_label("run_command", r#"{"args":["test"]}"#),
            "Ran a command"
        );
        assert_eq!(
            step_label("run_command", r#"{"command":"car"#),
            "Ran a command"
        );
    }

    #[test]
    fn yolo_asks_before_it_switches() {
        let mut app = chat_app("m1".into());
        app.mode = Mode::Edit;
        let mut client = AiClient::new("http://localhost", "k", "m1");
        let (mut p, _rx) = pane();

        app.handle_command("yolo", &mut client, &mut p);
        assert_eq!(app.mode, Mode::Edit, "asking does not switch");
        assert!(
            p.has_active_view(),
            "the prompt is a pane view, not a flash"
        );
        assert!(app.flash.is_none(), "{:?}", app.flash);

        app.on_app_event(AppEvent::YoloConfirmed, &mut client);
        assert_eq!(app.mode, Mode::Yolo);
        assert!(app.takeover.is_some(), "the switch plays the takeover");

        app.finish_takeover();
        assert!(app.takeover.is_none());
        assert!(
            app.clear_requested,
            "the screen is repainted in the new palette"
        );
        let out = rendered(&app);
        assert!(out.contains("aster"), "the header comes back: {out}");
        assert!(out.contains("YOLO mode ON"), "{out}");

        theme::set(theme::Theme::DEFAULT);
    }

    #[test]
    fn declining_yolo_leaves_the_mode_alone() {
        let mut app = chat_app("m1".into());
        app.mode = Mode::Edit;
        let mut client = AiClient::new("http://localhost", "k", "m1");
        let (mut p, _rx) = pane();

        app.handle_command("yolo", &mut client, &mut p);
        app.on_app_event(AppEvent::SetMode(Mode::Edit), &mut client);
        assert_eq!(app.mode, Mode::Edit);
    }

    #[test]
    fn a_command_shows_its_command_and_then_its_output() {
        let mut app = chat_app("m1".into());
        app.on_turn_event(TurnEvent::ToolCall {
            id: "1".into(),
            name: "run_command".into(),
            args: r#"{"command":"cargo","args":["test"]}"#.into(),
        });
        app.on_turn_event(TurnEvent::ToolResult {
            id: "1".into(),
            result: "test result: ok. 29 passed".into(),
            error: false,
        });

        let out = rendered(&app);
        assert!(out.contains("cargo test"), "the command is named: {out}");
        assert!(out.contains("29 passed"), "the output follows it: {out}");
        assert!(
            !out.contains("Explored"),
            "a command is not collapsed away as exploration: {out}"
        );
    }

    #[test]
    fn a_quiet_endpoint_still_renders_its_reply() {
        let mut app = chat_app("m1".into());
        app.finish_turn("the whole answer", &[], None);
        assert!(rendered(&app).contains("the whole answer"));
    }

    #[test]
    fn blank_only_tokens_render_nothing() {
        let mut app = chat_app("m1".into());
        app.on_turn_event(TurnEvent::Token("\n\n\n".into()));
        assert!(app.queue.is_empty(), "{:?}", rendered(&app));
        app.end_message();
        assert!(app.queue.is_empty(), "{:?}", rendered(&app));
    }

    #[test]
    fn blank_lines_inside_a_message_survive() {
        let mut app = chat_app("m1".into());
        app.on_turn_event(TurnEvent::Token("first\n\nsecond\n".into()));
        let out = rendered(&app);
        assert!(out.contains("first"));
        assert!(out.contains("second"));
        assert_eq!(app.pending_blanks, 0);
    }

    #[test]
    fn trailing_blank_lines_are_dropped_at_message_end() {
        let mut app = chat_app("m1".into());
        app.on_turn_event(TurnEvent::Token("done\n\n\n".into()));
        app.end_message();
        let rows: Vec<Line<'static>> = app.queue.iter().flatten().cloned().collect();
        let last = rows
            .last()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .unwrap_or_default();
        assert!(last.contains("done"), "{last:?}");
    }

    #[test]
    fn a_streamed_reply_is_not_rendered_twice() {
        let mut app = chat_app("m1".into());
        app.on_turn_event(TurnEvent::Token("the whole answer\n".into()));
        app.finish_turn("the whole answer", &[], None);
        assert_eq!(rendered(&app).matches("the whole answer").count(), 1);
    }

    #[test]
    fn a_failed_turn_drops_the_unanswered_question() {
        let mut app = chat_app("m1".into());
        app.history.push(ChatMessage {
            role: "user".into(),
            content: "hi".into(),
        });
        app.fail_turn("provider is down");
        assert!(app.history.is_empty());
        assert!(rendered(&app).contains("provider is down"));
    }

    #[test]
    fn command_unknown_is_reported() {
        let mut client = AiClient::new("http://localhost", "k", "m1");
        let mut app = chat_app(client.model.clone());
        let (mut p, _rx) = pane();
        app.handle_command("bogus", &mut client, &mut p);
        assert!(rendered(&app).contains("unknown command"));
    }

    #[test]
    fn resume_seeds_history_from_prior_session() {
        let home = tempfile::tempdir().unwrap();
        let store = Store::open(home.path()).unwrap();
        let repo = std::path::Path::new("/tmp/aster-resume-repo");
        {
            let mut w = store.new_session(repo, repo, Some("m".into())).unwrap();
            w.append_message(MessageEvent::user("hello")).unwrap();
            w.append_message(MessageEvent::assistant(Some("hi there".into()), vec![]))
                .unwrap();
        }

        let (recorder, messages) = resume_or_new(&store, repo, &Resume::Latest)
            .unwrap()
            .unwrap();
        assert_eq!(messages.len(), 2);

        let mut app = chat_app("m".into());
        app.store = Some(store);
        app.repo_root = repo.to_path_buf();
        app.recorder = Some(recorder);
        app.load_history(messages);
        assert_eq!(app.history.len(), 2);
        assert!(rendered(&app).contains("hi there"));
    }

    /// A picker cannot be shown before the UI exists, and opening a transcript
    /// early is what leaves the stray empty sessions `--continue` trips over.
    #[test]
    fn pick_opens_no_session_up_front() {
        let home = tempfile::tempdir().unwrap();
        let store = Store::open(home.path()).unwrap();
        let repo = std::path::Path::new("/tmp/aster-pick-repo");

        assert!(
            resume_or_new(&store, repo, &Resume::Pick)
                .unwrap()
                .is_none()
        );
        assert!(store.list_sessions(repo).unwrap().is_empty());
    }

    #[test]
    fn resume_by_id_reopens_that_session() {
        let home = tempfile::tempdir().unwrap();
        let store = Store::open(home.path()).unwrap();
        let repo = std::path::Path::new("/tmp/aster-by-id-repo");
        let id = {
            let mut w = store.new_session(repo, repo, Some("m".into())).unwrap();
            w.append_message(MessageEvent::user("the first one"))
                .unwrap();
            w.meta().id.clone()
        };
        // A newer session, so "latest" and "this id" disagree.
        {
            let mut w = store.new_session(repo, repo, Some("m".into())).unwrap();
            w.append_message(MessageEvent::user("the second one"))
                .unwrap();
        }

        let (_, messages) = resume_or_new(&store, repo, &Resume::Id(id))
            .unwrap()
            .unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].content, "the first one");
    }

    #[test]
    fn resume_by_unknown_id_is_an_error_not_a_new_session() {
        let home = tempfile::tempdir().unwrap();
        let store = Store::open(home.path()).unwrap();
        let repo = std::path::Path::new("/tmp/aster-bad-id-repo");

        let Err(err) = resume_or_new(&store, repo, &Resume::Id("nope".into())) else {
            panic!("an unknown id cannot be resumed");
        };
        assert!(err.to_string().contains("nope"), "{err}");
        assert!(store.list_sessions(repo).unwrap().is_empty());
    }

    #[test]
    fn the_session_picker_skips_empty_transcripts() {
        let home = tempfile::tempdir().unwrap();
        let store = Store::open(home.path()).unwrap();
        let repo = std::path::Path::new("/tmp/aster-picker-repo");
        store.new_session(repo, repo, Some("m".into())).unwrap();
        {
            let mut w = store.new_session(repo, repo, Some("m".into())).unwrap();
            w.append_message(MessageEvent::user("real work")).unwrap();
        }

        let mut app = chat_app("m".into());
        app.store = Some(store);
        app.repo_root = repo.to_path_buf();
        let (mut p, _rx) = pane();
        app.open_session_picker(&mut p);

        assert!(p.has_active_view(), "the one real session is offered");
        assert!(
            rendered(&app).contains("real work") || !rendered(&app).contains("no saved sessions"),
            "{}",
            rendered(&app)
        );
    }

    #[test]
    fn the_session_picker_says_so_when_there_is_nothing_to_resume() {
        let home = tempfile::tempdir().unwrap();
        let store = Store::open(home.path()).unwrap();
        let repo = std::path::Path::new("/tmp/aster-empty-picker-repo");
        store.new_session(repo, repo, Some("m".into())).unwrap();

        let mut app = chat_app("m".into());
        app.store = Some(store);
        app.repo_root = repo.to_path_buf();
        let (mut p, _rx) = pane();
        app.open_session_picker(&mut p);

        assert!(!p.has_active_view(), "an empty list is not a picker");
        assert!(
            rendered(&app).contains("no saved sessions"),
            "{}",
            rendered(&app)
        );
    }

    #[test]
    fn picking_a_session_seeds_its_history_and_reopens_its_transcript() {
        let home = tempfile::tempdir().unwrap();
        let store = Store::open(home.path()).unwrap();
        let repo = std::path::Path::new("/tmp/aster-adopt-repo");
        let id = {
            let mut w = store.new_session(repo, repo, Some("m".into())).unwrap();
            w.append_message(MessageEvent::user("earlier question"))
                .unwrap();
            w.append_message(MessageEvent::assistant(
                Some("earlier answer".into()),
                vec![],
            ))
            .unwrap();
            w.meta().id.clone()
        };

        let mut app = chat_app("m".into());
        app.store = Some(store);
        app.repo_root = repo.to_path_buf();
        let mut client = AiClient::new("http://localhost", "k", "m");
        app.on_app_event(AppEvent::SessionPicked(id), &mut client);

        assert_eq!(app.history.len(), 2);
        assert!(app.recorder.is_some(), "later turns append to that session");
        assert!(
            rendered(&app).contains("earlier answer"),
            "{}",
            rendered(&app)
        );
    }

    #[test]
    fn record_user_persists_turn() {
        let home = tempfile::tempdir().unwrap();
        let store = Store::open(home.path()).unwrap();
        let repo = std::path::Path::new("/tmp/aster-record-repo");

        let mut app = chat_app("m".into());
        app.store = Some(store.clone());
        app.repo_root = repo.to_path_buf();
        app.start_new_session();
        app.record_user("remember me");

        let latest = store.latest(repo).unwrap().unwrap();
        let persisted = latest.events.iter().any(|e| {
            matches!(e, aster_persist::TranscriptEvent::Message(m)
                if m.role == "user" && m.content.as_deref() == Some("remember me"))
        });
        assert!(persisted);
    }
}
