//! One ACP session: an Aster chat session bound to an editor thread, holding
//! the permission mode, policy, and history each turn of the agent loop needs.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use agent_client_protocol::schema::v1::{
    SessionConfigOption, SessionConfigOptionCategory, SessionConfigSelectGroup,
    SessionConfigSelectOption, SessionMode, SessionModeState,
};
use anyhow::{Context, Result};
use aster_ai::{AiClient, ChatMessage, Effort};
use aster_persist::MessageEvent;
use aster_policy::{Grants, Mode, PermissionsConfig, Policy};
use tokio::sync::Notify;

use crate::chat::{self, ChatEventSink, Limits, SessionCtx, SwarmLimits, UiSender};

const TITLE_TIMEOUT: Duration = Duration::from_secs(10);
const MODELS_TIMEOUT: Duration = Duration::from_secs(5);

const EFFORTS: [Effort; 7] = [
    Effort::Off,
    Effort::Low,
    Effort::Medium,
    Effort::High,
    Effort::XHigh,
    Effort::Max,
    Effort::Ultra,
];

async fn fetch_models(client: &AiClient) -> Vec<String> {
    tokio::time::timeout(MODELS_TIMEOUT, client.fetch_models())
        .await
        .ok()
        .and_then(|r| r.ok())
        .unwrap_or_default()
}

fn model_short(id: &str) -> String {
    let slug = id.rsplit('/').next().unwrap_or(id);
    slug.split('-')
        .map(case_token)
        .collect::<Vec<_>>()
        .join(" ")
}

fn case_token(word: &str) -> String {
    let all = |f: fn(char) -> bool| !word.is_empty() && word.chars().all(f);
    if all(|c| c.is_ascii_digit() || c == '.') {
        return word.to_string();
    }
    if word.len() >= 2
        && word.starts_with(['v', 'V'])
        && word[1..].chars().all(|c| c.is_ascii_digit())
    {
        return word.to_lowercase();
    }
    if word.chars().any(|c| c.is_ascii_digit()) && word.len() <= 3 {
        return word.to_uppercase();
    }
    if all(|c| c.is_ascii_alphabetic()) && !word.chars().any(|c| "aeiouyAEIOUY".contains(c)) {
        return word.to_uppercase();
    }
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

fn effort_label(effort: Effort) -> String {
    let id = effort.as_str();
    let mut chars = id.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => id.to_string(),
    }
}

pub(super) struct OpenOptions {
    pub model: Option<String>,
    pub mode: Option<Mode>,
    pub no_mcp: bool,
}

pub(super) struct TurnOutcome {
    pub cancelled: bool,
}

pub(super) struct Session {
    pub id: String,
    pub repo_root: PathBuf,
    client: Mutex<AiClient>,
    pub ctx: SessionCtx,
    grants: Arc<Grants>,
    models: Mutex<Vec<String>>,
    history: Mutex<Vec<ChatMessage>>,
    permissions: Mutex<PermissionsConfig>,
    policy: Mutex<Policy>,
    cancel_requested: AtomicBool,
    cancel: Notify,
}

/// Open a session in `cwd`: a fresh one, or the recorded transcript `resume`
/// names. The prior turns come back so a load can replay them to the editor.
pub(super) async fn open(
    cwd: &Path,
    resume: Option<&str>,
    opts: &OpenOptions,
) -> Result<(Arc<Session>, Vec<ChatMessage>)> {
    let repo_root =
        std::fs::canonicalize(cwd).with_context(|| format!("no directory {}", cwd.display()))?;
    let settings = crate::settings::Settings::load(Some(&repo_root))?;
    let client = crate::config::provider::resolve_client(&settings, opts.model.as_deref())?;

    let mut permissions = settings.permissions.clone();
    if let Some(mode) = opts.mode {
        permissions.mode = mode;
    }
    let policy = Policy::compile(&permissions)?;
    let grants = Arc::new(chat::configured_grants(&permissions, &repo_root));
    let credentials = Arc::new(chat::configured_credentials(&permissions, &repo_root));
    let yolo = permissions.mode == Mode::Yolo;

    let (mcp, problems) = if opts.no_mcp {
        (None, Vec::new())
    } else {
        crate::mcp::McpRuntime::connect_at(&settings.mcp, &repo_root).await
    };
    for problem in &problems {
        eprintln!("mcp: {problem}");
    }
    let models = fetch_models(&client).await;

    let store = crate::persist::store().ok();
    let (recorder, prior, id) = match (&store, resume) {
        (Some(store), Some(id)) => {
            let transcript = store
                .resume(&repo_root, id)
                .with_context(|| format!("no session {id:?} for this repo"))?;
            let writer = store.resume_writer(&repo_root, id)?;
            (
                Some(Arc::new(Mutex::new(writer))),
                transcript.to_chat_messages(),
                id.to_string(),
            )
        }
        (Some(store), None) => {
            let writer = store.new_session(&repo_root, &repo_root, Some(client.model.clone()))?;
            let id = writer.id().to_string();
            (Some(Arc::new(Mutex::new(writer))), Vec::new(), id)
        }
        (None, Some(id)) => anyhow::bail!("no session store, so {id:?} cannot be resumed"),
        (None, None) => (None, Vec::new(), ulid::Ulid::new().to_string()),
    };

    let ctx = SessionCtx {
        recorder,
        store,
        credentials,
        write_grants: Arc::new(chat::configured_write_grants(&repo_root)),
        skills: chat::discover_skills(&repo_root),
        instructions: Arc::new(crate::instructions::discover(&repo_root)),
        probe: Arc::new(bash_tools::ToolProbe::detect()),
        plan: Default::default(),
        mcp,
        limits: Limits::resolve(&settings.agent),
        environment: chat::environment_note(&repo_root),
        yolo,
        reads: Default::default(),
        previews: Default::default(),
        lookups: Default::default(),
        injected: Default::default(),
        agents: crate::agents::discover_agents(&repo_root),
        sub_agent: None,
        swarm: SwarmLimits::resolve(&settings.agents),
    };

    let session = Arc::new(Session {
        id,
        repo_root,
        client: Mutex::new(client),
        ctx,
        grants,
        models: Mutex::new(models),
        history: Mutex::new(prior.clone()),
        permissions: Mutex::new(permissions),
        policy: Mutex::new(policy),
        cancel_requested: AtomicBool::new(false),
        cancel: Notify::new(),
    });
    Ok((session, prior))
}

impl Session {
    pub fn mode(&self) -> Mode {
        self.permissions.lock().map(|p| p.mode).unwrap_or_default()
    }

    pub fn set_mode(&self, mode: Mode) -> Result<()> {
        let mut permissions = self
            .permissions
            .lock()
            .map_err(|_| anyhow::anyhow!("permissions lock poisoned"))?;
        permissions.mode = mode;
        let compiled = Policy::compile(&permissions)?;
        if let Ok(mut policy) = self.policy.lock() {
            *policy = compiled;
        }
        Ok(())
    }

    fn client(&self) -> AiClient {
        self.client
            .lock()
            .map(|c| c.clone())
            .unwrap_or_else(|poisoned| poisoned.into_inner().clone())
    }

    pub fn set_model(&self, model: &str) {
        if let Ok(mut client) = self.client.lock() {
            client.model = model.to_string();
        }
    }

    pub fn set_effort(&self, effort: Effort) {
        if let Ok(mut client) = self.client.lock() {
            client.set_effort(effort);
        }
    }

    /// The provider, model, and effort pickers the editor shows beside the
    /// mode picker. Models carry the same humanized names as the other UIs,
    /// with the provider's coding shortlist first.
    pub fn config_options(&self) -> Vec<SessionConfigOption> {
        let client = self.client();
        let base_url = client.base_url().trim_end_matches('/').to_string();

        let mut providers: Vec<SessionConfigSelectOption> = crate::init::provider_choices()
            .into_iter()
            .map(|(name, url, _)| {
                SessionConfigSelectOption::new(url.trim_end_matches('/').to_string(), name)
                    .description(url)
            })
            .collect();
        if !providers.iter().any(|p| p.value.0.as_ref() == base_url) {
            providers.insert(
                0,
                SessionConfigSelectOption::new(
                    base_url.clone(),
                    crate::init::provider_label(&base_url),
                )
                .description(base_url.clone()),
            );
        }

        let models = self.models.lock().map(|m| m.clone()).unwrap_or_default();
        let recommended: Vec<String> = crate::init::provider_recommended(&base_url)
            .into_iter()
            .filter(|id| models.is_empty() || models.contains(id))
            .collect();
        let mut rest: Vec<String> = models
            .iter()
            .filter(|id| !recommended.contains(id))
            .cloned()
            .collect();
        if !recommended.contains(&client.model) && !rest.contains(&client.model) {
            rest.insert(0, client.model.clone());
        }
        let row = |id: &String| {
            SessionConfigSelectOption::new(id.clone(), model_short(id)).description(id.clone())
        };
        let mut groups = Vec::new();
        if !recommended.is_empty() {
            groups.push(SessionConfigSelectGroup::new(
                "recommended",
                "Best for coding",
                recommended.iter().map(row).collect(),
            ));
        }
        if !rest.is_empty() {
            groups.push(SessionConfigSelectGroup::new(
                "available",
                "Available",
                rest.iter().map(row).collect(),
            ));
        }

        let effort_options: Vec<SessionConfigSelectOption> = EFFORTS
            .iter()
            .map(|e| SessionConfigSelectOption::new(e.as_str(), effort_label(*e)))
            .collect();
        vec![
            SessionConfigOption::select("provider", "Provider", base_url, providers)
                .category(SessionConfigOptionCategory::Other("provider".to_string())),
            SessionConfigOption::select("model", "Model", client.model.clone(), groups)
                .category(SessionConfigOptionCategory::Model),
            SessionConfigOption::select(
                "effort",
                "Effort",
                client.effort().as_str(),
                effort_options,
            )
            .category(SessionConfigOptionCategory::ThoughtLevel)
            .description("How much thinking each turn gets".to_string()),
        ]
    }

    /// Apply one picker choice by option id. The error is what the editor
    /// shows when the choice cannot be taken.
    pub async fn set_config(&self, id: &str, value: &str) -> Result<()> {
        match id {
            "model" => {
                self.set_model(value);
                Ok(())
            }
            "effort" => match EFFORTS.iter().find(|e| e.as_str() == value) {
                Some(effort) => {
                    self.set_effort(*effort);
                    Ok(())
                }
                None => anyhow::bail!("unknown effort {value:?}"),
            },
            "provider" => self.switch_provider(value).await,
            _ => anyhow::bail!("unknown option {id:?}"),
        }
    }

    async fn switch_provider(&self, base_url: &str) -> Result<()> {
        let want = base_url.trim_end_matches('/');
        let key = match aster_ai::keys::resolve_key(want) {
            Some((key, _)) => key,
            None if aster_ai::codex_api::is_codex(want) => {
                anyhow::bail!("not signed in to ChatGPT; run `aster login codex`")
            }
            None => anyhow::bail!(
                "no key found for {}; set {} or run `aster init`",
                crate::init::provider_label(want),
                aster_ai::keys::key_vars(want).join(" or ")
            ),
        };
        let model = crate::init::provider_choices()
            .into_iter()
            .find(|(_, url, _)| url.trim_end_matches('/') == want)
            .map(|(_, _, example)| example)
            .filter(|m| !m.is_empty())
            .or_else(|| crate::init::provider_recommended(want).into_iter().next());
        let client = {
            let mut client = self
                .client
                .lock()
                .map_err(|_| anyhow::anyhow!("client lock poisoned"))?;
            client.set_endpoint(want, key);
            if let Some(model) = model {
                client.model = model;
            }
            client.clone()
        };
        let models = fetch_models(&client).await;
        if let Ok(mut stored) = self.models.lock() {
            *stored = models;
        }
        Ok(())
    }

    pub fn mode_state(&self) -> SessionModeState {
        let available = aster_acp::modes()
            .map(|(_, id, name, description)| SessionMode::new(id, name).description(description))
            .collect();
        SessionModeState::new(aster_acp::mode_id(self.mode()), available)
    }

    pub fn cancel(&self) {
        self.cancel_requested.store(true, Ordering::SeqCst);
        self.cancel.notify_waiters();
    }

    async fn cancelled(&self) {
        loop {
            let notified = self.cancel.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.cancel_requested.load(Ordering::SeqCst) {
                return;
            }
            notified.await;
        }
    }

    /// Run one turn of the agent loop on `prompt`, streaming through `sink`
    /// and routing approvals through `approver`. A cancel ends the turn early.
    pub async fn turn(
        &self,
        prompt: String,
        approver: UiSender,
        sink: Arc<ChatEventSink>,
    ) -> Result<TurnOutcome> {
        let mut history = self
            .history
            .lock()
            .map_err(|_| anyhow::anyhow!("history lock poisoned"))?
            .clone();
        let mut turns = vec![ChatMessage {
            role: "user".into(),
            content: prompt.into(),
        }];
        chat::expand_skill_asks(&mut turns, &self.repo_root);
        if let Some(turn) = turns.first() {
            self.ctx.record(MessageEvent::user(turn.content.text()));
        }
        history.extend(turns);

        let policy = self
            .policy
            .lock()
            .map_err(|_| anyhow::anyhow!("policy lock poisoned"))?
            .clone();
        let allow_edits = self.mode().can_edit();
        self.cancel_requested.store(false, Ordering::SeqCst);
        let client = self.client();

        let mut edited = Vec::new();
        let outcome = {
            let run = chat::agent_loop(
                &client,
                &self.repo_root,
                &history,
                allow_edits,
                &policy,
                &self.grants,
                Some(&approver),
                &mut edited,
                &self.ctx,
                Some(&sink),
            );
            tokio::pin!(run);
            tokio::select! {
                result = &mut run => Some(result),
                () = self.cancelled() => None,
            }
        };

        let Some(result) = outcome else {
            if let Ok(mut stored) = self.history.lock() {
                *stored = history;
            }
            return Ok(TurnOutcome { cancelled: true });
        };
        let (reply, compacted) = result?;
        if let Some(compacted) = compacted {
            history = compacted;
        }
        history.push(ChatMessage {
            role: "assistant".into(),
            content: reply.into(),
        });
        if let Some(naming) = chat::name_session(&client, &self.ctx, &history, Some(sink)) {
            let _ = tokio::time::timeout(TITLE_TIMEOUT, naming).await;
        }
        if let Ok(mut stored) = self.history.lock() {
            *stored = history;
        }
        Ok(TurnOutcome { cancelled: false })
    }
}
