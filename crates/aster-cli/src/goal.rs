//! `/goal`: keep the turn loop running until a judge model deems a condition
//! met. The judge is a separate, cheap model, so the worker never certifies
//! its own completion.

use aster_ai::{AiClient, ChatMessage};

/// Turn cap for one goal run, so a stuck loop dies instead of spending forever.
const DEFAULT_MAX_TURNS: usize = 20;

/// What the judge decided about the condition after one turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GoalVerdict {
    Met,
    NotYet,
    Impossible,
}

impl GoalVerdict {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            GoalVerdict::Met => "met",
            GoalVerdict::NotYet => "not_yet",
            GoalVerdict::Impossible => "impossible",
        }
    }
}

/// One judgment: the verdict plus the judge's one-line reason.
#[derive(Debug, Clone)]
pub(crate) struct Judgment {
    pub verdict: GoalVerdict,
    pub reason: String,
}

/// Extract the condition from a `/goal <condition>` user message.
pub(crate) fn parse_goal(text: &str) -> Option<String> {
    let rest = text.strip_prefix("/goal")?;
    let condition = rest.trim();
    if condition.is_empty() || !rest.starts_with(char::is_whitespace) {
        return None;
    }
    Some(condition.to_string())
}

pub(crate) fn max_turns() -> usize {
    std::env::var("ASTER_GOAL_MAX_TURNS")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|&n| n > 0)
        .unwrap_or(DEFAULT_MAX_TURNS)
}

/// The directive the worker sees instead of the raw slash command.
pub(crate) fn directive(condition: &str) -> String {
    format!(
        "Work toward this goal: {condition}\n\nAfter each turn a separate \
        judge checks the condition and the loop continues automatically until \
        it is met, so end your turn whenever you reach a natural checkpoint. \
        Prove progress in your output: run the checks the condition names and \
        show their results."
    )
}

/// The steering message a `not_yet` verdict feeds into the next turn.
pub(crate) fn guidance(condition: &str, reason: &str) -> String {
    format!("Goal not yet met: {reason}\nKeep working toward: {condition}")
}

const JUDGE_PROMPT: &str = "You judge whether a goal condition is met, based \
solely on evidence from an agent's work transcript. Reply with ONLY this \
JSON, nothing else: {\"verdict\":\"met\"|\"not_yet\"|\"impossible\",\
\"reason\":\"one short sentence\"}. Rules: \"met\" only when the evidence \
explicitly demonstrates the condition holds, such as a passing test run or a \
shown result; claims without shown results are not evidence. \"impossible\" \
only when the condition can never be satisfied no matter the work. Otherwise \
\"not_yet\", with the most useful next step as the reason. If the condition \
carries its own budget clause (such as \"stop after 10 turns\") and the \
evidence shows it is exhausted, the verdict is \"impossible\".";

/// Ask the judge. Runs on the collector model when one is configured, so the
/// check costs a fraction of a worker turn.
pub(crate) async fn judge(
    client: &AiClient,
    collector_model: Option<String>,
    condition: &str,
    evidence: &str,
) -> anyhow::Result<Judgment> {
    let mut judge_client = client.clone();
    if let Some(model) = collector_model.or_else(|| std::env::var("ASTER_COLLECTOR_MODEL").ok()) {
        judge_client.model = model;
    }
    let user = format!("Goal condition:\n{condition}\n\nEvidence (newest last):\n{evidence}");
    let reply = judge_client.complete(JUDGE_PROMPT, &user, 0.0).await?;
    Ok(parse_judgment(&reply))
}

/// Parse the judge's reply, tolerating prose around the JSON. An unreadable
/// reply is `not_yet`: the turn cap bounds the damage, a false `met` does not.
fn parse_judgment(reply: &str) -> Judgment {
    let json = reply
        .find('{')
        .and_then(|start| reply.rfind('}').map(|end| &reply[start..=end]))
        .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok());
    if let Some(v) = json {
        let verdict = match v.get("verdict").and_then(|x| x.as_str()) {
            Some("met") => GoalVerdict::Met,
            Some("impossible") => GoalVerdict::Impossible,
            _ => GoalVerdict::NotYet,
        };
        let reason = v
            .get("reason")
            .and_then(|x| x.as_str())
            .unwrap_or("no reason given")
            .to_string();
        return Judgment { verdict, reason };
    }
    Judgment {
        verdict: GoalVerdict::NotYet,
        reason: "the progress check was unclear; continuing".to_string(),
    }
}

/// Tail of the work the judge reads: recent assistant output, newest last,
/// clipped so the check stays cheap.
pub(crate) fn evidence(history: &[ChatMessage], latest_reply: &str) -> String {
    const BUDGET: usize = 6000;
    let mut parts: Vec<String> = history
        .iter()
        .rev()
        .filter(|m| m.role == "assistant")
        .take(2)
        .map(|m| m.content.text().into_owned())
        .collect();
    parts.reverse();
    parts.push(latest_reply.to_string());
    let joined = parts.join("\n\n---\n\n");
    if joined.len() <= BUDGET {
        return joined;
    }
    let mut cut = joined.len() - BUDGET;
    while !joined.is_char_boundary(cut) {
        cut += 1;
    }
    format!("... [truncated]\n{}", &joined[cut..])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_goal_requires_a_condition_after_the_command() {
        assert_eq!(parse_goal("/goal tests pass"), Some("tests pass".into()));
        assert_eq!(
            parse_goal("/goal   lint is clean "),
            Some("lint is clean".into())
        );
        assert_eq!(parse_goal("/goal"), None);
        assert_eq!(parse_goal("/goal   "), None);
        assert_eq!(parse_goal("/goals tests pass"), None);
        assert_eq!(parse_goal("fix the tests"), None);
    }

    #[test]
    fn judgments_parse_with_or_without_surrounding_prose() {
        let j = parse_judgment("{\"verdict\":\"met\",\"reason\":\"suite green\"}");
        assert_eq!(j.verdict, GoalVerdict::Met);
        assert_eq!(j.reason, "suite green");

        let j =
            parse_judgment("Sure! {\"verdict\":\"impossible\",\"reason\":\"no such file\"} done");
        assert_eq!(j.verdict, GoalVerdict::Impossible);

        let j = parse_judgment("garbage");
        assert_eq!(j.verdict, GoalVerdict::NotYet);
    }

    #[test]
    fn an_unknown_verdict_falls_back_to_not_yet() {
        let j = parse_judgment("{\"verdict\":\"maybe\",\"reason\":\"hmm\"}");
        assert_eq!(j.verdict, GoalVerdict::NotYet);
    }
}
