//! Entry resolution: wishes -> concrete models (spec 7.5-7.6).

use std::collections::BTreeSet;

use crate::catalog::{Catalog, CatalogModel};
use crate::manifest::{ModelEntry, Power, Thinking};

pub trait Access {
    fn accessible(&self, model_id: &str) -> bool;
}

impl<F: Fn(&str) -> bool> Access for F {
    fn accessible(&self, model_id: &str) -> bool {
        self(model_id)
    }
}

/// The outcome of resolving one entry, including what was skipped and why,
/// so the switch record can explain the pick (spec 7.7).
#[derive(Debug, Clone)]
pub struct Resolution {
    pub model: String,
    pub window: u64,
    pub skipped: Vec<String>,
}

/// Resolves entries against the catalog with the host tool's own floors
/// and the session's demotions applied (spec 7.5).
pub struct Resolver<'a, A: Access> {
    catalog: &'a Catalog,
    access: A,
    host_needs_tools: bool,
    demoted: BTreeSet<String>,
}

impl<'a, A: Access> Resolver<'a, A> {
    pub fn new(catalog: &'a Catalog, access: A, host_needs_tools: bool) -> Self {
        Self {
            catalog,
            access,
            host_needs_tools,
            demoted: BTreeSet::new(),
        }
    }

    pub fn demote(&mut self, model_id: &str) {
        self.demoted.insert(model_id.to_string());
    }

    pub fn clear_demotions(&mut self) {
        self.demoted.clear();
    }

    pub fn demotions(&self) -> impl Iterator<Item = &str> {
        self.demoted.iter().map(String::as_str)
    }

    pub fn resolve(&self, entry: &ModelEntry) -> Option<Resolution> {
        let mut skipped = Vec::new();

        for pin in &entry.prefer {
            let Some(model) = self.catalog.find(pin) else {
                skipped.push(format!("{pin}: not in catalog"));
                continue;
            };
            if !self.access.accessible(&model.id) {
                skipped.push(format!("{}: provider not configured", model.id));
                continue;
            }
            if self.demoted.contains(&model.id) {
                skipped.push(format!("{}: demoted this session", model.id));
                continue;
            }
            return Some(Resolution {
                model: model.id.clone(),
                window: model.window,
                skipped,
            });
        }

        let candidates = self.candidates(entry, &mut skipped);
        let pick = pick_by_power(entry.power, &candidates)?;
        Some(Resolution {
            model: pick.id.clone(),
            window: pick.window,
            skipped,
        })
    }

    fn candidates(&self, entry: &ModelEntry, skipped: &mut Vec<String>) -> Vec<&'a CatalogModel> {
        let mut out: Vec<&CatalogModel> = self
            .catalog
            .models()
            .iter()
            .filter(|m| self.access.accessible(&m.id))
            .filter(|m| satisfies(entry, m, self.host_needs_tools))
            .collect();
        let demoted: Vec<&CatalogModel> = out
            .iter()
            .copied()
            .filter(|m| self.demoted.contains(&m.id))
            .collect();
        out.retain(|m| !self.demoted.contains(&m.id));
        if out.is_empty() && demoted.len() == 1 {
            return demoted;
        }
        for m in demoted {
            skipped.push(format!("{}: demoted this session", m.id));
        }
        out
    }
}

fn satisfies(entry: &ModelEntry, model: &CatalogModel, host_needs_tools: bool) -> bool {
    model.window >= entry.memory.floor_tokens()
        && (!matches!(entry.thinking, Thinking::Some | Thinking::Deep) || model.reasons)
        && (!entry.sees_images || model.images)
        && (!(entry.uses_tools || host_needs_tools) || model.tools)
}

/// The `power` mapping (spec 7.5): candidates ranked best to worst by
/// quality score (price as proxy), then `max` takes the top, `low` the
/// cheapest, `medium` the cheapest in the upper two-thirds.
fn pick_by_power<'m>(power: Power, candidates: &[&'m CatalogModel]) -> Option<&'m CatalogModel> {
    if candidates.is_empty() {
        return None;
    }
    let mut ranked = candidates.to_vec();
    ranked.sort_by(|a, b| {
        b.rank_score()
            .partial_cmp(&a.rank_score())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let cheapest = |pool: &[&'m CatalogModel]| {
        pool.iter().copied().min_by(|a, b| {
            a.blended_price()
                .partial_cmp(&b.blended_price())
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    };
    match power {
        Power::Max => ranked.first().copied(),
        Power::Low => cheapest(&ranked),
        Power::Medium => {
            let cut = ((ranked.len() * 2).div_ceil(3)).max(1);
            let upper_two_thirds = &ranked[..cut];
            let pick = cheapest(upper_two_thirds)?;
            if ranked.len() > 1 {
                let low = cheapest(&ranked)?;
                if pick.id == low.id {
                    let alternative = upper_two_thirds.iter().copied().filter(|m| m.id != low.id);
                    return alternative
                        .min_by(|a, b| {
                            a.blended_price()
                                .partial_cmp(&b.blended_price())
                                .unwrap_or(std::cmp::Ordering::Equal)
                        })
                        .or(Some(pick));
                }
            }
            Some(pick)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_access(_: &str) -> bool {
        true
    }

    fn entry(power: Power) -> ModelEntry {
        ModelEntry {
            power,
            ..ModelEntry::default()
        }
    }

    #[test]
    fn resolve_max_takes_top_quality() {
        let catalog = Catalog::builtin();
        let resolver = Resolver::new(&catalog, all_access, false);
        let r = resolver.resolve(&entry(Power::Max)).unwrap();
        assert_eq!(r.model, "anthropic/claude-opus-5");
    }

    #[test]
    fn resolve_low_takes_cheapest() {
        let catalog = Catalog::builtin();
        let resolver = Resolver::new(&catalog, all_access, false);
        let r = resolver.resolve(&entry(Power::Low)).unwrap();
        let picked = catalog.find(&r.model).unwrap();
        let min = catalog
            .models()
            .iter()
            .map(CatalogModel::blended_price)
            .fold(f64::INFINITY, f64::min);
        assert!(picked.blended_price() <= min + f64::EPSILON);
    }

    #[test]
    fn resolve_medium_differs_from_low() {
        let catalog = Catalog::builtin();
        let resolver = Resolver::new(&catalog, all_access, false);
        let low = resolver.resolve(&entry(Power::Low)).unwrap();
        let medium = resolver.resolve(&entry(Power::Medium)).unwrap();
        assert_ne!(low.model, medium.model);
    }

    #[test]
    fn resolve_prefer_pin_wins_when_accessible() {
        let catalog = Catalog::builtin();
        let resolver = Resolver::new(&catalog, all_access, false);
        let e = ModelEntry {
            prefer: vec!["zai/glm-5".into()],
            ..entry(Power::Max)
        };
        assert_eq!(resolver.resolve(&e).unwrap().model, "zai/glm-5");
    }

    #[test]
    fn resolve_pin_to_unconfigured_provider_is_skipped_never_honored() {
        let catalog = Catalog::builtin();
        let resolver = Resolver::new(&catalog, |id: &str| !id.starts_with("zai/"), false);
        let e = ModelEntry {
            prefer: vec!["zai/glm-5".into()],
            ..entry(Power::Max)
        };
        let r = resolver.resolve(&e).unwrap();
        assert_ne!(r.model, "zai/glm-5");
        assert!(r.skipped.iter().any(|s| s.contains("not configured")));
    }

    #[test]
    fn resolve_demoted_model_is_skipped_and_recorded() {
        let catalog = Catalog::builtin();
        let mut resolver = Resolver::new(&catalog, all_access, false);
        resolver.demote("anthropic/claude-opus-5");
        let r = resolver.resolve(&entry(Power::Max)).unwrap();
        assert_ne!(r.model, "anthropic/claude-opus-5");
        assert!(r.skipped.iter().any(|s| s.contains("demoted")));
    }

    #[test]
    fn resolve_host_tool_floor_excludes_non_tool_models() {
        let catalog = Catalog::builtin();
        let resolver = Resolver::new(&catalog, all_access, true);
        let r = resolver.resolve(&entry(Power::Low)).unwrap();
        assert!(catalog.find(&r.model).unwrap().tools);
    }

    #[test]
    fn resolve_nothing_satisfies_returns_none() {
        let catalog = Catalog::builtin();
        let resolver = Resolver::new(&catalog, |_: &str| false, false);
        assert!(resolver.resolve(&entry(Power::Max)).is_none());
    }
}
