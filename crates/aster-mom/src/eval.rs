//! The per-turn evaluation engine (spec 7.1-7.3, 7.6-7.7).

use std::collections::BTreeMap;
use std::collections::BTreeSet;

use crate::manifest::{Condition, Manifest};
use crate::resolve::{Access, Resolution, Resolver};

#[derive(Debug, Clone, Default)]
pub struct Signals {
    pub planning_mode: Option<String>,
    pub failed_steps: u32,
    pub looping: bool,
    pub model_down: bool,
    pub spent_usd: Option<f64>,
    pub tokens_used: u64,
    pub user_turns: u64,
    pub conversation_tokens: u64,
    pub x_active: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fired {
    Rule(usize),
    Emergency(usize),
    Hold(usize),
    Router,
    StartWith,
    User,
}

#[derive(Debug, Clone)]
pub struct SwitchRecord {
    pub turn: u64,
    pub fired: Fired,
    pub from_entry: Option<String>,
    pub from_model: Option<String>,
    pub to_entry: String,
    pub to_model: String,
    pub reason: String,
    pub skipped: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Selection {
    pub entry: String,
    pub model: String,
    pub record: Option<SwitchRecord>,
}

pub struct Engine {
    manifest: Manifest,
    holds: Vec<u32>,
    held_rule: Option<usize>,
    current_entry: Option<String>,
    current_model: Option<String>,
    suspended: bool,
    switches_this_turn: u8,
    turn: u64,
    failure_counts: BTreeMap<(String, String), u32>,
    router_entry: Option<String>,
    router_hold: u32,
    router_wanted: bool,
}

const MAX_SWITCHES_PER_TURN: u8 = 2;
const DEMOTE_AFTER: u32 = 2;

impl Engine {
    pub fn new(manifest: Manifest) -> Self {
        let holds = vec![0; manifest.switch.len()];
        Self {
            manifest,
            holds,
            held_rule: None,
            current_entry: None,
            current_model: None,
            suspended: false,
            switches_this_turn: 0,
            turn: 0,
            failure_counts: BTreeMap::new(),
            router_entry: None,
            router_hold: 0,
            router_wanted: false,
        }
    }

    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    pub fn current_model(&self) -> Option<&str> {
        self.current_model.as_deref()
    }

    pub fn current_entry(&self) -> Option<&str> {
        self.current_entry.as_deref()
    }

    pub fn suspend_for_user(&mut self) {
        self.suspended = true;
    }

    pub fn resume(&mut self) {
        self.suspended = false;
    }

    pub fn suspended(&self) -> bool {
        self.suspended
    }

    pub fn begin_turn(&mut self, turn: u64) {
        self.turn = turn;
        self.switches_this_turn = 0;
    }

    pub fn evaluate<A: Access>(
        &mut self,
        signals: &Signals,
        resolver: &mut Resolver<'_, A>,
    ) -> Option<Selection> {
        if self.suspended {
            return None;
        }
        self.router_wanted = false;
        self.router_hold = self.router_hold.saturating_sub(1);
        let resolutions = self.resolve_all(resolver);

        let emergency = self.walk(signals, &resolutions, true, None);
        let choice = match emergency {
            Some((idx, _)) => Some((idx, Fired::Emergency(idx))),
            None => self
                .ordinary_walk(signals, &resolutions)
                .map(|idx| (idx, Fired::Rule(idx))),
        };

        let matched = choice.as_ref().map(|(idx, _)| *idx);
        for (idx, counter) in self.holds.iter_mut().enumerate() {
            if Some(idx) == matched {
                *counter = self.manifest.switch[idx].hold;
            } else {
                *counter = counter.saturating_sub(1);
            }
        }
        if let Some(idx) = matched {
            self.held_rule = Some(idx);
        }

        let (entry, fired) = match choice {
            Some((idx, fired)) => (self.manifest.switch[idx].use_entry.clone(), fired),
            None => match self.step_five(&resolutions) {
                Some(pick) => pick,
                None => (self.manifest.start_with.clone(), Fired::StartWith),
            },
        };

        self.router_wanted = self.manifest.router.enabled && fired == Fired::StartWith;
        self.apply(signals, resolver, &resolutions, entry, fired)
    }

    pub fn evaluate_emergency<A: Access>(
        &mut self,
        signals: &Signals,
        resolver: &mut Resolver<'_, A>,
    ) -> Option<Selection> {
        if self.suspended {
            return None;
        }
        let resolutions = self.resolve_all(resolver);
        let idx = self.walk(signals, &resolutions, true, None)?.0;
        let entry = self.manifest.switch[idx].use_entry.clone();
        self.holds[idx] = self.manifest.switch[idx].hold;
        self.held_rule = Some(idx);
        self.apply(
            signals,
            resolver,
            &resolutions,
            entry,
            Fired::Emergency(idx),
        )
    }

    /// True when the last evaluation fell through to `start-with` with the
    /// router enabled, so the host should consult it (spec 6.4).
    pub fn router_wanted(&self) -> bool {
        self.router_wanted
    }

    /// Applies a router pick as a switch like any other (spec 6.4). Refused
    /// unless the last evaluation asked for the router and the pick names a
    /// declared, resolvable entry.
    pub fn apply_router_pick<A: Access>(
        &mut self,
        entry: &str,
        signals: &Signals,
        resolver: &mut Resolver<'_, A>,
    ) -> Option<Selection> {
        if self.suspended || !self.router_wanted || !self.manifest.models.contains_key(entry) {
            return None;
        }
        let resolutions = self.resolve_all(resolver);
        resolutions.get(entry)?;
        self.router_wanted = false;
        // A pick that keeps the current entry is not a switch; holding it
        // would pin the session and stop the next turn from being judged.
        if self.current_entry.as_deref() != Some(entry) {
            self.router_entry = Some(entry.to_string());
            self.router_hold = crate::manifest::DEFAULT_HOLD;
        }
        self.apply(
            signals,
            resolver,
            &resolutions,
            entry.to_string(),
            Fired::Router,
        )
    }

    fn apply<A: Access>(
        &mut self,
        signals: &Signals,
        resolver: &mut Resolver<'_, A>,
        resolutions: &BTreeMap<String, Resolution>,
        entry: String,
        fired: Fired,
    ) -> Option<Selection> {
        let resolution = resolutions.get(&entry)?;
        let model = resolution.model.clone();

        if self.current_model.as_deref() == Some(model.as_str()) {
            self.current_entry = Some(entry.clone());
            return Some(Selection {
                entry,
                model,
                record: None,
            });
        }
        if self.switches_this_turn >= MAX_SWITCHES_PER_TURN {
            return self.current_selection();
        }

        self.note_failure_trigger(signals, resolver);

        let record = SwitchRecord {
            turn: self.turn,
            fired: fired.clone(),
            from_entry: self.current_entry.clone(),
            from_model: self.current_model.clone(),
            to_entry: entry.clone(),
            to_model: model.clone(),
            reason: self.reason(&fired, signals),
            skipped: resolution.skipped.clone(),
        };
        self.switches_this_turn += 1;
        self.current_entry = Some(entry.clone());
        self.current_model = Some(model.clone());
        Some(Selection {
            entry,
            model,
            record: Some(record),
        })
    }

    fn current_selection(&self) -> Option<Selection> {
        Some(Selection {
            entry: self.current_entry.clone()?,
            model: self.current_model.clone()?,
            record: None,
        })
    }

    fn note_failure_trigger<A: Access>(
        &mut self,
        signals: &Signals,
        resolver: &mut Resolver<'_, A>,
    ) {
        if !(signals.looping || signals.model_down || signals.failed_steps > 0) {
            return;
        }
        let (Some(entry), Some(model)) = (self.current_entry.clone(), self.current_model.clone())
        else {
            return;
        };
        let count = self
            .failure_counts
            .entry((entry, model.clone()))
            .or_insert(0);
        *count += 1;
        if *count >= DEMOTE_AFTER {
            resolver.demote(&model);
        }
    }

    fn resolve_all<A: Access>(&self, resolver: &Resolver<'_, A>) -> BTreeMap<String, Resolution> {
        self.manifest
            .models
            .iter()
            .filter_map(|(name, entry)| resolver.resolve(entry).map(|r| (name.clone(), r)))
            .collect()
    }

    fn walk(
        &self,
        signals: &Signals,
        resolutions: &BTreeMap<String, Resolution>,
        emergencies_only: bool,
        chat_full_percent: Option<f64>,
    ) -> Option<(usize, ())> {
        for (idx, rule) in self.manifest.switch.iter().enumerate() {
            let Some(resolution) = resolutions.get(&rule.use_entry) else {
                continue;
            };
            if resolution.window < signals.conversation_tokens {
                continue;
            }
            let matched = rule.when.iter().any(|c| {
                if emergencies_only && !c.is_emergency() {
                    return false;
                }
                if !emergencies_only && matches!(c, Condition::ChatFull(_)) {
                    let Some(percent) = chat_full_percent else {
                        return false;
                    };
                    let Condition::ChatFull(threshold) = c else {
                        return false;
                    };
                    return percent > *threshold;
                }
                self.condition_true(c, signals).unwrap_or(false)
            });
            if matched {
                return Some((idx, ()));
            }
        }
        None
    }

    fn ordinary_walk(
        &self,
        signals: &Signals,
        resolutions: &BTreeMap<String, Resolution>,
    ) -> Option<usize> {
        let tentative = self
            .walk(signals, resolutions, false, None)
            .map(|(idx, _)| self.manifest.switch[idx].use_entry.clone())
            .or_else(|| self.step_five(resolutions).map(|(entry, _)| entry))
            .unwrap_or_else(|| self.manifest.start_with.clone());
        let percent = resolutions
            .get(&tentative)
            .map(|r| signals.conversation_tokens as f64 / r.window as f64 * 100.0);
        self.walk(signals, resolutions, false, percent)
            .map(|(idx, _)| idx)
    }

    fn step_five(&self, resolutions: &BTreeMap<String, Resolution>) -> Option<(String, Fired)> {
        if let Some(idx) = self.held_rule
            && self.holds[idx] > 0
        {
            let entry = self.manifest.switch[idx].use_entry.clone();
            if resolutions.contains_key(&entry) {
                return Some((entry, Fired::Hold(idx)));
            }
        }
        if self.router_hold > 0
            && let Some(entry) = &self.router_entry
            && resolutions.contains_key(entry)
        {
            return Some((entry.clone(), Fired::Router));
        }
        resolutions
            .contains_key(&self.manifest.start_with)
            .then(|| (self.manifest.start_with.clone(), Fired::StartWith))
    }

    fn condition_true(&self, condition: &Condition, signals: &Signals) -> Option<bool> {
        match condition {
            Condition::Planning(None) => Some(signals.planning_mode.is_some()),
            Condition::Planning(Some(mode)) => Some(
                signals
                    .planning_mode
                    .as_deref()
                    .is_some_and(|m| m.eq_ignore_ascii_case(mode)),
            ),
            Condition::Stuck(threshold) => Some(signals.failed_steps >= *threshold),
            Condition::Looping => Some(signals.looping),
            Condition::ModelDown => Some(signals.model_down),
            Condition::ChatFull(_) => None,
            Condition::SpentOver(limit) => signals.spent_usd.map(|spent| spent > *limit),
            Condition::TokensOver(limit) => Some(signals.tokens_used > *limit),
            Condition::TurnOver(limit) => Some(signals.user_turns > *limit),
            Condition::Extension(key, param) => {
                let with_param = param.as_str().map(|p| format!("{key}={p}"));
                Some(
                    signals.x_active.contains(key)
                        || with_param.is_some_and(|k| signals.x_active.contains(&k)),
                )
            }
            Condition::Inert(_) => None,
        }
    }

    fn reason(&self, fired: &Fired, signals: &Signals) -> String {
        match fired {
            Fired::Emergency(idx) | Fired::Rule(idx) => {
                let rule = &self.manifest.switch[*idx];
                let cause = rule
                    .when
                    .iter()
                    .filter(|c| !matches!(fired, Fired::Emergency(_)) || c.is_emergency())
                    .find(|c| self.condition_true(c, signals).unwrap_or(false))
                    .or_else(|| rule.when.first());
                match cause {
                    Some(Condition::Planning(_)) => "the tool entered plan mode".to_string(),
                    Some(Condition::Stuck(n)) => {
                        format!("stuck: {n} failed tool steps in a row")
                    }
                    Some(Condition::Looping) => {
                        "the model was repeating itself or producing empty output".to_string()
                    }
                    Some(Condition::ModelDown) => {
                        "the model is unavailable or erroring".to_string()
                    }
                    Some(Condition::ChatFull(p)) => {
                        format!("the conversation passed {p}% of the context window")
                    }
                    Some(Condition::SpentOver(d)) => format!("session cost passed ${d}"),
                    Some(Condition::TokensOver(t)) => format!("session passed {t} tokens"),
                    Some(Condition::TurnOver(t)) => format!("session passed {t} turns"),
                    Some(Condition::Extension(key, _)) => format!("condition '{key}' held"),
                    _ => format!("rule {} matched", idx + 1),
                }
            }
            Fired::Hold(idx) => format!("held by rule {}", idx + 1),
            Fired::Router => "picked for this kind of task".to_string(),
            Fired::StartWith => "no rule matched; start-with".to_string(),
            Fired::User => "the user picked a model".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::Catalog;
    use crate::manifest::parse;

    fn all_access(_: &str) -> bool {
        true
    }

    const ABSTRACT_FILE: &str = r#"
mom: "0.1"
models:
  everyday:
    power: medium
  thinker:
    power: max
    thinking: deep
start-with: everyday
switch:
  - when: planning
    use: thinker
  - when: stuck
    use: thinker
  - when: { spent-over: 5 }
    use: everyday
"#;

    fn engine(text: &str) -> Engine {
        Engine::new(parse(text).unwrap())
    }

    fn quiet() -> Signals {
        Signals::default()
    }

    #[test]
    fn trace_abstract_file_matches_spec() {
        let catalog = Catalog::builtin();
        let mut resolver = Resolver::new(&catalog, all_access, false);
        let mut engine = engine(ABSTRACT_FILE);

        // Turn 1: nothing special -> start-with.
        engine.begin_turn(1);
        let s = engine.evaluate(&quiet(), &mut resolver).unwrap();
        assert_eq!(s.entry, "everyday");
        assert_eq!(s.record.as_ref().map(|r| &r.fired), Some(&Fired::StartWith));
        let everyday_model = s.model.clone();

        // Turn 2: plan mode -> planning matches, hold set.
        engine.begin_turn(2);
        let signals = Signals {
            planning_mode: Some("plan".into()),
            ..quiet()
        };
        let s = engine.evaluate(&signals, &mut resolver).unwrap();
        assert_eq!(s.entry, "thinker");
        assert_eq!(s.record.as_ref().map(|r| &r.fired), Some(&Fired::Rule(0)));

        // Turns 3-4: no match, thinker held.
        for turn in 3..=4 {
            engine.begin_turn(turn);
            let s = engine.evaluate(&quiet(), &mut resolver).unwrap();
            assert_eq!(s.entry, "thinker", "turn {turn} should stay held");
            assert!(s.record.is_none());
        }

        // Turn 5: hold expired -> everyday.
        engine.begin_turn(5);
        let s = engine.evaluate(&quiet(), &mut resolver).unwrap();
        assert_eq!(s.entry, "everyday");
        assert_eq!(s.model, everyday_model);

        // Turn 9: three failed steps -> stuck matches.
        engine.begin_turn(9);
        let signals = Signals {
            failed_steps: 3,
            ..quiet()
        };
        let s = engine.evaluate(&signals, &mut resolver).unwrap();
        assert_eq!(s.entry, "thinker");
        assert_eq!(s.record.as_ref().map(|r| &r.fired), Some(&Fired::Rule(1)));

        // Turn 12: recovered, hold expired -> everyday.
        for turn in 10..=12 {
            engine.begin_turn(turn);
            engine.evaluate(&quiet(), &mut resolver).unwrap();
        }
        assert_eq!(engine.current_entry(), Some("everyday"));

        // Turn 20: $5.20 spent -> spending rule matches, already there.
        engine.begin_turn(20);
        let spent = Signals {
            spent_usd: Some(5.2),
            ..quiet()
        };
        let s = engine.evaluate(&spent, &mut resolver).unwrap();
        assert_eq!(s.entry, "everyday");
        assert!(s.record.is_none());

        // Turn 21: stuck matches first; order decides.
        engine.begin_turn(21);
        let signals = Signals {
            failed_steps: 3,
            spent_usd: Some(5.2),
            ..quiet()
        };
        let s = engine.evaluate(&signals, &mut resolver).unwrap();
        assert_eq!(s.entry, "thinker");
    }

    const SPEND_FIRST: &str = r#"
mom: "0.1"
models:
  everyday:
    power: medium
  thinker:
    power: max
    thinking: deep
start-with: everyday
switch:
  - when: { spent-over: 5 }
    use: everyday
  - when: stuck
    use: thinker
  - when: looping
    use: thinker
"#;

    #[test]
    fn trace_spending_rule_first_matches_spec() {
        let catalog = Catalog::builtin();
        let mut resolver = Resolver::new(&catalog, all_access, false);
        let mut engine = engine(SPEND_FIRST);

        engine.begin_turn(20);
        let spent = Signals {
            spent_usd: Some(5.2),
            ..quiet()
        };
        let s = engine.evaluate(&spent, &mut resolver).unwrap();
        assert_eq!(s.entry, "everyday");

        // Turn 21: stuck also matches, but the spending rule is first.
        engine.begin_turn(21);
        let signals = Signals {
            failed_steps: 3,
            spent_usd: Some(5.2),
            ..quiet()
        };
        let s = engine.evaluate(&signals, &mut resolver).unwrap();
        assert_eq!(s.entry, "everyday", "order decides for non-emergencies");

        // Turn 22: looping is an emergency, walked first.
        engine.begin_turn(22);
        let signals = Signals {
            looping: true,
            spent_usd: Some(5.2),
            ..quiet()
        };
        let s = engine.evaluate(&signals, &mut resolver).unwrap();
        assert_eq!(s.entry, "thinker", "emergency beats budget");
        assert!(matches!(s.record.unwrap().fired, Fired::Emergency(2)));
    }

    #[test]
    fn evaluate_suspended_returns_none() {
        let catalog = Catalog::builtin();
        let mut resolver = Resolver::new(&catalog, all_access, false);
        let mut engine = engine(ABSTRACT_FILE);
        engine.suspend_for_user();
        assert!(engine.evaluate(&quiet(), &mut resolver).is_none());
    }

    #[test]
    fn evaluate_switch_budget_caps_at_two_per_turn() {
        let catalog = Catalog::builtin();
        let mut resolver = Resolver::new(&catalog, all_access, false);
        let mut engine = engine(ABSTRACT_FILE);
        engine.begin_turn(1);
        engine.evaluate(&quiet(), &mut resolver).unwrap();
        let planning = Signals {
            planning_mode: Some("plan".into()),
            ..quiet()
        };
        engine.evaluate(&planning, &mut resolver).unwrap();
        // Third change within the same turn is refused; the choice stands.
        let stuck = Signals {
            failed_steps: 3,
            ..quiet()
        };
        let s = engine.evaluate(&stuck, &mut resolver).unwrap();
        assert!(s.record.is_none());
    }

    #[test]
    fn evaluate_emergency_only_runs_emergency_rules() {
        let catalog = Catalog::builtin();
        let mut resolver = Resolver::new(&catalog, all_access, false);
        let mut engine = engine(SPEND_FIRST);
        engine.begin_turn(1);
        engine.evaluate(&quiet(), &mut resolver).unwrap();
        // A stuck signal mid-turn is not an emergency; nothing switches.
        let stuck = Signals {
            failed_steps: 3,
            ..quiet()
        };
        assert!(engine.evaluate_emergency(&stuck, &mut resolver).is_none());
        // Looping mid-turn is.
        let looping = Signals {
            looping: true,
            ..quiet()
        };
        let s = engine.evaluate_emergency(&looping, &mut resolver).unwrap();
        assert_eq!(s.entry, "thinker");
    }

    #[test]
    fn evaluate_demotes_after_two_failure_switches() {
        let catalog = Catalog::builtin();
        let mut resolver = Resolver::new(&catalog, all_access, false);
        let mut engine = engine(ABSTRACT_FILE);
        engine.begin_turn(1);
        let first = engine.evaluate(&quiet(), &mut resolver).unwrap();
        let everyday_model = first.model;

        // Two stuck switches away from the same everyday model.
        for turn in [2u64, 10] {
            engine.begin_turn(turn);
            let stuck = Signals {
                failed_steps: 3,
                ..quiet()
            };
            engine.evaluate(&stuck, &mut resolver).unwrap();
            // Fall back to everyday by letting the hold expire.
            for t in turn + 1..turn + 5 {
                engine.begin_turn(t);
                engine.evaluate(&quiet(), &mut resolver).unwrap();
            }
        }
        assert!(resolver.demotions().any(|m| m == everyday_model));
    }

    const ROUTED_FILE: &str = r#"
mom: "0.1"
models:
  everyday:
    power: medium
  thinker:
    power: max
    thinking: deep
start-with: everyday
router:
  enabled: true
  power: low
switch:
  - when: planning
    use: thinker
"#;

    #[test]
    fn router_wanted_only_when_no_rule_or_hold_applies() {
        let catalog = Catalog::builtin();
        let mut resolver = Resolver::new(&catalog, all_access, false);
        let mut engine = engine(ROUTED_FILE);

        engine.begin_turn(1);
        engine.evaluate(&quiet(), &mut resolver).unwrap();
        assert!(engine.router_wanted(), "start-with fallback consults it");

        engine.begin_turn(2);
        let planning = Signals {
            planning_mode: Some("plan".into()),
            ..quiet()
        };
        engine.evaluate(&planning, &mut resolver).unwrap();
        assert!(
            !engine.router_wanted(),
            "a matched rule is never overridden"
        );

        engine.begin_turn(3);
        engine.evaluate(&quiet(), &mut resolver).unwrap();
        assert!(!engine.router_wanted(), "a held rule beats the router");
    }

    #[test]
    fn router_pick_switches_and_holds() {
        let catalog = Catalog::builtin();
        let mut resolver = Resolver::new(&catalog, all_access, false);
        let mut engine = engine(ROUTED_FILE);

        engine.begin_turn(1);
        engine.evaluate(&quiet(), &mut resolver).unwrap();
        let s = engine
            .apply_router_pick("thinker", &quiet(), &mut resolver)
            .unwrap();
        assert_eq!(s.entry, "thinker");
        assert!(matches!(s.record.unwrap().fired, Fired::Router));

        // The pick holds like a rule switch; no re-consultation meanwhile.
        for turn in 2..=3 {
            engine.begin_turn(turn);
            let s = engine.evaluate(&quiet(), &mut resolver).unwrap();
            assert_eq!(s.entry, "thinker", "turn {turn} should stay held");
            assert!(!engine.router_wanted());
        }

        // Hold expired: back to start-with, router consulted again.
        engine.begin_turn(4);
        let s = engine.evaluate(&quiet(), &mut resolver).unwrap();
        assert_eq!(s.entry, "everyday");
        assert!(engine.router_wanted());
    }

    #[test]
    fn router_pick_rejects_undeclared_entry_and_stale_calls() {
        let catalog = Catalog::builtin();
        let mut resolver = Resolver::new(&catalog, all_access, false);
        let mut engine = engine(ROUTED_FILE);

        engine.begin_turn(1);
        engine.evaluate(&quiet(), &mut resolver).unwrap();
        assert!(
            engine
                .apply_router_pick("nonsense", &quiet(), &mut resolver)
                .is_none()
        );

        // A pick landing after a rule matched is refused.
        engine.begin_turn(2);
        let planning = Signals {
            planning_mode: Some("plan".into()),
            ..quiet()
        };
        engine.evaluate(&planning, &mut resolver).unwrap();
        assert!(
            engine
                .apply_router_pick("everyday", &quiet(), &mut resolver)
                .is_none()
        );
    }

    #[test]
    fn chat_full_measures_against_would_be_pick() {
        let catalog = Catalog::builtin();
        let mut resolver = Resolver::new(&catalog, all_access, false);
        let mut engine = engine(
            r#"
mom: "0.1"
models:
  small:
    power: low
  marathon:
    power: medium
    memory: vast
start-with: small
switch:
  - when: { chat-full: 70 }
    use: marathon
"#,
        );
        engine.begin_turn(1);
        let s = engine.evaluate(&quiet(), &mut resolver).unwrap();
        let small_window = catalog.find(&s.model).unwrap().window;

        // Fill past 70% of the small model's window: rule matches.
        engine.begin_turn(2);
        let full = Signals {
            conversation_tokens: small_window * 8 / 10,
            ..quiet()
        };
        let s = engine.evaluate(&full, &mut resolver).unwrap();
        assert_eq!(s.entry, "marathon");

        // Still full next turn: the counterfactual pick is still small, so
        // the rule keeps matching and the session stays on marathon.
        engine.begin_turn(3);
        let s = engine.evaluate(&full, &mut resolver).unwrap();
        assert_eq!(s.entry, "marathon", "no flap back to the small model");
    }
}
