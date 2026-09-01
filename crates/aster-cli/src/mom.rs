use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::Result;
use aster_mom::{Catalog, Engine, ModelEntry, Power, Resolver, Selection, Signals, Thinking};
use clap::{Args, Subcommand};

/// Everything needed to reach one resolved model from a turn task.
#[derive(Debug, Clone)]
pub struct RouterTarget {
    pub base_url: String,
    pub key: String,
    pub model_param: String,
}

/// A snapshot for one router consultation (spec 6.4): the picking model,
/// the declared entries with descriptions, and where each pick would land.
#[derive(Debug, Clone)]
pub struct RouterPlan {
    pub router: RouterTarget,
    pub entries: Vec<(String, String)>,
    pub targets: BTreeMap<String, RouterTarget>,
}

pub struct MomSession {
    engine: Engine,
    catalog: Catalog,
    demoted: BTreeSet<String>,
    provider_urls: BTreeMap<String, String>,
    openrouter_configured: bool,
    aggregator: Option<(String, String)>,
}

fn accessible_with(
    provider_urls: &BTreeMap<String, String>,
    openrouter_configured: bool,
    model_id: &str,
) -> bool {
    if openrouter_configured {
        return true;
    }
    let prefix = model_id.split('/').next().unwrap_or_default();
    provider_urls
        .get(provider_id(prefix))
        .is_some_and(|url| aster_ai::keys::resolve_key(url).is_some())
}

fn provider_id(prefix: &str) -> &str {
    match prefix {
        "google" => "google_gemini",
        "alibaba" => "dashscope",
        other => other,
    }
}

fn openrouter_slug(model_id: &str) -> String {
    let Some((prefix, rest)) = model_id.split_once('/') else {
        return model_id.to_string();
    };
    let mapped = match prefix {
        "zai" => "z-ai",
        "xai" => "x-ai",
        "alibaba" => "qwen",
        other => other,
    };
    format!("{mapped}/{rest}")
}

impl MomSession {
    pub fn load(repo_root: &Path) -> Option<Self> {
        let home = dirs::home_dir().map(|h| h.join(".aster"));
        let path = aster_mom::discover(Some(repo_root), home.as_deref())?;
        let manifest = match aster_mom::load(&path) {
            Ok(m) => m,
            Err(e) => {
                eprintln!(
                    "mom.yaml at {} is invalid and was ignored: {e:#}",
                    path.display()
                );
                return None;
            }
        };
        for warning in &manifest.warnings {
            eprintln!("mom.yaml: {warning}");
        }
        let provider_urls: BTreeMap<String, String> =
            crate::init::provider_base_urls().into_iter().collect();
        let openrouter_configured = provider_urls
            .get("openrouter")
            .is_some_and(|url| aster_ai::keys::resolve_key(url).is_some());
        let session_url = std::env::var("ASTER_BASE_URL").ok().or_else(|| {
            crate::settings::Settings::load(Some(repo_root))
                .ok()
                .and_then(|s| s.review.base_url)
        });
        let aggregator = session_url
            .filter(|url| {
                let want = url.trim_end_matches('/');
                let openrouter = provider_urls
                    .get("openrouter")
                    .map(|u| u.trim_end_matches('/'));
                let known = provider_urls
                    .values()
                    .any(|u| u.trim_end_matches('/') == want);
                !known || Some(want) == openrouter
            })
            .and_then(|url| aster_ai::keys::resolve_key(&url).map(|(key, _)| (url, key)));
        Some(Self {
            engine: Engine::new(manifest),
            catalog: Catalog::builtin(),
            demoted: BTreeSet::new(),
            provider_urls,
            openrouter_configured,
            aggregator,
        })
    }

    fn accessible(&self, model_id: &str) -> bool {
        self.aggregator.is_some()
            || accessible_with(&self.provider_urls, self.openrouter_configured, model_id)
    }

    pub fn evaluate_turn(&mut self, turn: u64, signals: &Signals) -> Option<Selection> {
        self.engine.begin_turn(turn);
        self.run(signals, false)
    }

    pub fn evaluate_again(&mut self, signals: &Signals) -> Option<Selection> {
        self.run(signals, false)
    }

    fn run(&mut self, signals: &Signals, emergency: bool) -> Option<Selection> {
        let provider_urls = &self.provider_urls;
        let openrouter = self.openrouter_configured;
        let aggregator = self.aggregator.is_some();
        let accessible =
            move |id: &str| aggregator || accessible_with(provider_urls, openrouter, id);
        let mut resolver = Resolver::new(&self.catalog, accessible, true);
        for model in &self.demoted {
            resolver.demote(model);
        }
        let selection = if emergency {
            self.engine.evaluate_emergency(signals, &mut resolver)
        } else {
            self.engine.evaluate(signals, &mut resolver)
        };
        self.demoted = resolver.demotions().map(str::to_string).collect();
        selection
    }

    pub fn router_wanted(&self) -> bool {
        self.engine.router_wanted()
    }

    /// Resolves the router model and every declared entry to concrete
    /// endpoints, or None when the router is disabled or under-resolved.
    pub fn router_plan(&self) -> Option<RouterPlan> {
        let manifest = self.engine.manifest();
        if !manifest.router.enabled {
            return None;
        }
        let accessible = |id: &str| self.accessible(id);
        let mut resolver = Resolver::new(&self.catalog, &accessible, false);
        for model in &self.demoted {
            resolver.demote(model);
        }
        let picker = ModelEntry {
            power: manifest.router.power,
            prefer: manifest.router.prefer.clone(),
            ..ModelEntry::default()
        };
        let router = self.target_for(&resolver.resolve(&picker)?.model)?;

        let mut entry_resolver = Resolver::new(&self.catalog, &accessible, true);
        for model in &self.demoted {
            entry_resolver.demote(model);
        }
        let mut entries = Vec::new();
        let mut targets = BTreeMap::new();
        for (name, entry) in &manifest.models {
            let Some(resolution) = entry_resolver.resolve(entry) else {
                continue;
            };
            let Some(target) = self.target_for(&resolution.model) else {
                continue;
            };
            entries.push((name.clone(), describe(entry)));
            targets.insert(name.clone(), target);
        }
        if entries.len() < 2 {
            return None;
        }
        Some(RouterPlan {
            router,
            entries,
            targets,
        })
    }

    /// Records a router pick as a switch, with the same resolver setup the
    /// ordinary evaluation uses.
    pub fn apply_router(&mut self, entry: &str, signals: &Signals) -> Option<Selection> {
        let provider_urls = &self.provider_urls;
        let openrouter = self.openrouter_configured;
        let aggregator = self.aggregator.is_some();
        let accessible =
            move |id: &str| aggregator || accessible_with(provider_urls, openrouter, id);
        let mut resolver = Resolver::new(&self.catalog, accessible, true);
        for model in &self.demoted {
            resolver.demote(model);
        }
        let selection = self.engine.apply_router_pick(entry, signals, &mut resolver);
        self.demoted = resolver.demotions().map(str::to_string).collect();
        selection
    }

    fn target_for(&self, model_id: &str) -> Option<RouterTarget> {
        let (base_url, key) = self.endpoint_for(model_id)?;
        let model_param = self.model_param(&base_url, model_id);
        Some(RouterTarget {
            base_url,
            key,
            model_param,
        })
    }

    pub fn suspend_for_user(&mut self) {
        self.engine.suspend_for_user();
    }

    pub fn suspended(&self) -> bool {
        self.engine.suspended()
    }

    pub fn endpoint_for(&self, model_id: &str) -> Option<(String, String)> {
        if let Some((url, key)) = &self.aggregator {
            return Some((url.clone(), key.clone()));
        }
        let prefix = model_id.split('/').next().unwrap_or_default();
        let native = self
            .provider_urls
            .get(provider_id(prefix))
            .and_then(|url| aster_ai::keys::resolve_key(url).map(|(key, _)| (url.clone(), key)));
        native.or_else(|| {
            let url = self.provider_urls.get("openrouter")?;
            aster_ai::keys::resolve_key(url).map(|(key, _)| (url.clone(), key))
        })
    }

    pub fn model_param(&self, base_url: &str, model_id: &str) -> String {
        if base_url.contains("openrouter") {
            return openrouter_slug(model_id);
        }
        let is_aggregator = self
            .aggregator
            .as_ref()
            .is_some_and(|(url, _)| url.trim_end_matches('/') == base_url.trim_end_matches('/'));
        if is_aggregator {
            model_id.to_string()
        } else {
            model_id.split('/').nth(1).unwrap_or(model_id).to_string()
        }
    }
}

pub struct MomOverview {
    pub name: Option<String>,
    pub path: Option<std::path::PathBuf>,
    pub suspended: bool,
    pub current: Option<(String, String)>,
    pub entries: Vec<(String, Option<String>)>,
    pub rules: usize,
}

impl MomSession {
    pub fn resume(&mut self) {
        self.engine.resume();
    }

    pub fn overview(&self) -> MomOverview {
        let manifest = self.engine.manifest();
        let accessible = |id: &str| self.accessible(id);
        let mut resolver = Resolver::new(&self.catalog, &accessible, true);
        for model in &self.demoted {
            resolver.demote(model);
        }
        let entries = manifest
            .models
            .iter()
            .map(|(name, entry)| (name.clone(), resolver.resolve(entry).map(|r| r.model)))
            .collect();
        let current = self
            .engine
            .current_entry()
            .zip(self.engine.current_model())
            .map(|(e, m)| (e.to_string(), m.to_string()));
        MomOverview {
            name: manifest.name.clone(),
            path: manifest.path.clone(),
            suspended: self.engine.suspended(),
            current,
            entries,
            rules: manifest.switch.len(),
        }
    }
}

fn describe(entry: &ModelEntry) -> String {
    if let Some(description) = &entry.description {
        return description.clone();
    }
    let power = match entry.power {
        Power::Low => "a light model for trivial asks",
        Power::Medium => "an everyday model for routine coding work",
        Power::Max => "the strongest model, for hard or open-ended problems",
    };
    match entry.thinking {
        Thinking::Some | Thinking::Deep => format!("{power}; reasons before answering"),
        Thinking::None => power.to_string(),
    }
}

/// Asks the router model which entry fits this message (spec 6.4). The
/// input is the entry list and the message text, nothing more; any error,
/// timeout, or malformed reply returns None so the caller keeps start-with.
pub async fn consult_router(
    client: &aster_ai::AiClient,
    plan: &RouterPlan,
    message: &str,
) -> Option<String> {
    let mut client = client.clone();
    if client.base_url().trim_end_matches('/') != plan.router.base_url.trim_end_matches('/') {
        client.set_endpoint(&plan.router.base_url, plan.router.key.clone());
    }
    let mut system = String::from(
        "You route one user message to the model entry best suited to answer it. \
         Weigh how hard the message is: multi-step design, debugging, or \
         refactoring work goes to a stronger entry; small mechanical asks go to \
         a cheaper one. Reply with only the JSON {\"use\":\"<entry>\"} naming \
         one entry.\nEntries:\n",
    );
    for (name, desc) in &plan.entries {
        system.push_str(&format!("- {name}: {desc}\n"));
    }
    let reply = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        client.complete_with(&plan.router.model_param, &system, message, 0.0),
    )
    .await;
    let (pick, outcome) = match &reply {
        Err(_) => (None, "timed out".to_string()),
        Ok(Err(e)) => (None, format!("call failed: {e:#}")),
        Ok(Ok(text)) => match parse_pick(text, plan) {
            Some(entry) => (Some(entry.clone()), format!("picked {entry}")),
            None => (None, format!("unusable reply: {}", text.trim())),
        },
    };
    log_router(&plan.router.model_param, &outcome);
    pick
}

fn log_router(router_model: &str, outcome: &str) {
    let Some(dir) = dirs::home_dir().map(|h| h.join(".aster/logs")) else {
        return;
    };
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let line = serde_json::json!({
        "at": chrono::Utc::now().to_rfc3339(),
        "router": router_model,
        "outcome": outcome,
    });
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("mom-router.jsonl"))
    {
        let _ = writeln!(f, "{line}");
    }
}

fn parse_pick(reply: &str, plan: &RouterPlan) -> Option<String> {
    let start = reply.find('{')?;
    let end = reply.rfind('}')?;
    let value: serde_json::Value = serde_json::from_str(&reply[start..=end]).ok()?;
    let pick = value.get("use")?.as_str()?;
    plan.targets.contains_key(pick).then(|| pick.to_string())
}

pub fn log_switch(record: &aster_mom::SwitchRecord) {
    let Some(dir) = dirs::home_dir().map(|h| h.join(".aster/logs")) else {
        return;
    };
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let line = serde_json::json!({
        "at": chrono::Utc::now().to_rfc3339(),
        "turn": record.turn,
        "fired": format!("{:?}", record.fired),
        "from_entry": record.from_entry,
        "from_model": record.from_model,
        "to_entry": record.to_entry,
        "to_model": record.to_model,
        "reason": record.reason,
        "skipped": record.skipped,
    });
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("mom-switches.jsonl"))
    {
        let _ = writeln!(f, "{line}");
    }
}

#[derive(Args)]
pub struct MomArgs {
    #[command(subcommand)]
    cmd: MomCmd,
}

#[derive(Subcommand)]
enum MomCmd {
    Check,
    /// Ask the router which entry it would pick for a message.
    Route {
        message: Vec<String>,
    },
}

pub async fn run_mom(args: MomArgs) -> Result<()> {
    match args.cmd {
        MomCmd::Check => check(),
        MomCmd::Route { message } => route(&message.join(" ")).await,
    }
}

async fn route(message: &str) -> Result<()> {
    if message.trim().is_empty() {
        anyhow::bail!("give me a message to route, e.g. aster mom route fix this typo");
    }
    let repo_root = std::env::current_dir().unwrap_or_default();
    let Some(session) = MomSession::load(&repo_root) else {
        println!("no mom.yaml found (looked in the project root, .agents/, and ~/.aster)");
        return Ok(());
    };
    let Some(plan) = session.router_plan() else {
        println!("the router is off or under-resolved; enable it under `router:` in mom.yaml");
        return Ok(());
    };
    let (base_url, key) = (plan.router.base_url.clone(), plan.router.key.clone());
    let client = aster_ai::AiClient::new(base_url, key, plan.router.model_param.clone());
    match consult_router(&client, &plan, message).await {
        Some(entry) => {
            let model = plan
                .targets
                .get(&entry)
                .map(|t| t.model_param.as_str())
                .unwrap_or("?");
            println!("{entry} ({model})");
        }
        None => println!(
            "no pick; the session would stay on start-with (details in ~/.aster/logs/mom-router.jsonl)"
        ),
    }
    Ok(())
}

fn check() -> Result<()> {
    let repo_root = std::env::current_dir().unwrap_or_default();
    let Some(session) = MomSession::load(&repo_root) else {
        println!("no mom.yaml found (looked in the project root, .agents/, and ~/.aster)");
        return Ok(());
    };
    let manifest = session.engine.manifest().clone();
    if let Some(path) = &manifest.path {
        println!("manifest: {}", path.display());
    }
    if let Some(name) = &manifest.name {
        println!("name: {name}");
    }
    println!("start-with: {}", manifest.start_with);
    if manifest.router.enabled {
        println!("router: on · a cheap model picks the entry when no rule matches");
    }
    println!();

    let accessible = |id: &str| session.accessible(id);
    let resolver = Resolver::new(&session.catalog, &accessible, true);
    for (name, entry) in &manifest.models {
        match resolver.resolve(entry) {
            Some(r) => {
                println!("  {name} -> {}", r.model);
                for skip in &r.skipped {
                    println!("      skipped {skip}");
                }
            }
            None => println!("  {name} -> unresolvable: no accessible model satisfies it"),
        }
    }
    println!();
    if manifest.switch.is_empty() {
        println!(
            "no switch rules; {} runs the whole session",
            manifest.start_with
        );
    } else {
        println!(
            "{} switch rule(s), checked in order before every turn",
            manifest.switch.len()
        );
    }
    Ok(())
}
