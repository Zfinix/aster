//! Wire-format translation between Aster's OpenAI `/chat/completions` shapes and
//! the Codex backend's Responses API. The only place that knows the other dialect;
//! auth lives in [`crate::codex`].

use serde_json::{Value, json};

/// The Codex backend host. Only this host takes the Responses path.
pub fn is_codex(base_url: &str) -> bool {
    let host = base_url
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(base_url)
        .split('/')
        .next()
        .unwrap_or_default();
    host == "chatgpt.com"
}

pub fn endpoint(base_url: &str) -> String {
    format!("{}/responses", base_url.trim_end_matches('/'))
}

/// Translate a chat-completions request body (plain or tool-calling; both
/// serialize to the same shape) into a Responses API request.
pub fn translate_request(chat: &Value) -> Value {
    let messages = chat["messages"].as_array().cloned().unwrap_or_default();

    let mut instructions = Vec::new();
    let mut input = Vec::new();
    for message in &messages {
        let role = message["role"].as_str().unwrap_or_default();
        match role {
            "system" => {
                if let Some(text) = text_of(&message["content"]) {
                    instructions.push(text);
                }
            }
            "user" => input.push(user_item(message)),
            "assistant" => {
                if let Some(text) = text_of(&message["content"]) {
                    input.push(json!({
                        "type": "message",
                        "role": "assistant",
                        "content": [{"type": "output_text", "text": text}],
                    }));
                }
                for call in message["tool_calls"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                {
                    let function = &call["function"];
                    input.push(json!({
                        "type": "function_call",
                        "call_id": call["id"],
                        "name": function["name"],
                        "arguments": function["arguments"],
                    }));
                }
            }
            "tool" => input.push(json!({
                "type": "function_call_output",
                "call_id": message["tool_call_id"],
                "output": text_of(&message["content"]).unwrap_or_default(),
            })),
            _ => {}
        }
    }

    let mut request = json!({
        "model": chat["model"],
        "input": input,
        "stream": chat["stream"].as_bool().unwrap_or(false),
        // The ChatGPT backend rejects stored responses on subscription auth.
        "store": false,
    });
    if !instructions.is_empty() {
        request["instructions"] = json!(instructions.join("\n\n"));
    }
    // The Responses API has no seed or plugins; effort carries over as-is.
    // The backend rejects requests with no reasoning block at all, so one is
    // always sent, defaulting to medium.
    let effort = chat["reasoning"]["effort"].as_str().unwrap_or("medium");
    request["reasoning"] = json!({"effort": effort});
    let tools: Vec<Value> = chat["tools"]
        .as_array()
        .map(|tools| {
            tools
                .iter()
                .map(|tool| {
                    let function = &tool["function"];
                    json!({
                        "type": "function",
                        "name": function["name"],
                        "description": function["description"],
                        "parameters": function["parameters"],
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    if !tools.is_empty() {
        request["tools"] = json!(tools);
    }
    request
}

/// A user message becomes an input item. Text arrives either as a plain string
/// or as content parts; image parts are forwarded so vision models on the plan
/// still see them.
fn user_item(message: &Value) -> Value {
    match message["content"] {
        Value::String(ref text) => json!({
            "type": "message",
            "role": "user",
            "content": [{"type": "input_text", "text": text}],
        }),
        ref parts @ Value::Array(_) => {
            let content: Vec<Value> = parts
                .as_array()
                .expect("checked above")
                .iter()
                .filter_map(|part| match part["type"].as_str() {
                    Some("text") => Some(json!({
                        "type": "input_text",
                        "text": part["text"],
                    })),
                    Some("image_url") => Some(json!({
                        "type": "input_image",
                        "image_url": part["image_url"]["url"],
                    })),
                    _ => None,
                })
                .collect();
            json!({
                "type": "message",
                "role": "user",
                "content": content,
            })
        }
        _ => json!({
            "type": "message",
            "role": "user",
            "content": [],
        }),
    }
}

/// Text out of a message content field, whether it arrived as a string or as
/// content parts.
fn text_of(content: &Value) -> Option<String> {
    match content {
        Value::String(text) => (!text.is_empty()).then(|| text.clone()),
        Value::Array(parts) => {
            let joined = parts
                .iter()
                .filter_map(|part| part["text"].as_str())
                .collect::<Vec<_>>()
                .join("");
            (!joined.is_empty()).then_some(joined)
        }
        _ => None,
    }
}

/// Translate a non-streaming Responses reply into a chat-completions body, so
/// the existing parsing paths run unchanged.
pub fn translate_response(responses: &Value) -> Value {
    let mut content = String::new();
    let mut tool_calls = Vec::new();
    for item in responses["output"].as_array().cloned().unwrap_or_default() {
        match item["type"].as_str() {
            Some("message") => {
                if let Some(text) = text_of(&item["content"]) {
                    content.push_str(&text);
                }
            }
            Some("function_call") => tool_calls.push(json!({
                "id": item["call_id"].as_str().or_else(|| item["id"].as_str()).unwrap_or_default(),
                "kind": "function",
                "function": {
                    "name": item["name"],
                    "arguments": item["arguments"],
                },
            })),
            _ => {}
        }
    }
    // Always a string: a null content breaks the plain-chat response parse.
    let message = json!({
        "role": "assistant",
        "content": content,
        "tool_calls": tool_calls,
    });
    json!({
        "choices": [{
            "index": 0,
            "message": message,
            "finish_reason": if tool_calls.is_empty() { "stop" } else { "tool_calls" },
        }],
        "usage": translate_usage(&responses["usage"]),
    })
}

/// Assemble a whole SSE stream into one chat-completions body for callers
/// that did not ask to stream; the backend refuses `stream: false`.
pub fn assemble_stream(sse: &str) -> Option<Value> {
    let mut output = Vec::new();
    let mut completed = None;
    for line in sse.lines() {
        let Some(data) = line.strip_prefix("data:") else {
            continue;
        };
        let Ok(event) = serde_json::from_str::<Value>(data.trim()) else {
            continue;
        };
        match event["type"].as_str() {
            Some("response.output_item.done") => output.push(event["item"].clone()),
            Some("response.completed") => completed = Some(event["response"].clone()),
            _ => {}
        }
    }
    let mut response = completed?;
    // The backend strips `output` from the completed event; the item-done
    // events above are then the only whole copy of the reply.
    if !response["output"]
        .as_array()
        .is_some_and(|items| !items.is_empty())
    {
        response["output"] = Value::Array(output);
    }
    Some(translate_response(&response))
}

/// Map a Responses usage block onto the chat-completions names. Absent fields
/// stay absent rather than becoming zeros, so estimation still kicks in.
fn translate_usage(usage: &Value) -> Value {
    let mut mapped = json!({});
    if let Some(v) = usage["input_tokens"].as_u64() {
        mapped["prompt_tokens"] = json!(v);
    }
    if let Some(v) = usage["output_tokens"].as_u64() {
        mapped["completion_tokens"] = json!(v);
    }
    if let (Some(p), Some(c)) = (
        usage["input_tokens"].as_u64(),
        usage["output_tokens"].as_u64(),
    ) {
        mapped["total_tokens"] = json!(p + c);
    }
    mapped
}

/// Stateful SSE event translator for one streaming request. Tool-call items
/// arrive whole, so each gets its own fragment index here; the reassembly in
/// [`crate`] keys fragments by index and would otherwise merge distinct calls.
#[derive(Default)]
pub struct StreamTranslator {
    tool_indices: std::collections::HashMap<String, usize>,
}

impl StreamTranslator {
    /// Translate one SSE `data:` payload into a chat-completions stream chunk.
    /// `None` for events with no chat-completions equivalent, which the caller
    /// skips.
    pub fn event(&mut self, data: &str) -> Option<String> {
        let parsed: Value = serde_json::from_str(data).ok()?;
        let chunk = match parsed["type"].as_str()? {
            "response.output_text.delta" => json!({
                "choices": [{"index": 0, "delta": {"content": parsed["delta"]}}],
            }),
            "response.function_call_arguments.delta" => json!({
                "choices": [{"index": 0, "delta": {"tool_calls": [{
                    "index": self.tool_index(&parsed),
                    "function": {"arguments": parsed["delta"]},
                }]}}],
            }),
            "response.output_item.done" if parsed["item"]["type"] == "function_call" => {
                let item = &parsed["item"];
                json!({
                    "choices": [{"index": 0, "delta": {"tool_calls": [{
                        "index": self.tool_index(&parsed),
                        "id": item["call_id"].as_str().or_else(|| item["id"].as_str()).unwrap_or_default(),
                        "function": {
                            "name": item["name"],
                            "arguments": item["arguments"],
                        },
                    }]}}],
                })
            }
            "response.completed" => json!({
                "choices": [{"index": 0, "delta": {}}],
                "usage": translate_usage(&parsed["response"]["usage"]),
            }),
            _ => return None,
        };
        Some(chunk.to_string())
    }

    /// Fragment index for an event's tool call. An argument delta and the
    /// item-done event that closes its call share an `item_id`, so they must
    /// land in the same reassembly slot; each distinct call gets its own.
    fn tool_index(&mut self, parsed: &Value) -> usize {
        let Some(id) = parsed["item_id"]
            .as_str()
            .or_else(|| parsed["item"]["id"].as_str())
        else {
            return 0;
        };
        let next = self.tool_indices.len();
        *self.tool_indices.entry(id.to_string()).or_insert(next)
    }
}

#[cfg(test)]
#[path = "codex_api_tests.rs"]
mod tests;
