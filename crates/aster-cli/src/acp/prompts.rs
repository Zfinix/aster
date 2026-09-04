//! Route the agent's approvals and questions through ACP permission requests,
//! the one interactive prompt the protocol gives an agent.

use std::path::Path;
use std::sync::Arc;

use agent_client_protocol::schema::v1::{
    CurrentModeUpdate, PermissionOption, PermissionOptionKind, RequestPermissionOutcome,
    RequestPermissionRequest, SessionId, SessionUpdate, ToolCallStatus, ToolCallUpdate,
    ToolCallUpdateFields, ToolKind,
};
use agent_client_protocol::{Client, ConnectionTo};
use aster_policy::Mode;
use tokio::sync::mpsc;

use super::session::{Session, mode_id};
use crate::chat::{Answer, ApprovalRequest, QuestionRequest, UiRequest, UiSender};

const MAX_TITLE_CHARS: usize = 80;

pub(super) fn spawn_approver(cx: ConnectionTo<Client>, session: Arc<Session>) -> UiSender {
    let (tx, mut rx) = mpsc::channel::<UiRequest>(1);
    tokio::spawn(async move {
        let session_id = SessionId::new(session.id.as_str());
        let mut counter = 0usize;
        while let Some(request) = rx.recv().await {
            counter += 1;
            match request {
                UiRequest::Approval(approval) => {
                    let scopeless = approval.scope.is_none();
                    let (answer, respond) =
                        ask_approval(&cx, &session_id, counter, approval, false).await;
                    // "Always" on an in-repo edit means stop asking, as in the TUI;
                    // with a scope it is a grant the loop records on its own.
                    if answer == Answer::Always && scopeless {
                        promote(&cx, &session_id, &session);
                    }
                    let _ = respond.send(answer);
                }
                UiRequest::PlanApproval(approval) => {
                    let (answer, respond) =
                        ask_approval(&cx, &session_id, counter, approval, true).await;
                    if answer.allowed() {
                        promote(&cx, &session_id, &session);
                    }
                    let _ = respond.send(answer);
                }
                UiRequest::Question(question) => {
                    let (answer, respond) = ask_question(&cx, &session_id, counter, question).await;
                    let _ = respond.send(answer);
                }
            }
        }
    });
    tx
}

fn promote(cx: &ConnectionTo<Client>, session_id: &SessionId, session: &Session) {
    if session.mode().can_edit() {
        return;
    }
    if let Err(err) = session.set_mode(Mode::Edit) {
        tracing::warn!("acp: could not promote the session: {err:#}");
        return;
    }
    let update = SessionUpdate::CurrentModeUpdate(CurrentModeUpdate::new(mode_id(Mode::Edit)));
    let _ = cx.send_notification(agent_client_protocol::schema::v1::SessionNotification::new(
        session_id.clone(),
        update,
    ));
}

async fn ask_approval(
    cx: &ConnectionTo<Client>,
    session_id: &SessionId,
    counter: usize,
    approval: ApprovalRequest,
    plan: bool,
) -> (Answer, tokio::sync::oneshot::Sender<Answer>) {
    let ApprovalRequest {
        preview,
        markdown,
        scope,
        respond,
    } = approval;
    let title = if plan {
        "Approve the plan".to_string()
    } else {
        first_line(&preview)
    };
    let kind = if plan {
        ToolKind::SwitchMode
    } else if looks_like_diff(&preview) {
        ToolKind::Edit
    } else {
        ToolKind::Execute
    };
    let body = markdown.unwrap_or(preview);
    let fields = ToolCallUpdateFields::new()
        .title(title)
        .kind(kind)
        .status(ToolCallStatus::Pending)
        .content(vec![body.into()]);
    let update = ToolCallUpdate::new(format!("aster-approval-{counter}"), fields);

    let mut options = vec![PermissionOption::new(
        "allow",
        if plan { "Approve" } else { "Allow" },
        PermissionOptionKind::AllowOnce,
    )];
    // Without a scope, "always" means stop asking for this repo: the answer
    // promotes the session to `edit`, as it does in the TUI.
    if !plan {
        let name = match &scope {
            Some(dir) => format!("Always allow in {}", short_path(dir)),
            None => "Always allow".to_string(),
        };
        options.push(PermissionOption::new(
            "always",
            name,
            PermissionOptionKind::AllowAlways,
        ));
    }
    options.push(PermissionOption::new(
        "reject",
        "Reject",
        PermissionOptionKind::RejectOnce,
    ));

    let answer = match selected(cx, session_id, update, options).await.as_deref() {
        Some("allow") => Answer::Yes,
        Some("always") => Answer::Always,
        _ => Answer::No,
    };
    (answer, respond)
}

async fn ask_question(
    cx: &ConnectionTo<Client>,
    session_id: &SessionId,
    counter: usize,
    question: QuestionRequest,
) -> (Option<String>, tokio::sync::oneshot::Sender<Option<String>>) {
    let QuestionRequest {
        header,
        question,
        options,
        respond,
    } = question;
    let title = if header.trim().is_empty() {
        first_line(&question)
    } else {
        header
    };
    let fields = ToolCallUpdateFields::new()
        .title(title)
        .kind(ToolKind::Think)
        .status(ToolCallStatus::Pending)
        .content(vec![question.into()]);
    let update = ToolCallUpdate::new(format!("aster-question-{counter}"), fields);

    let mut choices: Vec<PermissionOption> = options
        .iter()
        .enumerate()
        .map(|(i, option)| {
            PermissionOption::new(
                format!("opt-{i}"),
                option.clone(),
                PermissionOptionKind::AllowOnce,
            )
        })
        .collect();
    choices.push(PermissionOption::new(
        "skip",
        "Skip",
        PermissionOptionKind::RejectOnce,
    ));

    let answer = selected(cx, session_id, update, choices)
        .await
        .and_then(|id| id.strip_prefix("opt-")?.parse::<usize>().ok())
        .and_then(|i| options.get(i).cloned());
    (answer, respond)
}

async fn selected(
    cx: &ConnectionTo<Client>,
    session_id: &SessionId,
    update: ToolCallUpdate,
    options: Vec<PermissionOption>,
) -> Option<String> {
    let request = RequestPermissionRequest::new(session_id.clone(), update, options);
    match cx.send_request(request).block_task().await {
        Ok(response) => match response.outcome {
            RequestPermissionOutcome::Selected(picked) => Some(picked.option_id.0.to_string()),
            _ => None,
        },
        Err(err) => {
            tracing::warn!("acp: permission request failed: {err}");
            None
        }
    }
}

fn first_line(text: &str) -> String {
    let line = text
        .lines()
        .map(|l| l.trim().trim_end_matches(':'))
        .find(|l| !l.is_empty())
        .unwrap_or("Approve this action");
    if line.chars().count() > MAX_TITLE_CHARS {
        let head: String = line.chars().take(MAX_TITLE_CHARS).collect();
        format!("{head}…")
    } else {
        line.to_string()
    }
}

fn looks_like_diff(preview: &str) -> bool {
    preview
        .lines()
        .any(|l| l.starts_with('+') || l.starts_with('-') || l.starts_with("@@"))
}

fn short_path(dir: &Path) -> String {
    let home = dirs::home_dir();
    match home.as_deref().and_then(|h| dir.strip_prefix(h).ok()) {
        Some(rest) => format!("~/{}", rest.display()),
        None => dir.display().to_string(),
    }
}
