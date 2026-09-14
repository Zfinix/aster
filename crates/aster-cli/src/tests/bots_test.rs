use super::*;

use tempfile::TempDir;

/// The botmigrate fixture, which is the share JSON schema in full.
const SHARE: &str = r#"{
  "profile": {
    "name": "Research Bot",
    "description": "Autonomous research assistant that watches arXiv and writes briefs.",
    "title": "Research specialist",
    "avatarShape": "blob",
    "avatarColor": "cyan"
  },
  "memory": [
    { "kind": "profile", "createdAt": "2026-03-01", "content": "Prefer primary sources over summaries." }
  ],
  "skills": [
    {
      "name": "arxiv-brief",
      "description": "Summarize an arXiv paper into a one-page brief.",
      "content": "Read the paper. Extract problem, method, results, and caveats."
    }
  ],
  "routines": [
    {
      "slug": "weekly-digest",
      "name": "Weekly digest",
      "description": "Runs on cron schedule 0 9 * * 1.",
      "content": "Summarize new arXiv papers from the last week."
    }
  ],
  "plugins": [
    { "pluginId": "github", "name": "GitHub", "description": "Read repositories and issues." }
  ],
  "gettingStarted": { "skill": "arxiv-brief" },
  "visibility": "public"
}"#;

fn read() -> ir::BotIr {
    grok::read(SHARE, "fixture.json").unwrap()
}

#[test]
fn reads_the_share_fixture() {
    let ir = read();
    assert_eq!(ir.identity.name, "Research Bot");
    assert_eq!(ir.skills.len(), 1);
    assert_eq!(ir.skills[0].name, "arxiv-brief");
    assert_eq!(ir.requirements.len(), 1);
    assert_eq!(ir.requirements[0].id, "github");
    assert_eq!(ir.notes.len(), 1);
    assert!(ir.missing.is_empty());
}

/// A share JSON has no schedule field: the cron is prose.
#[test]
fn recovers_the_cron_from_prose() {
    let ir = read();
    let found: Vec<_> = ir.cron_routines().collect();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].1, "0 9 * * 1");
    assert_eq!(found[0].0.name, "weekly-digest");
}

/// Every recovered cron must be one aster-cron can install, so validation is
/// the extractor rather than a pattern that merely looks cron-shaped.
#[test]
fn ignores_prose_that_is_not_a_cron() {
    let share = SHARE.replace("Runs on cron schedule 0 9 * * 1.", "Runs when I ask it to.");
    let ir = grok::read(&share, "fixture.json").unwrap();
    assert_eq!(ir.cron_routines().count(), 0);
    assert_eq!(ir.non_cron_routines().count(), 1);
}

/// Event listeners are not crons. Inventing one would misreport when it runs.
#[test]
fn event_triggers_are_not_turned_into_cron() {
    let share = SHARE.replace(
        "Runs on cron schedule 0 9 * * 1.",
        "Fires on a github webhook.",
    );
    let ir = grok::read(&share, "fixture.json").unwrap();
    assert_eq!(ir.cron_routines().count(), 0);
    let routine = ir.non_cron_routines().next().unwrap();
    assert!(matches!(&routine.trigger, ir::Trigger::Event { name } if name == "github"));

    let checked = check::check(&ir, &Default::default());
    assert!(
        checked
            .iter()
            .any(|c| c.status == Status::Unsupported && c.id.contains("weekly-digest"))
    );
}

/// A description standing in for an instruction body is a summary. Report the
/// gap rather than installing something that cannot be followed.
#[test]
fn a_skill_without_a_body_is_reported_not_invented() {
    let share = SHARE.replace(
        "\"content\": \"Read the paper. Extract problem, method, results, and caveats.\"",
        "\"content\": \"\"",
    );
    let ir = grok::read(&share, "fixture.json").unwrap();
    assert!(ir.skills.is_empty());
    assert_eq!(ir.missing.len(), 1);
    assert!(ir.missing[0].contains("arxiv-brief"));
}

#[test]
fn installs_a_discoverable_agent_with_scoped_skills() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("bots");
    let ir = read();
    let written = write::install(&root, &ir, SHARE, &default_tools(), false).unwrap();

    assert_eq!(written.name, "research-bot");
    assert_eq!(written.skills, 1);
    assert!(written.dir.join("bot.json").is_file());
    assert!(written.dir.join("source/share.json").is_file());

    // The agent is discovered from the bots root, and its skills are bound to
    // the package rather than to any skills root the user owns.
    let registry = aster_agents::AgentRegistry::discover_all(&[], std::slice::from_ref(&root));
    let agent = registry.get("research-bot").unwrap();
    assert_eq!(agent.skills_root, Some(written.dir.join("skills")));
    assert!(agent.load_body().unwrap().contains("Autonomous research"));

    let scoped = aster_skills::SkillSet::discover(&[written.dir.join("skills")]);
    assert_eq!(scoped.len(), 1);
    assert!(scoped.get("arxiv-brief").is_some());
}

/// An agent you wrote outranks a bot's agent of the same name, so installing a
/// bot can never replace something you authored.
#[test]
fn a_hand_written_agent_shadows_a_bot() {
    let tmp = TempDir::new().unwrap();
    let bots = tmp.path().join("bots");
    let agents = tmp.path().join("agents");
    write::install(&bots, &read(), SHARE, &default_tools(), false).unwrap();

    let mine = agents.join("research-bot");
    std::fs::create_dir_all(&mine).unwrap();
    std::fs::write(
        mine.join("AGENT.md"),
        "---\nname: research-bot\ndescription: Mine, not theirs.\n---\nMine.\n",
    )
    .unwrap();

    let registry = aster_agents::AgentRegistry::discover_all(&[agents], &[bots]);
    let agent = registry.get("research-bot").unwrap();
    assert_eq!(agent.description, "Mine, not theirs.");
    assert!(agent.skills_root.is_none());
}

#[test]
fn refuses_to_overwrite_without_force() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("bots");
    let ir = read();
    write::install(&root, &ir, SHARE, &default_tools(), false).unwrap();
    assert!(write::install(&root, &ir, SHARE, &default_tools(), false).is_err());
    assert!(write::install(&root, &ir, SHARE, &default_tools(), true).is_ok());
}

/// Publisher notes are their preferences and their machine's layout. They are
/// recorded as theirs and never merged into the user's memory.
#[test]
fn notes_stay_in_the_record() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("bots");
    let written = write::install(&root, &read(), SHARE, &default_tools(), false).unwrap();
    let record = BotRecord::load(&written.dir).unwrap();
    assert_eq!(record.bot.notes.len(), 1);
    assert!(record.bot.notes[0].content.contains("primary sources"));
    let agent = std::fs::read_to_string(written.dir.join("AGENT.md")).unwrap();
    assert!(!agent.contains("primary sources"));
}

#[test]
fn a_requirement_with_no_provider_needs_setup() {
    let ir = read();
    let checked = check::check(&ir, &Default::default());
    let github = checked.iter().find(|c| c.id == "github").unwrap();
    assert_eq!(github.status, Status::NeedsSetup);
}

#[test]
fn slugs_are_directory_safe() {
    assert_eq!(ir::slug("Research Bot"), "research-bot");
    assert_eq!(ir::slug("  Weird///Name!! "), "weird-name");
    assert_eq!(ir::slug("../etc"), "etc");
    assert!(ir::slug("!!!").is_empty());
}
