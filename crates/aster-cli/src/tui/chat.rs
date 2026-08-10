//! The chat TUI behind bare `aster`. Finished output goes
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
use super::helpers::{clip_row, human_count, listed, short_path};
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
    /// A server toggled from the `/mcp` panel: name plus its new state.
    McpToggle {
        name: String,
        disabled: bool,
    },
    ModelChanged(String),
    /// A newer release found on GitHub, checked off the loop at startup.
    UpdateAvailable(crate::update::UpdateInfo),
    /// A skill chosen from `/skills`; its action menu opens next.
    SkillPicked(String),
    /// Start a message in the composer that applies the skill.
    SkillUse(String),
    /// Show the skill's full description and where it lives.
    SkillView(String),
    /// Ask before deleting; the confirmed event does the removal.
    SkillDelete(String),
    SkillDeleteConfirmed(String),
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
    /// The MCP connect finished off the loop; the session adopts its tools and
    /// replays whatever was submitted while it was still pending.
    McpReady {
        runtime: Option<crate::mcp::McpRuntime>,
        problems: Vec<String>,
    },
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
    mcp: tokio::task::JoinHandle<(Option<crate::mcp::McpRuntime>, Vec<String>)>,
    limits: crate::chat::Limits,
    swarm: crate::chat::SwarmLimits,
    agents: std::sync::Arc<aster_agents::AgentRegistry>,
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
            credentials: sync::Arc::new(crate::chat::configured_credentials(&perms, &repo_root)),
        },
        approval_tx,
        events_tx,
    );
    app.repo_root = repo_root.clone();
    app.width = tui.width() as usize;
    app.instructions = sync::Arc::new(crate::instructions::discover(&repo_root));
    app.mcp_pending = true;
    app.limits = limits;
    app.swarm = swarm;
    app.agents = agents;
    app.provider_base_url = client.base_url().to_string();

    let mut pane: BottomPane<AppEvent> = BottomPane::new(
        CHAT_COMMANDS,
        "Message Aster…  (/ for commands)",
        tui.frame_requester(),
        app_tx.clone(),
        |answer, scope| AppEvent::ApprovalDecided { answer, scope },
        AppEvent::MentionQueried,
    );
    pane.set_skills(
        crate::chat::discover_skills(&repo_root)
            .iter()
            .map(|s| (s.name.clone(), s.description.clone()))
            .collect(),
    );

    // The store opens before the welcome prints, so a resumed session's id
    // lands in the header; its history replays underneath.
    let mut seeded: Option<Vec<ChatMessage>> = None;
    if let Ok(store) = crate::persist::store() {
        match resume_or_new(&store, &repo_root, &resume) {
            Ok(Some((recorder, messages))) => {
                app.recorder = Some(recorder);
                seeded = Some(messages);
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

    let welcome = app.welcome_block();
    app.emit(welcome);
    if let Some(messages) = seeded {
        app.load_history(messages);
    }

    if matches!(resume, Resume::Pick) {
        app.open_session_picker(&mut pane);
    }

    {
        let tx = app_tx.clone();
        tokio::spawn(async move {
            let (runtime, problems) = mcp.await.unwrap_or((None, Vec::new()));
            let _ = tx.send(AppEvent::McpReady { runtime, problems });
        });
    }
    {
        let tx = app_tx.clone();
        tokio::spawn(async move {
            if let Some(info) = crate::update::check().await {
                let _ = tx.send(AppEvent::UpdateAvailable(info));
            }
        });
    }

    let mut turn: Option<ChatTurn> = None;
    if let Some(seed) = seed.filter(|s| !s.trim().is_empty()) {
        turn = app.submit_or_hold(&seed, &[], &client, &repo_root);
        pane.set_task_running(turn.is_some());
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
                    if let Flow::Quit = on_key(
                        &mut app,
                        &mut pane,
                        key,
                        &mut client,
                        &mut turn,
                        &mut events_rx,
                        &repo_root,
                    ) {
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
                    AppEvent::SkillPicked(name) => app.open_skill_actions(&name, &mut pane),
                    AppEvent::SkillUse(name) => {
                        pane.composer.insert_str(&format!("Use the \"{name}\" skill: "))
                    }
                    AppEvent::SkillDelete(name) => app.confirm_skill_delete(&name, &mut pane),
                    AppEvent::SkillDeleteConfirmed(name) => {
                        app.delete_skill(&name);
                        let skills = crate::chat::discover_skills(&repo_root);
                        pane.set_skills(
                            skills
                                .iter()
                                .map(|s| (s.name.clone(), s.description.clone()))
                                .collect(),
                        );
                    }
                    AppEvent::McpReady { runtime, problems } => {
                        app.mcp = runtime;
                        app.mcp_pending = false;
                        // Do not print "MCP connected" anymore.
                        app.error_box(&problems);
                        if let Some((text, refs)) = app.held_submit.take() {
                            app.flash = None;
                            turn = Some(app.submit(&text, &refs, &client, &repo_root));
                            pane.set_task_running(true);
                        }
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
    // Only a session someone actually talked in is worth resuming.
    if let Some(id) = app.session_id()
        && app.history.iter().any(|m| m.role == "user")
    {
        tui.insert_history(history::notice(
            &format!("Resume this session with: aster --resume {id}"),
            app.width,
        ))?;
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
    events_rx: &mut mpsc::Receiver<TurnEvent>,
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
            app.flash = None;
            *turn = app.submit_or_hold(&text, &refs, client, repo_root);
            pane.set_task_running(turn.is_some());
        }
        InputResult::Command(cmd) => {
            app.handle_command(&cmd, client, pane);
        }
        InputResult::Busy { text, refs } => {
            abort(app, turn, pane);
            // The aborted turn's user message was never answered; drop it so
            // the new message does not stack a duplicate user turn.
            if app.history.last().is_some_and(|m| m.role == "user") {
                app.history.pop();
            }
            // Drop stale events the old turn already pushed before it was
            // cancelled, so they do not render as part of the new turn.
            while events_rx.try_recv().is_ok() {}
            app.flash = None;
            *turn = app.submit_or_hold(&text, &refs, client, repo_root);
            pane.set_task_running(turn.is_some());
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

/// The swarm's clean text: one status line per agent, then the curated report
/// from the synthesizer (or the last collector to finish).
fn agent_report_text(rows: &[AgentRow]) -> String {
    let mut out = String::new();
    for row in rows {
        match row.status {
            AgentRowStatus::Running => out.push_str(&format!("◼ {} running\n", row.agent)),
            AgentRowStatus::Done => out.push_str(&format!("✔ {} done\n", row.agent)),
            AgentRowStatus::Failed => out.push_str(&format!(
                "✖ {}: {}\n",
                row.agent,
                row.error.as_deref().unwrap_or("failed")
            )),
        }
    }
    if let Some(report) = rows
        .iter()
        .rev()
        .find(|r| r.status == AgentRowStatus::Done)
        .and_then(|r| r.report.as_deref())
    {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(report);
    }
    out
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
        "agent_status" => Some(TurnEvent::AgentStatus {
            call_id: event.get("call_id")?.as_str()?.to_string(),
            agent: event.get("agent")?.as_str()?.to_string(),
            status: event.get("status")?.as_str()?.to_string(),
            report: event
                .get("report")
                .and_then(Value::as_str)
                .map(String::from),
            error: event.get("error").and_then(Value::as_str).map(String::from),
            done: event.get("done").and_then(Value::as_u64).unwrap_or(0) as usize,
            total: event.get("total").and_then(Value::as_u64).unwrap_or(0) as usize,
        }),
        "citations" => {
            let sources = event.get("sources")?.as_array()?;
            let citations = sources
                .iter()
                .filter_map(|s| {
                    Some(Citation {
                        url: s.get("url")?.as_str()?.to_string(),
                        title: s.get("title").and_then(Value::as_str).map(String::from),
                    })
                })
                .collect();
            Some(TurnEvent::Citations(citations))
        }
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
        "run_command" => match s("description") {
            "" => match command_line(&parsed) {
                None => "Ran a command".to_string(),
                Some(line) => format!("Ran {line}"),
            },
            summary => summary.to_string(),
        },
        "edit_file" => match s("path") {
            "" => "Edited file".to_string(),
            path => format!("Edited {path}"),
        },
        "remember" => "Saved to memory".to_string(),
        "recall" => format!("Recalled {}", s("name")),
        "read_skill" => format!("Read skill {}", s("name")),
        "agent" => {
            let names: Vec<&str> = parsed["tasks"]
                .as_array()
                .map(|a| a.iter().filter_map(|t| t["agent"].as_str()).collect())
                .unwrap_or_default();
            let total = names.len();
            let unique: Vec<&str> = {
                let mut seen = std::collections::BTreeSet::new();
                names.into_iter().filter(|n| seen.insert(*n)).collect()
            };
            if total == 0 {
                "agent".to_string()
            } else if total == 1 {
                format!("agent: {}", unique[0])
            } else if unique.len() == 1 {
                format!("agent ×{total}: {unique}", unique = unique[0])
            } else {
                format!("agent ×{total}: {}", unique.join(", "))
            }
        }
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
    /// Web-search source citations from the OpenRouter `web` plugin.
    Citations(Vec<Citation>),
    /// Something the harness did that the user has to know about, e.g. the
    /// turn being cut short at the tool-round cap.
    Notice(String),
    /// Live progress for one sub-agent in an `agent` tool call.
    AgentStatus {
        call_id: String,
        agent: String,
        status: String,
        report: Option<String>,
        error: Option<String>,
        done: usize,
        total: usize,
    },
}

/// One source URL returned by the web-search plugin.
pub(super) struct Citation {
    pub(super) url: String,
    pub(super) title: Option<String>,
}

/// A tool call the agent has made but not yet finished.
struct RunningTool {
    id: String,
    name: String,
    label: String,
    path: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AgentRowStatus {
    Running,
    Done,
    Failed,
}

/// One sub-agent's state inside an `agent` tool call, accumulated from
/// `agent_status` events so the finished call renders clean reports.
struct AgentRow {
    agent: String,
    status: AgentRowStatus,
    report: Option<String>,
    error: Option<String>,
}

/// One compiled policy per gating mode, since the picker switches between
/// them per turn, plus the shared out-of-repo grants.
struct SessionPermissions {
    plan: sync::Arc<Policy>,
    manual: sync::Arc<Policy>,
    auto: sync::Arc<Policy>,
    edit: sync::Arc<Policy>,
    grants: sync::Arc<Grants>,
    credentials: sync::Arc<aster_policy::CommandGrants>,
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
    ("enter", "send · interrupts a running turn if needed"),
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
        desc: "Enable or disable MCP servers",
    },
    CommandDesc {
        name: "skills",
        takes_arg: false,
        desc: "Pick a skill to use, view, or delete",
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
    /// Live per-`agent`-tool-call rows, keyed by call id, fed by `agent_status`
    /// events so a finished swarm renders cleanly instead of as a JSON dump.
    agent_rows: std::collections::HashMap<String, Vec<AgentRow>>,
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
    /// The connect is still running, so `mcp` being `None` means "not yet"
    /// rather than "none configured" and a turn must wait for it.
    mcp_pending: bool,
    /// A submit that beat the connect, replayed once the servers answer.
    held_submit: Option<(String, Vec<(String, String)>)>,
    /// Per-turn caps, cloned into each turn's context.
    limits: crate::chat::Limits,
    /// The endpoint in use, so `/provider` can mark the current row.
    provider_base_url: String,
    /// When the last interrupt key landed, so quitting takes two presses.
    quit_armed: Option<Instant>,
    /// A YOLO switch is playing its full-screen animation; cleared by
    /// `finish_takeover`, which repaints the world in the new palette.
    takeover: Option<Takeover>,
    /// Agents discovered at startup, cloned into each turn's context.
    agents: sync::Arc<aster_agents::AgentRegistry>,
    /// Fan-out caps, cloned into each turn's context.
    swarm: crate::chat::SwarmLimits,
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
            agent_rows: std::collections::HashMap::new(),
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
            mcp_pending: false,
            held_submit: None,
            limits: crate::chat::Limits::default(),
            provider_base_url: String::new(),
            quit_armed: None,
            takeover: None,
            agents: sync::Arc::default(),
            swarm: crate::chat::SwarmLimits::default(),
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

    fn error_box(&mut self, texts: &[String]) {
        let block = history::error_box(texts, self.width);
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
            TurnEvent::AgentStatus {
                call_id,
                agent,
                status,
                report,
                error,
                done,
                total,
            } => {
                let rows = self.agent_rows.entry(call_id).or_default();
                match status.as_str() {
                    "running" => {
                        if !rows.iter().any(|r| r.agent == agent) {
                            rows.push(AgentRow {
                                agent: agent.clone(),
                                status: AgentRowStatus::Running,
                                report: None,
                                error: None,
                            });
                        }
                        self.note(&format!("agent {agent}: started"));
                    }
                    "done" => {
                        if let Some(row) = rows.iter_mut().find(|r| r.agent == agent) {
                            row.status = AgentRowStatus::Done;
                            row.report = report;
                        }
                        let n = if done > 0 {
                            format!(" ({done}/{total})")
                        } else {
                            String::new()
                        };
                        self.note(&format!("agent {agent}: done{n}"));
                    }
                    "error" => {
                        if let Some(row) = rows.iter_mut().find(|r| r.agent == agent) {
                            row.status = AgentRowStatus::Failed;
                            row.error = error.clone();
                        }
                        let why = error
                            .as_deref()
                            .map(|e| e.lines().next().unwrap_or("failed"))
                            .unwrap_or("failed");
                        self.note(&format!("agent {agent}: failed: {why}"));
                    }
                    _ => {}
                }
            }
            TurnEvent::Citations(sources) => {
                self.end_message();
                self.end_explored();
                let block = history::citations(&sources, self.width);
                self.emit(block);
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
        let block = if tool.name == "agent" {
            // Render the swarm cleanly from the accumulated agent_status rows
            // instead of the JSON array the model sees.
            let rows = self.agent_rows.remove(&tool.id).unwrap_or_default();
            history::tool(&tool.label, &agent_report_text(&rows), failed, self.width)
        } else if failed {
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

    /// Turns carry the MCP tool list, so a submit that beats the connect is
    /// held and replayed rather than run without the servers' tools.
    fn submit_or_hold(
        &mut self,
        text: &str,
        refs: &[(String, String)],
        client: &AiClient,
        repo_root: &std::path::Path,
    ) -> Option<ChatTurn> {
        if self.mcp_pending {
            self.held_submit = Some((text.to_string(), refs.to_vec()));
            self.flash = Some("connecting to MCP servers…".into());
            return None;
        }
        Some(self.submit(text, refs, client, repo_root))
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
            credentials: self.perms.credentials.clone(),
            skills: crate::chat::discover_skills(&repo_root),
            instructions: self.instructions.clone(),
            probe: std::sync::Arc::new(bash_tools::ToolProbe::detect()),
            plan: self.plan.clone(),
            mcp: self.mcp.clone(),
            limits: self.limits,
            environment: crate::chat::environment_note(&repo_root),
            yolo: self.mode == Mode::Yolo,
            reads: Default::default(),
            lookups: Default::default(),
            injected: Default::default(),
            agents: self.agents.clone(),
            sub_agent: None,
            swarm: self.swarm.clone(),
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
            // Handled on the run loop, which owns the turn a hold replays into.
            AppEvent::McpReady { .. } => {}
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
            AppEvent::McpToggle { name, disabled } => self.toggle_mcp(&name, disabled),
            AppEvent::ModelChanged(model) => self.set_model(model, client),
            AppEvent::ProviderPicked { base_url, model } => {
                self.switch_provider(base_url, model, client)
            }
            AppEvent::UpdateAvailable(info) => {
                let block = history::update(&info, self.width);
                self.emit(block);
            }
            // Skill events that need the pane are handled on the run loop.
            AppEvent::SkillPicked(_)
            | AppEvent::SkillUse(_)
            | AppEvent::SkillDelete(_)
            | AppEvent::SkillDeleteConfirmed(_) => {}
            AppEvent::SkillView(name) => self.show_skill(&name),
            AppEvent::ModelsLoaded(_) => {}
            AppEvent::MentionQueried(_) | AppEvent::MentionResults { .. } => {}
            AppEvent::ModelsFailed(e) => self.note(&format!("failed to load model list: {e}")),
            AppEvent::Compacted {
                history,
                summary,
                replaces_through,
            } => {
                let agents = self.agents.clone();
                let swarm = self.swarm.clone();
                let ctx = SessionCtx {
                    recorder: self.recorder.clone(),
                    agents,
                    swarm,
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
                    .display_title()
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
        let mut fields: Vec<(&str, String)> = vec![
            ("model", self.model.clone()),
            (
                "provider",
                crate::init::provider_label(&self.provider_base_url),
            ),
            ("cwd", short_path(&self.repo_root)),
            ("mode", self.mode.as_str().to_string()),
            ("effort", self.effort.to_string()),
        ];

        let instructions = self.instructions.labels();
        if !instructions.is_empty() {
            fields.push(("instructions", instructions.join(", ")));
        }
        fields.push((
            "tools",
            crate::chat::tool_names(self.mode.can_edit(), true).join(", "),
        ));
        if !self.agents.is_empty() {
            let names: Vec<&str> = self.agents.iter().map(|a| a.name.as_str()).collect();
            fields.push(("agents", names.join(", ")));
        }
        let skills = crate::chat::discover_skills(&self.repo_root);
        if !skills.is_empty() {
            fields.push(("skills", listed(skills.iter().map(|s| s.name.as_str()), 10)));
        }
        // MCP is deliberately absent: the `mcp connected` note lands on its
        // own once the servers finish starting.
        history::welcome(&fields, self.width)
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
                        Span::styled(format!("  {}", c.desc), theme::get().text_style()),
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
                        Span::styled(format!("  {what}"), theme::get().text_style()),
                    ]));
                }
                let block = history::assistant(lines, true, width);
                self.emit(block);
            }
            "compact" => self.start_compact(client, pane.sender()),
            "status" => self.show_status(),
            "diff" | "d" => self.show_diff(),
            "mcp" => self.show_mcp(pane),
            "skills" => self.open_skills_picker(pane),
            "memory" => self.show_memory(),
            "quit" | "q" | "exit" => self.should_quit = true,
            // A skill name typed as a command starts a message that applies
            // it, with anything after the name carried along as the task.
            other => {
                let skills = crate::chat::discover_skills(&self.repo_root);
                match skills.get(other) {
                    Some(skill) => {
                        let task = arg.unwrap_or_default();
                        pane.composer
                            .insert_str(&format!("Use the \"{}\" skill: {task}", skill.name));
                    }
                    None => self.note(&format!("unknown command: /{other} (try /help)")),
                }
            }
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
        if !crate::chat::can_compact(&self.history) {
            self.note("nothing to compact yet");
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

    /// Bold-label rows rendered like the /help block. Each description is
    /// clipped to its one row: a long one reads as a teaser, never a wall.
    fn emit_rows(&mut self, title: &str, rows: Vec<(String, String)>) {
        let width = self.width;
        let mut lines = vec![Line::from(Span::styled(
            title.to_string(),
            Style::default().add_modifier(Modifier::BOLD),
        ))];
        let pad = rows
            .iter()
            .map(|(k, _)| k.chars().count())
            .max()
            .unwrap_or(0);
        // The cell body wraps at width minus the gutter; whatever the key
        // column leaves over is the room one description gets.
        let room = width.saturating_sub(4 + pad + 2).clamp(20, 120);
        for (key, value) in rows {
            let first = value.lines().next().unwrap_or_default();
            lines.push(Line::from(vec![
                Span::styled(format!("{key:<pad$}"), theme::get().accent_style()),
                Span::styled(
                    format!("  {}", super::helpers::clip_row(first, room)),
                    theme::get().dim_style(),
                ),
            ]));
        }
        let block = history::assistant(lines, true, width);
        self.emit(block);
    }

    fn session_id(&self) -> Option<String> {
        let recorder = self.recorder.as_ref()?;
        recorder.lock().ok().map(|w| w.id().to_string())
    }

    fn show_status(&mut self) {
        let chars: usize = self.history.iter().map(|m| m.content.len()).sum();
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

    /// The `/mcp` control panel: every configured server with its state;
    /// choosing one flips `disabled` in whichever config file declares it.
    fn show_mcp(&mut self, pane: &mut BottomPane<AppEvent>) {
        let settings = match crate::settings::Settings::load(Some(&self.repo_root)) {
            Ok(s) => s,
            Err(e) => {
                self.note(&format!("could not read config: {e:#}"));
                return;
            }
        };
        if settings.mcp.servers.is_empty() {
            self.note("no MCP servers configured (add them to .mcp.json or `mcp:` in aster.yaml, or run `aster mcp import`)");
            return;
        }
        let connected: Vec<String> = self
            .mcp
            .as_ref()
            .map(|rt| rt.server_names())
            .unwrap_or_default();
        let items = settings
            .mcp
            .servers
            .iter()
            .map(|(name, config)| {
                let state = if config.disabled {
                    "disabled"
                } else if connected.contains(name) {
                    "connected"
                } else {
                    "enabled (not connected)"
                };
                SelectionItem {
                    name: format!("{} {name}", if config.disabled { "◻" } else { "◼" }),
                    description: format!("{state} · {} {}", config.command, config.args.join(" ")),
                    is_current: false,
                    event: AppEvent::McpToggle {
                        name: name.clone(),
                        disabled: !config.disabled,
                    },
                }
            })
            .collect();
        pane.push_picker("MCP servers — enter toggles on/off", items);
    }

    fn toggle_mcp(&mut self, name: &str, disabled: bool) {
        match crate::mcp::toggle_server(Some(&self.repo_root), name, disabled) {
            Ok(path) => {
                let verb = if disabled { "disabled" } else { "enabled" };
                self.note(&format!(
                    "{verb} {name} in {} (takes effect next session)",
                    short_path(&path)
                ));
            }
            Err(e) => self.note(&format!("could not toggle {name}: {e:#}")),
        }
    }

    fn open_skills_picker(&mut self, pane: &mut BottomPane<AppEvent>) {
        let skills = crate::chat::discover_skills(&self.repo_root);
        if skills.is_empty() {
            self.note("no skills installed (put SKILL.md folders under .aster/skills/)");
            return;
        }
        let items = skills
            .iter()
            .map(|s| SelectionItem {
                name: s.name.clone(),
                description: clip_row(&s.description, 60),
                is_current: false,
                event: AppEvent::SkillPicked(s.name.clone()),
            })
            .collect();
        pane.push_picker("Skills", items);
    }

    fn open_skill_actions(&mut self, name: &str, pane: &mut BottomPane<AppEvent>) {
        let items = vec![
            SelectionItem {
                name: "use".into(),
                description: "start a message that applies this skill".into(),
                is_current: false,
                event: AppEvent::SkillUse(name.to_string()),
            },
            SelectionItem {
                name: "view".into(),
                description: "show the full description and path".into(),
                is_current: false,
                event: AppEvent::SkillView(name.to_string()),
            },
            SelectionItem {
                name: "delete".into(),
                description: "remove the skill folder from disk".into(),
                is_current: false,
                event: AppEvent::SkillDelete(name.to_string()),
            },
        ];
        pane.push_picker(&format!("Skill: {name}"), items);
    }

    /// Deleting removes a folder from disk, so it gets the same ask-first
    /// treatment as YOLO; declining returns to the skill's action menu.
    fn confirm_skill_delete(&mut self, name: &str, pane: &mut BottomPane<AppEvent>) {
        let skills = crate::chat::discover_skills(&self.repo_root);
        let Some(skill) = skills.get(name) else {
            self.note(&format!("no skill named {name:?}"));
            return;
        };
        let folder = skill
            .path
            .parent()
            .map(short_path)
            .unwrap_or_else(|| short_path(&skill.path));
        pane.push_picker(
            &format!("Delete {name}?"),
            vec![
                SelectionItem {
                    name: "No, keep it".into(),
                    description: String::new(),
                    is_current: true,
                    event: AppEvent::SkillPicked(name.to_string()),
                },
                SelectionItem {
                    name: "Yes, delete it".into(),
                    description: format!("removes {folder}"),
                    is_current: false,
                    event: AppEvent::SkillDeleteConfirmed(name.to_string()),
                },
            ],
        );
    }

    fn show_skill(&mut self, name: &str) {
        let skills = crate::chat::discover_skills(&self.repo_root);
        let Some(skill) = skills.get(name) else {
            self.note(&format!("no skill named {name:?}"));
            return;
        };
        let width = self.width;
        let mut lines = vec![Line::from(Span::styled(
            format!("Skill: {name}"),
            Style::default().add_modifier(Modifier::BOLD),
        ))];
        lines.push(Line::from(Span::styled(
            short_path(&skill.path),
            theme::get().dim_style(),
        )));
        lines.push(Line::from(""));
        for line in skill.description.lines() {
            lines.push(Line::from(Span::styled(
                line.to_string(),
                theme::get().text_style(),
            )));
        }
        let block = history::assistant(lines, true, width);
        self.emit(block);
    }

    fn delete_skill(&mut self, name: &str) {
        let skills = crate::chat::discover_skills(&self.repo_root);
        let Some(skill) = skills.get(name) else {
            self.note(&format!("no skill named {name:?}"));
            return;
        };
        let Some(folder) = skill.path.parent() else {
            self.note(&format!("skill {name} has no folder to delete"));
            return;
        };
        match std::fs::remove_dir_all(folder) {
            Ok(()) => self.note(&format!("deleted skill {name} ({})", short_path(folder))),
            Err(e) => self.note(&format!("could not delete {name}: {e}")),
        }
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
#[path = "tests/chat_test.rs"]
mod tests;
