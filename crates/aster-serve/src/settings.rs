//! What the browser chose and expects to find again: its model, permission
//! mode, effort, and endpoint. The editor extension keeps these in the host's
//! own state store; here they live next to the rest of Aster's own state.

use std::env;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Model ids picked before, most recent first. Capped so the picker stays short.
const RECENT_LIMIT: usize = 5;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub permission_mode: String,
    pub model: Option<String>,
    /// Ids typed by hand, kept alongside the vetted list.
    pub custom_models: Vec<String>,
    pub recent_models: Vec<String>,
    /// Unset until the user picks a level, so `aster.yaml` keeps deciding.
    pub effort: Option<String>,
    pub provider: Option<ProviderOverride>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            permission_mode: "edit".into(),
            model: None,
            custom_models: Vec::new(),
            recent_models: Vec::new(),
            effort: None,
            provider: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderOverride {
    pub base_url: String,
    /// Vars that may hold this endpoint's own key, from `aster models --providers`.
    #[serde(default)]
    pub key_env: Vec<String>,
}

impl ProviderOverride {
    /// This endpoint's key, from the first of its own vars that is set. `None`
    /// leaves the CLI to fall back to the shared one.
    pub fn key(&self) -> Option<String> {
        self.key_env
            .iter()
            .filter_map(|var| env::var(var).ok())
            .find(|value| !value.trim().is_empty())
    }
}

impl Settings {
    pub fn load() -> Self {
        path()
            .and_then(|path| fs::read(path).ok())
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    /// Best effort: a settings file that cannot be written is not worth
    /// refusing a turn over, and the choice still holds for this run.
    pub fn save(&self) {
        let Some(path) = path() else { return };
        if let Some(dir) = path.parent() {
            let _ = fs::create_dir_all(dir);
        }
        if let Ok(bytes) = serde_json::to_vec_pretty(self) {
            let _ = fs::write(path, bytes);
        }
    }

    /// Remember a model the picker just used, hand-typed ones included.
    pub fn remember_model(&mut self, model: &str, vetted: &[&str]) {
        self.model = Some(model.to_string());
        if !vetted.contains(&model) && !self.custom_models.iter().any(|m| m == model) {
            self.custom_models.push(model.to_string());
        }
        self.recent_models.retain(|m| m != model);
        self.recent_models.insert(0, model.to_string());
        self.recent_models.truncate(RECENT_LIMIT);
    }
}

fn path() -> Option<PathBuf> {
    Some(crate::paths::home()?.join("serve.json"))
}

#[cfg(test)]
#[path = "tests/settings_test.rs"]
mod tests;
