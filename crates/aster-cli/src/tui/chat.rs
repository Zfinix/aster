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
use ratatui::widgets::Widget;
use serde_json::Value;
use tokio::sync::mpsc;

use super::bottom_pane::{BottomPane, CommandDesc, InputResult, SelectionItem};
use super::guard::TuiGuard;
use super::helpers::{human_count, short_path};
use super::markdown::{self, MarkdownStream};
use super::render::Renderable;
use super::terminal::{Tui, TuiEvent};
use super::{ACCENT, history, theme};
use crate::chat::{Answer, ApprovalRequest, ApprovalSender, SessionCtx};
use crate::persist::Recorder;

type ChatTurn = tokio::task::JoinHandle<Result<(String, Vec<String>, Option<Vec<ChatMessage>>)>>;

/// Read-only tools whose consecutive calls collapse into one `Explored` cell.
const READ_ONLY: &[&str] = &[
    "read_file",
    "list_files",
    "search_files",
    "recall",
    "read_skill",
];

/// Side effects routed back from the bottom pane's views.
#[derive(Clone)]
enum AppEvent {
    SetMode(Mode),
    SetEffort(Effort),
    ApprovalDecided {
        answer: Answer,
        scope: Option<PathBuf>,
    },
}

pub async fn run_chat(
    mut client: AiClient,
    repo_root: std::path::PathBuf,
    allow_edits: bool,
    perms: PermissionsConfig,
    seed: Option<String>,
    resume_latest: bool,
) -> Result<()> {
    let _guard = TuiGuard::install(super::terminal::restore_raw);
    // Idle layout is four rows: gap, status, composer, footer. Anchoring
    // smaller would make the first draw grow the viewport, and that growth
    // scrolls blank rows into the middle of the transcript.
    let mut tui = Tui::new(4)?;

    // Depth 1: the agent awaits each approval before proposing the next.
    let (approval_tx, mut approval_rx) = mpsc::channel::<ApprovalRequest>(1);
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

    let mut pane: BottomPane<AppEvent> = BottomPane::new(
        CHAT_COMMANDS,
        "Message Aster…  (/ for commands)",
        tui.frame_requester(),
        app_tx.clone(),
        |answer, scope| AppEvent::ApprovalDecided { answer, scope },
    );

    let endpoint = crate::init::provider_label(client.base_url());
    app.emit(history::welcome(
        &[
            ("model", app.model.clone()),
            ("provider", endpoint),
            ("cwd", short_path(&repo_root)),
            ("mode", mode_desc(app.mode)),
            ("effort", client.effort().to_string()),
        ],
        app.width,
    ));

    if let Ok(store) = crate::persist::store() {
        match resume_or_new(&store, &repo_root, &client.model, resume_latest) {
            Ok((recorder, seeded)) => {
                app.recorder = Some(recorder);
                app.load_history(seeded);
            }
            Err(e) => tracing::warn!("could not open session store: {e:#}"),
        }
        app.store = Some(store);
    }

    let mut turn: Option<ChatTurn> = None;
    if let Some(seed) = seed.filter(|s| !s.trim().is_empty()) {
        turn = Some(app.submit(&seed, &client, &repo_root));
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
                TuiEvent::Paste(text) => pane.handle_paste(text),
                TuiEvent::Resize => {
                    tui.resized()?;
                    app.width = tui.width() as usize;
                    frames.schedule_now();
                }
                TuiEvent::Draw => {
                    app.usage = Some(client.usage_snapshot());
                    draw(&mut tui, &app, &pane)?;
                }
            },
            Some(ev) = events_rx.recv() => {
                app.on_turn_event(ev);
                pane.set_status_detail(app.running.last().map(|t| t.label.to_lowercase()));
            }
            Some(req) = approval_rx.recv() => {
                app.on_approval_request(req, &mut pane);
            }
            Some(ev) = app_rx.recv() => {
                app.on_app_event(ev, &mut client);
            }
            res = wait_turn(&mut turn) => {
                match res {
                    Ok(Ok((reply, edited, compacted))) => app.finish_turn(&reply, &edited, compacted),
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

fn draw(tui: &mut Tui, app: &ChatApp, pane: &BottomPane<AppEvent>) -> Result<()> {
    let width = tui.width();
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
            if ctrl || pane.composer.is_empty() {
                return Flow::Quit;
            }
            pane.composer.clear();
            return Flow::Continue;
        }
        // Shift+tab opens the modes panel, unless it is standing in for
        // shift+enter mid-composition.
        if key.code == KeyCode::BackTab && pane.composer.is_empty() {
            app.open_mode_picker(pane);
            return Flow::Continue;
        }
    } else if ctrl && key.code == KeyCode::Char('c') {
        pane.handle_key(key, app.width as u16);
        return Flow::Quit;
    }

    match pane.handle_key(key, app.width as u16) {
        InputResult::Submitted(text) => {
            *turn = Some(app.submit(&text, client, repo_root));
            pane.set_task_running(true);
        }
        InputResult::Command(cmd) => {
            app.handle_command(&cmd, client, pane);
        }
        InputResult::None => {}
    }
    app.flash = None;
    Flow::Continue
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
fn resume_or_new(
    store: &Store,
    repo_root: &std::path::Path,
    model: &str,
    resume_latest: bool,
) -> Result<(Recorder, Vec<ChatMessage>)> {
    let prev = if resume_latest {
        store.latest(repo_root)?
    } else {
        None
    };
    if let Some(prev) = prev {
        let messages = prev.to_chat_messages();
        let writer = store.resume_writer(repo_root, &prev.meta.id)?;
        Ok((sync::Arc::new(sync::Mutex::new(writer)), messages))
    } else {
        let writer = store.new_session(repo_root, repo_root, Some(model.to_string()))?;
        Ok((sync::Arc::new(sync::Mutex::new(writer)), Vec::new()))
    }
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
            Mode::Edit => self.edit.clone(),
        }
    }
}

/// Picker order: plan → manual → auto → edit.
const MODE_ORDER: [Mode; 4] = [Mode::Plan, Mode::Manual, Mode::Auto, Mode::Edit];

/// The accent each mode wears in the footer. Amber marks the modes that stop
/// to ask; plan stays a quiet gray.
fn mode_color(mode: Mode) -> Color {
    match mode {
        Mode::Edit => ACCENT,
        Mode::Auto => ACCENT,
        Mode::Manual => theme::AMBER,
        Mode::Plan => theme::DIMMER,
    }
}

/// `name · what it does`, for the welcome header and footer flashes.
fn mode_desc(mode: Mode) -> String {
    format!("{} · {}", mode.as_str(), mode.description())
}

/// Opens every mid-conversation note about the edit tool, so a later toggle can
/// find and replace the previous one.
const EDIT_NOTE_PREFIX: &str = "Edits are now ";

fn is_edit_note(msg: &ChatMessage) -> bool {
    msg.role == "system" && msg.content.starts_with(EDIT_NOTE_PREFIX)
}

const CHAT_COMMANDS: &[CommandDesc] = &[
    CommandDesc {
        name: "model",
        takes_arg: true,
        desc: "Switch the active model, or show it with no argument",
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
    /// Consecutive read-only calls, collapsed into one `Explored` cell.
    explored: Vec<String>,
    running: Vec<RunningTool>,
    /// A tool cell was emitted since the last prose, so the next prose opens
    /// with a rule dividing the work from the answer.
    worked: bool,

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
    approval_tx: ApprovalSender,
    events_tx: mpsc::Sender<TurnEvent>,
    should_quit: bool,
    /// Transient footer status, cleared on the next keystroke.
    flash: Option<String>,
    /// Set by `/clear`; the run loop wipes the screen on the next pass.
    clear_requested: bool,
}

impl ChatApp {
    fn new(
        mode: Mode,
        effort: Effort,
        edits_locked: bool,
        model: String,
        perms: SessionPermissions,
        approval_tx: ApprovalSender,
        events_tx: mpsc::Sender<TurnEvent>,
    ) -> Self {
        Self {
            queue: VecDeque::new(),
            markdown: MarkdownStream::default(),
            speaking: false,
            streamed: String::new(),
            explored: Vec::new(),
            running: Vec::new(),
            worked: false,
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

    /* ---- streaming ---- */

    fn on_turn_event(&mut self, ev: TurnEvent) {
        match ev {
            TurnEvent::Token(delta) => {
                self.end_explored();
                self.streamed.push_str(&delta);
                let lines = self.markdown.push(&delta);
                if !lines.is_empty() {
                    self.divide_work_from_answer();
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
        }
    }

    fn on_tool_result(&mut self, tool: RunningTool, result: &str, failed: bool) {
        if !failed && READ_ONLY.contains(&tool.name.as_str()) {
            self.explored.push(tool.label);
            return;
        }
        self.end_explored();
        let block = if failed {
            history::tool(&tool.label, result, true, self.width)
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
        self.worked = true;
    }

    /// Close the assistant message in flight, emitting its trailing partial line.
    fn end_message(&mut self) {
        if !self.markdown.is_empty() {
            let lines = self.markdown.flush();
            if !lines.is_empty() {
                let block = history::assistant(lines, !self.speaking, self.width);
                self.emit(block);
            }
        }
        self.speaking = false;
    }

    fn end_explored(&mut self) {
        if self.explored.is_empty() {
            return;
        }
        let labels = std::mem::take(&mut self.explored);
        let block = history::explored(&labels, self.width);
        self.emit(block);
        self.worked = true;
    }

    /// Draw the rule between what the agent did and what it has to say, once
    /// per stretch of work.
    fn divide_work_from_answer(&mut self) {
        if !self.worked || self.speaking {
            return;
        }
        self.worked = false;
        let block = history::rule(self.width);
        self.emit(block);
    }

    /* ---- turns ---- */

    fn submit(&mut self, text: &str, client: &AiClient, repo_root: &std::path::Path) -> ChatTurn {
        let block = history::user(text, self.width);
        self.emit(block);
        self.history.push(ChatMessage {
            role: "user".into(),
            content: text.into(),
        });
        self.record_user(text);
        self.thinking = true;
        self.started = Some(Instant::now());
        self.streamed.clear();
        self.worked = false;

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
            probe: std::sync::Arc::new(bash_tools::ToolProbe::detect()),
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
            self.divide_work_from_answer();
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

    /* ---- approvals ---- */

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

    fn on_app_event(&mut self, ev: AppEvent, client: &mut AiClient) {
        match ev {
            AppEvent::SetMode(mode) => self.select_mode(mode),
            AppEvent::SetEffort(effort) => self.set_effort(effort, client),
            AppEvent::ApprovalDecided { answer, scope } => {
                let note = match (answer, &scope) {
                    (Answer::No, _) => Some("edit rejected".to_string()),
                    (Answer::Always, Some(dir)) => {
                        Some(format!("always allowing {}", short_path(dir)))
                    }
                    _ => None,
                };
                // "Always" on an in-repo edit means "stop asking": promote the
                // session so later requests auto-approve.
                if answer == Answer::Always && scope.is_none() && !self.edits_locked {
                    self.select_mode(Mode::Edit);
                }
                if let Some(note) = note {
                    self.note(&note);
                }
            }
        }
    }

    /* ---- modes and effort ---- */

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

    /// Apply a picker choice. A locked run stays in `plan` and says why.
    fn select_mode(&mut self, mode: Mode) {
        if self.edits_locked && mode.can_edit() {
            self.flash = Some("edits are off for this run (mode: plan)".into());
            return;
        }
        if mode == self.mode {
            return;
        }
        self.mode = mode;
        self.note_edit_mode();
        // A footer flash, not a scrollback line, so the transcript stays a
        // record of the conversation rather than of settings.
        self.flash = Some(if self.thinking {
            // The running turn cloned its tool list already.
            format!("mode {} · applies to your next message", mode_desc(mode))
        } else {
            format!("mode {}", mode_desc(mode))
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

    /* ---- commands ---- */

    /// A model change takes effect next turn, since each turn clones the client.
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
                Some(model) => {
                    client.model = model.to_string();
                    self.model = model.to_string();
                    self.note(&format!("model set to {model}"));
                }
                None => self.note(&format!("current model: {}", self.model)),
            },
            "mode" => match arg.map(|a| MODE_ORDER.iter().find(|m| m.as_str() == a)) {
                Some(Some(mode)) => self.select_mode(*mode),
                Some(None) => {
                    self.flash = Some("unknown mode (expected plan, manual, auto, or edit)".into());
                }
                None => self.open_mode_picker(pane),
            },
            "effort" => match arg.map(str::parse::<Effort>) {
                Some(Ok(effort)) => self.set_effort(effort, client),
                Some(Err(e)) => self.flash = Some(e),
                None => self.open_effort_picker(pane),
            },
            "clear" | "c" => {
                self.history.clear();
                self.start_new_session();
                // The loop wipes the screen and scrollback on the next pass;
                // any note queued now would vanish with them.
                self.queue.clear();
                self.clear_requested = true;
            }
            "help" | "h" => {
                let width = self.width;
                let mut lines = vec![Line::from(Span::styled(
                    "Commands",
                    Style::default().add_modifier(Modifier::BOLD),
                ))];
                for c in CHAT_COMMANDS {
                    lines.push(Line::from(vec![
                        Span::styled(format!("/{:<7}", c.name), Style::default().fg(ACCENT)),
                        Span::styled(format!("  {}", c.desc), theme::dim()),
                    ]));
                }
                let block = history::assistant(lines, true, width);
                self.emit(block);
            }
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

    /* ---- footer ---- */

    fn footer_line(&self) -> Line<'static> {
        let dark = theme::faint();
        let mut spans = vec![
            Span::styled(format!("  {}", self.model), dark),
            Span::styled(
                format!("  ✎ {}", self.mode.as_str()),
                Style::default().fg(mode_color(self.mode)),
            ),
            Span::styled(format!("  ⌁ {}", self.effort), dark),
        ];
        if let Some(usage) = self.usage.filter(|u| u.total_tokens > 0) {
            let approx = if usage.estimated { "~" } else { "" };
            let cost = usage
                .estimated_cost_usd
                .map(|c| format!("  ·  ~${c:.4}"))
                .unwrap_or_default();
            spans.push(Span::styled(
                format!(
                    "  ·  ctx {approx}{} in / {approx}{} out{cost}",
                    human_count(usage.prompt_tokens as usize),
                    human_count(usage.completion_tokens as usize),
                ),
                dark,
            ));
        }
        if let Some(msg) = &self.flash {
            spans.push(Span::styled("  ·  ", dark));
            spans.push(Span::styled(msg.clone(), Style::default().fg(ACCENT)));
        }
        Line::from(spans)
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
            BottomPane::new(CHAT_COMMANDS, "hint", frames, tx, |answer, scope| {
                AppEvent::ApprovalDecided { answer, scope }
            }),
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
    fn consecutive_reads_collapse_into_one_explored_cell() {
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
        assert!(app.queue.is_empty(), "the group is still open");

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
    fn a_quiet_endpoint_still_renders_its_reply() {
        let mut app = chat_app("m1".into());
        app.finish_turn("the whole answer", &[], None);
        assert!(rendered(&app).contains("the whole answer"));
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

        let (recorder, messages) = resume_or_new(&store, repo, "m", true).unwrap();
        assert_eq!(messages.len(), 2);

        let mut app = chat_app("m".into());
        app.store = Some(store);
        app.repo_root = repo.to_path_buf();
        app.recorder = Some(recorder);
        app.load_history(messages);
        assert_eq!(app.history.len(), 2);
        assert!(rendered(&app).contains("hi there"));
    }

    #[test]
    fn record_user_persists_turn() {
        let home = tempfile::tempdir().unwrap();
        let store = Store::open(home.path()).unwrap();
        let repo = std::path::Path::new("/tmp/aster-record-repo");
        let (recorder, _) = resume_or_new(&store, repo, "m", true).unwrap();

        let mut app = chat_app("m".into());
        app.store = Some(store.clone());
        app.repo_root = repo.to_path_buf();
        app.recorder = Some(recorder);
        app.record_user("remember me");

        let latest = store.latest(repo).unwrap().unwrap();
        let persisted = latest.events.iter().any(|e| {
            matches!(e, aster_persist::TranscriptEvent::Message(m)
                if m.role == "user" && m.content.as_deref() == Some("remember me"))
        });
        assert!(persisted);
    }
}
