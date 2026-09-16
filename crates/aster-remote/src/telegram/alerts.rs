//! `/alerts`: what the phone tells the chat on its own. The settings live in
//! the phone app, so every read and change goes through `asterctl alerts`.

use serde_json::{Value, json};

use super::{Api, CLI_TIMEOUT, TelegramConfig, callback_message_ids};
use crate::markdown;

/// Telegram refuses callback data over 64 bytes.
const CALLBACK_MAX: usize = 64;

#[derive(Debug, PartialEq)]
pub(crate) struct AlertSettings {
    pub(crate) battery: bool,
    pub(crate) levels: Vec<u8>,
    /// Label and package, in the order the phone lists them.
    pub(crate) apps: Vec<(String, String)>,
}

/// Read what `asterctl alerts` prints; its own error line comes back as the error.
pub(crate) fn parse_alerts(raw: &str) -> Result<AlertSettings, String> {
    let raw = raw.trim();
    if let Some(error) = raw.strip_prefix("error: ") {
        return Err(error.lines().next().unwrap_or(error).to_string());
    }
    let mut lines = raw.lines();
    let head = lines.next().unwrap_or_default();
    let field = |key: &str| {
        head.split_whitespace()
            .find_map(|part| part.strip_prefix(key)?.strip_prefix('='))
    };
    let battery = match field("battery") {
        Some("on") => true,
        Some("off") => false,
        _ => return Err(format!("unexpected reply from the phone: {head}")),
    };
    let levels = field("levels")
        .unwrap_or_default()
        .split(',')
        .filter_map(|level| level.parse().ok())
        .collect();
    let apps = lines
        .filter_map(|line| {
            let (label, rest) = line.trim().rsplit_once("  (")?;
            Some((
                label.trim().to_string(),
                rest.strip_suffix(')')?.to_string(),
            ))
        })
        .collect();
    Ok(AlertSettings {
        battery,
        levels,
        apps,
    })
}

pub(crate) fn render_alerts(settings: &AlertSettings) -> (String, Value) {
    let levels = settings
        .levels
        .iter()
        .map(|level| format!("{level}%"))
        .collect::<Vec<_>>()
        .join(" and ");
    let battery = match settings.battery {
        true => format!("on, at {levels}"),
        false => "off".to_string(),
    };
    let apps = match settings.apps.is_empty() {
        true => "none yet".to_string(),
        false => settings
            .apps
            .iter()
            .map(|(label, _)| markdown::escape(label))
            .collect::<Vec<_>>()
            .join(", "),
    };
    let text = format!(
        "<b>Alerts</b>\n\
         🔋 Battery warnings: {battery}\n\
         🔔 Notifications sent here: {apps}\n\n\
         Add an app: <code>/alerts add whatsapp</code>\n\
         Change the levels: <code>/alerts battery 30,15</code>"
    );
    let toggle = match settings.battery {
        true => json!({ "text": "Turn battery warnings off", "callback_data": "L:b:off" }),
        false => json!({ "text": "Turn battery warnings on", "callback_data": "L:b:on" }),
    };
    let rows = std::iter::once(json!([toggle]))
        .chain(settings.apps.iter().filter_map(|(label, pkg)| {
            let data = format!("L:r:{pkg}");
            (data.len() <= CALLBACK_MAX)
                .then(|| json!([{ "text": format!("Stop {label}"), "callback_data": data }]))
        }))
        .collect();
    (text, Value::Array(rows))
}

/// The phone's answer to `alerts <args>`, or what went wrong in plain words.
async fn run(cfg: &TelegramConfig, args: &[&str]) -> Result<AlertSettings, String> {
    let bin = cfg.repo_root.join("bin").join("asterctl");
    let run = tokio::process::Command::new(&bin)
        .arg("alerts")
        .args(args)
        .kill_on_drop(true)
        .output();
    let out = match tokio::time::timeout(CLI_TIMEOUT, run).await {
        Ok(Ok(out)) => out,
        Ok(Err(_)) => {
            return Err(
                "The phone app isn't reachable. Open Aster on the phone and try again.".into(),
            );
        }
        Err(_) => return Err("The phone took too long to answer. Try again in a moment.".into()),
    };
    let stdout = String::from_utf8_lossy(&out.stdout);
    if stdout.contains("cannot reach the accessibility service") {
        return Err("Aster's accessibility service is off on the phone. Turn it back on in Settings, then try again.".into());
    }
    parse_alerts(&stdout).map_err(|error| match error.strip_prefix("no app matching ") {
        Some(rest) => format!(
            "No app on the phone is called {}. Check the name and try again.",
            rest.split(';').next().unwrap_or(rest)
        ),
        None => format!("Couldn't change alerts: {error}"),
    })
}

pub(super) async fn alerts_command(api: &Api, cfg: &TelegramConfig, chat_id: i64, arg: &str) {
    if !cfg!(target_os = "android") {
        api.send_text(chat_id, "/alerts only works on asterdroid.")
            .await;
        return;
    }
    let args: Vec<&str> = arg.split_whitespace().collect();
    match run(cfg, &args).await {
        Ok(settings) => {
            let (text, keyboard) = render_alerts(&settings);
            api.send_keyboard(chat_id, &text, keyboard).await;
        }
        Err(error) => api.send_text(chat_id, &error).await,
    }
}

/// A tap on the card changes the setting and redraws the card in place.
pub(super) async fn alerts_callback(
    api: &Api,
    cfg: &TelegramConfig,
    callback: &Value,
    callback_id: &str,
    data: &str,
) {
    let args = match data.split_once(':') {
        Some(("b", state)) => vec!["battery", state],
        Some(("r", pkg)) => vec!["remove", pkg],
        _ => return api.answer_callback(callback_id, "").await,
    };
    match run(cfg, &args).await {
        Ok(settings) => {
            api.answer_callback(callback_id, "Saved").await;
            if let Some((chat_id, message_id)) = callback_message_ids(callback) {
                let (text, keyboard) = render_alerts(&settings);
                api.edit_html_keyboard(chat_id, message_id, &text, keyboard)
                    .await;
            }
        }
        Err(error) => api.answer_callback(callback_id, &error).await,
    }
}
