//! `aster acp`: serve the agent over the Agent Client Protocol on stdio, so
//! editors such as Zed drive it as an external agent.

mod events;
mod prompts;
mod session;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::schema::v1::{
    AgentCapabilities, AuthMethod, AuthMethodTerminal, AuthenticateRequest, AuthenticateResponse,
    AvailableCommand, AvailableCommandsUpdate, CancelNotification, ContentBlock, ContentChunk,
    EmbeddedResourceResource, Implementation, InitializeRequest, InitializeResponse,
    LoadSessionRequest, LoadSessionResponse, NewSessionRequest, NewSessionResponse,
    PromptCapabilities, PromptRequest, PromptResponse, SessionConfigOptionValue, SessionId,
    SessionNotification, SessionUpdate, SetSessionConfigOptionRequest,
    SetSessionConfigOptionResponse, SetSessionModeRequest, SetSessionModeResponse, StopReason,
};
use agent_client_protocol::{Agent, Client, ConnectionTo, Error, Stdio};
use anyhow::Result;
use aster_ai::ChatMessage;
use clap::Args;

use crate::chat::PermissionModeArg;
use crate::config::provider::MissingCredentials;
use events::Sink;
use session::{OpenOptions, Session};

const LOGIN_HINT: &str =
    "Run `aster login` (or `aster init`) in a terminal, then start a new thread.";
const INTERNAL_ERROR: i32 = -32603;
const AUTH_REQUIRED: i32 = -32000;

#[derive(Args)]
pub(crate) struct AcpArgs {
    /// Model override (else ASTER_MODEL, aster.yaml, default).
    #[arg(long, value_name = "MODEL")]
    model: Option<String>,

    /// Permission mode every new thread starts in; the editor can change it.
    #[arg(long, value_name = "MODE", value_enum)]
    permission_mode: Option<PermissionModeArg>,

    /// Skip connecting MCP servers when a thread opens.
    #[arg(long)]
    no_mcp: bool,

    /// Echo every protocol line to stderr.
    #[arg(long)]
    trace: bool,
}

struct Server {
    opts: OpenOptions,
    sessions: Mutex<HashMap<String, Arc<Session>>>,
}

impl Server {
    fn get(&self, id: &SessionId) -> Option<Arc<Session>> {
        self.sessions.lock().ok()?.get(id.0.as_ref()).cloned()
    }

    fn insert(&self, session: Arc<Session>) {
        if let Ok(mut sessions) = self.sessions.lock() {
            sessions.insert(session.id.clone(), session);
        }
    }
}

pub(crate) async fn run(args: AcpArgs) -> Result<()> {
    let server = Arc::new(Server {
        opts: OpenOptions {
            model: args.model,
            mode: args.permission_mode.map(Into::into),
            no_mcp: args.no_mcp,
        },
        sessions: Mutex::new(HashMap::new()),
    });
    let transport = if args.trace {
        Stdio::new().with_debug(|line, direction| eprintln!("acp {direction:?}: {line}"))
    } else {
        Stdio::new()
    };

    let new_server = server.clone();
    let load_server = server.clone();
    let mode_server = server.clone();
    let config_server = server.clone();
    let prompt_server = server.clone();
    let cancel_server = server.clone();

    Agent
        .builder()
        .name("aster")
        .on_receive_request(
            async move |_request: InitializeRequest, responder, _cx| {
                responder.respond(initialize_response())
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |_request: AuthenticateRequest, responder, _cx| {
                let _ = responder.respond(AuthenticateResponse::new());
                Ok::<(), Error>(())
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: NewSessionRequest, responder, cx: ConnectionTo<Client>| {
                let server = new_server.clone();
                let spawned = cx.clone();
                cx.spawn(async move {
                    match session::open(&request.cwd, None, &server.opts).await {
                        Ok((session, _)) => {
                            server.insert(session.clone());
                            let response = NewSessionResponse::new(session.id.clone())
                                .modes(session.mode_state())
                                .config_options(session.config_options());
                            responder.respond(response)?;
                            announce_commands(&spawned, &session);
                            Ok(())
                        }
                        Err(err) => responder.respond_with_error(to_error(err)),
                    }
                })
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: LoadSessionRequest, responder, cx: ConnectionTo<Client>| {
                let server = load_server.clone();
                let spawned = cx.clone();
                cx.spawn(async move {
                    let id = request.session_id.0.to_string();
                    match session::open(&request.cwd, Some(&id), &server.opts).await {
                        Ok((session, prior)) => {
                            server.insert(session.clone());
                            replay(&spawned, &request.session_id, &prior);
                            let response = LoadSessionResponse::new()
                                .modes(session.mode_state())
                                .config_options(session.config_options());
                            responder.respond(response)?;
                            announce_commands(&spawned, &session);
                            Ok(())
                        }
                        Err(err) => responder.respond_with_error(to_error(err)),
                    }
                })
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: SetSessionModeRequest, responder, _cx| {
                let Some(session) = mode_server.get(&request.session_id) else {
                    return responder.respond_with_error(unknown_session());
                };
                let Some(mode) = session::mode_from_id(request.mode_id.0.as_ref()) else {
                    return responder.respond_with_error(Error::invalid_params());
                };
                match session.set_mode(mode) {
                    Ok(()) => responder.respond(SetSessionModeResponse::new()),
                    Err(err) => responder.respond_with_error(to_error(err)),
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: SetSessionConfigOptionRequest,
                        responder,
                        cx: ConnectionTo<Client>| {
                let Some(session) = config_server.get(&request.session_id) else {
                    return responder.respond_with_error(unknown_session());
                };
                let SessionConfigOptionValue::ValueId { value } = &request.value else {
                    return responder.respond_with_error(Error::invalid_params());
                };
                let (id, value) = (request.config_id.0.to_string(), value.0.to_string());
                cx.spawn(async move {
                    match session.set_config(&id, &value).await {
                        Ok(()) => responder.respond(SetSessionConfigOptionResponse::new(
                            session.config_options(),
                        )),
                        Err(err) => responder.respond_with_error(to_error(err)),
                    }
                })
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: PromptRequest, responder, cx: ConnectionTo<Client>| {
                let Some(session) = prompt_server.get(&request.session_id) else {
                    return responder.respond_with_error(unknown_session());
                };
                let prompt = prompt_text(&request.prompt);
                let spawned = cx.clone();
                cx.spawn(async move {
                    let sink = Sink::new(
                        spawned.clone(),
                        request.session_id.clone(),
                        session.repo_root.clone(),
                    );
                    let sink = Arc::new(sink.into_chat_sink());
                    let approver = prompts::spawn_approver(spawned, session.clone());
                    match session.turn(prompt, approver, sink).await {
                        Ok(outcome) => {
                            let stop = if outcome.cancelled {
                                StopReason::Cancelled
                            } else {
                                StopReason::EndTurn
                            };
                            responder.respond(PromptResponse::new(stop))
                        }
                        Err(err) => responder.respond_with_error(to_error(err)),
                    }
                })
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_notification(
            async move |notification: CancelNotification, _cx| {
                if let Some(session) = cancel_server.get(&notification.session_id) {
                    session.cancel();
                }
                Ok::<(), Error>(())
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .connect_to(transport)
        .await
        .map_err(|err| anyhow::anyhow!("acp connection failed: {err}"))
}

fn initialize_response() -> InitializeResponse {
    let capabilities = AgentCapabilities::new()
        .load_session(true)
        .prompt_capabilities(PromptCapabilities::new().embedded_context(true));
    // A terminal method: the editor runs `aster login` in a terminal when a
    // thread opens without credentials, instead of only showing the hint.
    let login = AuthMethodTerminal::new("login", "Sign in")
        .description("Opens the browser sign-in, then start a new thread".to_string())
        .args(vec!["login".to_string()]);
    InitializeResponse::new(ProtocolVersion::V1)
        .agent_capabilities(capabilities)
        .auth_methods(vec![AuthMethod::Terminal(login)])
        .agent_info(Implementation::new("aster", env!("CARGO_PKG_VERSION")))
}

fn announce_commands(cx: &ConnectionTo<Client>, session: &Session) {
    let commands: Vec<AvailableCommand> = session
        .ctx
        .skills
        .iter()
        .map(|skill| AvailableCommand::new(skill.name.clone(), skill.description.clone()))
        .collect();
    if commands.is_empty() {
        return;
    }
    let update = SessionUpdate::AvailableCommandsUpdate(AvailableCommandsUpdate::new(commands));
    let _ = cx.send_notification(SessionNotification::new(
        SessionId::new(session.id.as_str()),
        update,
    ));
}

fn replay(cx: &ConnectionTo<Client>, session_id: &SessionId, prior: &[ChatMessage]) {
    for message in prior {
        let chunk = ContentChunk::new(message.content.text().into_owned().into());
        let update = match message.role.as_str() {
            "user" => SessionUpdate::UserMessageChunk(chunk),
            "assistant" => SessionUpdate::AgentMessageChunk(chunk),
            _ => continue,
        };
        let _ = cx.send_notification(SessionNotification::new(session_id.clone(), update));
    }
}

fn prompt_text(blocks: &[ContentBlock]) -> String {
    let mut text = String::new();
    let mut context = Vec::new();
    for block in blocks {
        match block {
            ContentBlock::Text(t) => text.push_str(&t.text),
            ContentBlock::ResourceLink(link) => {
                let uri = link.uri.strip_prefix("file://").unwrap_or(&link.uri);
                text.push_str(&format!("[{}]({uri})", link.name));
            }
            ContentBlock::Resource(resource) => {
                if let EmbeddedResourceResource::TextResourceContents(contents) = &resource.resource
                {
                    let uri = contents
                        .uri
                        .strip_prefix("file://")
                        .unwrap_or(&contents.uri);
                    context.push(format!(
                        "<context path=\"{uri}\">\n{}\n</context>",
                        contents.text
                    ));
                }
            }
            _ => {}
        }
    }
    if !context.is_empty() {
        text.push_str("\n\n");
        text.push_str(&context.join("\n\n"));
    }
    text
}

fn unknown_session() -> Error {
    Error::new(INTERNAL_ERROR, "unknown session; open a new thread")
}

fn to_error(err: anyhow::Error) -> Error {
    if let Some(missing) = err.downcast_ref::<MissingCredentials>() {
        return Error::new(AUTH_REQUIRED, format!("{missing} {LOGIN_HINT}"));
    }
    Error::new(INTERNAL_ERROR, format!("{err:#}"))
}
