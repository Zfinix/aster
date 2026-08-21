//! Saved sessions, and the transcript a reopened one is rebuilt from. Assistant
//! turns keep their thinking and tool calls: replaying only `content` would drop
//! every reasoning block and any turn that was tool calls alone, which is most
//! of a working session.

use serde_json::{Value, json};

use crate::cli::Cli;

pub async fn list(cli: &Cli) -> Value {
    cli.json(&["sessions", "list"], None)
        .await
        .unwrap_or_else(|_| json!([]))
}

pub async fn delete(cli: &Cli, id: &str) -> Result<(), String> {
    cli.json(&["sessions", "delete", id], None)
        .await
        .map(|_| ())
}

pub async fn rename(cli: &Cli, id: &str, title: &str) -> Result<(), String> {
    cli.json(&["sessions", "rename", id, title], None)
        .await
        .map(|_| ())
}

pub async fn load(cli: &Cli, id: &str) -> Result<Value, String> {
    let parsed = cli
        .json(&["sessions", "show", id], None)
        .await
        .map_err(|_| format!("could not load session {id}"))?;
    Ok(turns(&parsed))
}

/// Rebuild the turns the thread renders from a session's recorded events.
fn turns(session: &Value) -> Value {
    let events: Vec<&Value> = session["events"]
        .as_array()
        .map(|events| {
            events
                .iter()
                .filter(|event| event["type"] == "message")
                .collect()
        })
        .unwrap_or_default();

    // Results arrive as their own `tool` events after the call, so index them
    // first and let each assistant turn pick up its own.
    let results: std::collections::HashMap<&str, &str> = events
        .iter()
        .filter(|event| event["role"] == "tool")
        .filter_map(|event| {
            Some((
                event["tool_call_id"].as_str()?,
                event["content"].as_str().unwrap_or_default(),
            ))
        })
        .collect();

    let mut turns = Vec::new();
    for event in events {
        let content = event["content"].as_str().unwrap_or_default();
        match event["role"].as_str() {
            // Harness steering is recorded as `system` and never lands here,
            // but an empty user turn is still nothing to draw.
            Some("user") if !content.trim().is_empty() => {
                turns.push(json!({ "role": "user", "content": content }));
            }
            Some("assistant") => {
                let calls = tool_calls(&event["tool_calls"], &results);
                let reasoning = reasoning(&event["reasoning"]);
                // A round can be tool calls with no commentary, or thinking
                // alone. Only a turn carrying none of the three is empty.
                if content.trim().is_empty() && calls.is_empty() && reasoning.is_none() {
                    continue;
                }
                turns.push(json!({
                    "role": "assistant",
                    "content": content,
                    "reasoning": reasoning,
                    "toolCalls": calls,
                }));
            }
            _ => {}
        }
    }
    Value::Array(turns)
}

fn tool_calls(calls: &Value, results: &std::collections::HashMap<&str, &str>) -> Vec<Value> {
    calls
        .as_array()
        .map(|calls| {
            calls
                .iter()
                .enumerate()
                .map(|(index, call)| {
                    let id = call["id"]
                        .as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| format!("restored-{index}"));
                    let result = results.get(id.as_str()).copied();
                    json!({
                        "id": id,
                        "name": call["function"]["name"].as_str().unwrap_or("unknown"),
                        "arguments": call["function"]["arguments"].as_str().unwrap_or("{}"),
                        "result": result,
                        "error": result.map(|r| r.starts_with("error: ")).filter(|e| *e),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn reasoning(reasoning: &Value) -> Option<Value> {
    let text = reasoning["text"]
        .as_str()
        .filter(|t| !t.trim().is_empty())?;
    Some(json!({
        "text": text,
        "tokens": reasoning["tokens"],
        "durationMs": reasoning["duration_ms"],
    }))
}

#[cfg(test)]
#[path = "tests/sessions_test.rs"]
mod tests;
