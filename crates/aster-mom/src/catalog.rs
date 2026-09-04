//! The model catalog: metadata resolution runs against (spec 7.5).

use anyhow::{Context, Result};
use serde::Deserialize;

/// One catalog model: the fields a catalog MUST provide (spec 7.5),
/// plus an optional quality score used for `power` ranking.
#[derive(Debug, Clone, Deserialize)]
pub struct CatalogModel {
    pub id: String,
    pub window: u64,
    pub in_per_m: f64,
    pub out_per_m: f64,
    pub reasons: bool,
    pub images: bool,
    pub tools: bool,
    #[serde(default)]
    pub quality: Option<f64>,
}

impl CatalogModel {
    /// The provider half of the id (`anthropic/claude-opus-5` -> `anthropic`).
    pub fn provider(&self) -> &str {
        self.id.split('/').next().unwrap_or(&self.id)
    }

    /// Blended per-token price used as the quality proxy and cost ranking.
    pub fn blended_price(&self) -> f64 {
        (self.in_per_m * 3.0 + self.out_per_m) / 4.0
    }

    /// Ranking key for `power`: quality when the catalog has it, price as proxy.
    pub fn rank_score(&self) -> f64 {
        self.quality.unwrap_or_else(|| self.blended_price())
    }
}

#[derive(Debug, Deserialize)]
struct Snapshot {
    models: Vec<CatalogModel>,
}

/// A loaded catalog. Works offline from its most recent snapshot (spec 7.5).
#[derive(Debug, Clone)]
pub struct Catalog {
    models: Vec<CatalogModel>,
}

impl Catalog {
    pub fn builtin() -> Self {
        Self::from_json(include_str!("../snapshot.json")).unwrap_or(Self { models: Vec::new() })
    }

    pub fn from_json(text: &str) -> Result<Self> {
        let snapshot: Snapshot =
            serde_json::from_str(text).context("catalog snapshot is not valid JSON")?;
        Ok(Self {
            models: snapshot.models,
        })
    }

    pub fn models(&self) -> &[CatalogModel] {
        &self.models
    }

    /// Looks a model up by exact id, or by bare name across providers.
    pub fn find(&self, id: &str) -> Option<&CatalogModel> {
        self.models.iter().find(|m| m.id == id).or_else(|| {
            self.models
                .iter()
                .find(|m| m.id.split('/').nth(1) == Some(id))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_builtin_snapshot_parses() {
        let catalog = Catalog::builtin();
        assert!(catalog.models().len() >= 10);
    }

    #[test]
    fn catalog_find_by_full_and_bare_id() {
        let catalog = Catalog::builtin();
        assert!(catalog.find("anthropic/claude-opus-5").is_some());
        assert_eq!(
            catalog.find("claude-opus-5").map(|m| m.id.as_str()),
            Some("anthropic/claude-opus-5")
        );
        assert!(catalog.find("nonexistent/model").is_none());
    }
}
