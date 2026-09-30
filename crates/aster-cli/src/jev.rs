//! The Jev loop advisor. Compiled in only with the `jev` feature; without it
//! every advise is a no-op and the loop behaves exactly as before. Even with
//! the feature, nothing happens unless the toggle is on (`ASTER_JEV` or
//! `experimental.jev`) and `ASTER_JEV_API_KEY` is set.

use std::sync::OnceLock;

use crate::settings::Experimental;

#[cfg_attr(not(feature = "jev"), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Advice {
    None,
    Retry,
    AskUser,
    Stop,
}

#[cfg_attr(not(feature = "jev"), allow(dead_code))]
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum RouteAdvice {
    Unavailable,
    Picked { entry: String, probability: f64 },
    Unsure { entry: String, probability: f64 },
    Failed(String),
}

pub(crate) struct Advisor {
    #[cfg(feature = "jev")]
    client: Option<aster_jev::JevClient>,
}

static ADVISOR: OnceLock<Advisor> = OnceLock::new();

/// Set the advisor from the loaded settings. Called once from `chat::run`.
pub(crate) fn init(experimental: &Experimental) {
    let _ = ADVISOR.set(Advisor::resolve(experimental));
}

pub(crate) fn current() -> &'static Advisor {
    ADVISOR.get_or_init(|| Advisor::resolve(&Experimental::default()))
}

#[cfg(feature = "jev")]
fn env_on(key: &str) -> bool {
    std::env::var(key).is_ok_and(|v| matches!(v.as_str(), "1" | "true" | "yes" | "on"))
}

impl Advisor {
    fn resolve(experimental: &Experimental) -> Self {
        #[cfg(feature = "jev")]
        {
            let enabled = env_on("ASTER_JEV") || experimental.jev.unwrap_or(false);
            let key = std::env::var("ASTER_JEV_API_KEY")
                .ok()
                .map(|key| key.trim().to_string())
                .filter(|key| !key.is_empty());
            Advisor {
                client: match (enabled, key) {
                    (true, Some(key)) => Some(aster_jev::JevClient::new(key)),
                    _ => None,
                },
            }
        }
        #[cfg(not(feature = "jev"))]
        {
            let _ = experimental;
            Advisor {}
        }
    }

    pub(crate) async fn advise(
        &self,
        wire: &[serde_json::Value],
        round: usize,
        round_cap: usize,
    ) -> Advice {
        #[cfg(feature = "jev")]
        {
            let Some(client) = &self.client else {
                return Advice::None;
            };
            // Before the first round there is nothing to judge but the prompt.
            if round == 0 {
                return Advice::None;
            }
            let state = aster_jev::LoopState {
                round,
                round_cap,
                recent: snapshot(wire),
            };
            match client.advise_loop(&state).await {
                Some(aster_jev::LoopAction::Retry) => Advice::Retry,
                Some(aster_jev::LoopAction::AskUser) => Advice::AskUser,
                Some(aster_jev::LoopAction::Stop) => Advice::Stop,
                Some(aster_jev::LoopAction::Continue) | None => Advice::None,
            }
        }
        #[cfg(not(feature = "jev"))]
        {
            let _ = (wire, round, round_cap);
            Advice::None
        }
    }

    pub(crate) async fn route(&self, message: &str, entries: &[(String, String)]) -> RouteAdvice {
        #[cfg(feature = "jev")]
        {
            let Some(client) = &self.client else {
                return RouteAdvice::Unavailable;
            };
            match client.route(message, entries).await {
                Ok(pick) if pick.probability >= aster_jev::ROUTE_CONFIDENCE_MIN => {
                    RouteAdvice::Picked {
                        entry: pick.entry,
                        probability: pick.probability,
                    }
                }
                Ok(pick) => RouteAdvice::Unsure {
                    entry: pick.entry,
                    probability: pick.probability,
                },
                Err(e) => RouteAdvice::Failed(format!("{e:#}")),
            }
        }
        #[cfg(not(feature = "jev"))]
        {
            let _ = (message, entries);
            RouteAdvice::Unavailable
        }
    }
}

#[cfg(feature = "jev")]
fn snapshot(wire: &[serde_json::Value]) -> String {
    const MAX_ENTRIES: usize = 6;
    const MAX_ENTRY_CHARS: usize = 512;
    let start = wire.len().saturating_sub(MAX_ENTRIES);
    wire[start..]
        .iter()
        .filter_map(|msg| {
            let role = msg.get("role")?.as_str()?;
            let content = match msg.get("content") {
                Some(content) => match content.as_str() {
                    Some(text) => text.to_string(),
                    None => content.to_string(),
                },
                None => String::new(),
            };
            let clipped = console::truncate_str(content.trim(), MAX_ENTRY_CHARS, "…").to_string();
            Some(format!("{role}: {clipped}"))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
#[path = "jev_tests.rs"]
mod tests;
