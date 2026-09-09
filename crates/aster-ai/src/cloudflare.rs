//! Workers AI serves chat on an OpenAI-compatible path but answers no
//! `/models`; the list lives on the account's own REST path, in Cloudflare's
//! envelope rather than OpenAI's shape.

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::ModelInfo;

/// The most rows the search endpoint returns at once.
pub(crate) const PER_PAGE: usize = 100;

/// A stop for the paging loop, so a widened catalog cannot spin it.
pub(crate) const MAX_PAGES: usize = 10;

/// The Workers AI endpoint, which lists models its own way.
pub(crate) fn is_workers_ai(base_url: &str) -> bool {
    let url = base_url.trim_end_matches('/');
    url.starts_with("https://api.cloudflare.com/") && url.ends_with("/ai/v1")
}

/// One page of the account's text generation models. Chat is all Aster sends,
/// so the other tasks (image, speech, embeddings) stay out of the picker.
pub(crate) fn models_url(base_url: &str, page: usize) -> String {
    let account = base_url.trim_end_matches('/').trim_end_matches("/v1");
    format!("{account}/models/search?task=Text%20Generation&per_page={PER_PAGE}&page={page}")
}

/// The gist copy of the catalog, fetched so it can be updated without a
/// rebuild. The gist is the single source of truth.
pub(crate) const GIST_MODELS_URL: &str = "https://gist.githubusercontent.com/Zfinix/eb911066ca6510b350a6abe781f936a4/raw/cloudflare-models.json";

/// The gist's JSON shape.
pub(crate) fn parse_catalog(json: &str) -> Result<Vec<ModelInfo>> {
    #[derive(Deserialize)]
    struct Entry {
        id: String,
        #[serde(default)]
        task: String,
        #[serde(default)]
        vision: bool,
        #[serde(default)]
        context_window: Option<u32>,
    }
    #[derive(Deserialize)]
    struct File {
        models: Vec<Entry>,
    }
    // The file holds every task (image, speech, embeddings too); chat is all
    // Aster sends, so only text generation reaches the picker.
    let file: File = serde_json::from_str(json)?;
    Ok(file
        .models
        .into_iter()
        .filter(|e| e.task == "Text Generation")
        .map(|e| ModelInfo {
            id: e.id,
            takes_images: Some(e.vision),
            context_window: e.context_window,
        })
        .collect())
}

pub(crate) fn parse_models(body: &str) -> Result<Vec<ModelInfo>> {
    let search: Search = serde_json::from_str(body)
        .with_context(|| format!("parsing the Workers AI model list: {body}"))?;
    Ok(search.result.into_iter().map(ModelInfo::from).collect())
}

#[derive(Deserialize)]
struct Search {
    #[serde(default)]
    result: Vec<Model>,
}

#[derive(Deserialize)]
struct Model {
    name: String,
    #[serde(default)]
    properties: Vec<Property>,
}

#[derive(Deserialize)]
struct Property {
    property_id: String,
    #[serde(default)]
    value: serde_json::Value,
}

impl From<Model> for ModelInfo {
    fn from(model: Model) -> Self {
        let takes_images = model
            .properties
            .iter()
            .any(|p| p.property_id == "vision" && p.value.as_str() == Some("true"));
        // Workers AI windows are small enough to matter, and the search writes
        // the number as a string.
        let context_window = model
            .properties
            .iter()
            .find(|p| p.property_id == "context_window")
            .and_then(|p| match &p.value {
                serde_json::Value::String(text) => text.parse().ok(),
                value => value.as_u64().and_then(|n| u32::try_from(n).ok()),
            });
        Self {
            id: model.name,
            takes_images: Some(takes_images),
            context_window,
        }
    }
}

/// Workers AI serves the OpenAI shape but not all of it: `seed` must be >= 1,
/// and message content must be a bare string rather than the parts array the
/// other providers take. Only the fields it rejects are rewritten.
pub(crate) fn adapt_request(body: &serde_json::Value) -> serde_json::Value {
    use serde_json::Value;

    let mut body = body.clone();
    let Some(object) = body.as_object_mut() else {
        return body;
    };
    if object
        .get("seed")
        .and_then(Value::as_i64)
        .is_some_and(|s| s < 1)
    {
        object.remove("seed");
    }
    let Some(messages) = object.get_mut("messages").and_then(Value::as_array_mut) else {
        return body;
    };
    for message in messages {
        // A tool call carries no text, and every other endpoint takes the null
        // that leaves behind; this one reads it as the field being missing.
        if message.get("content").is_none_or(Value::is_null) {
            message["content"] = Value::String(String::new());
            continue;
        }
        let Some(parts) = message.get("content").and_then(Value::as_array) else {
            continue;
        };
        let text: String = parts
            .iter()
            .filter_map(|part| part.get("text").and_then(Value::as_str))
            .collect();
        message["content"] = Value::String(text);
    }
    body
}

#[cfg(test)]
#[path = "tests/cloudflare_test.rs"]
mod tests;
