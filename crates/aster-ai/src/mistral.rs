//! Mistral serves the OpenAI shape but spells the sampling seed
//! `random_seed`, and refuses a request that carries `seed`.

pub(crate) fn is_mistral(base_url: &str) -> bool {
    base_url.contains("api.mistral.ai")
}

pub(crate) fn adapt_request(body: &serde_json::Value) -> serde_json::Value {
    let mut body = body.clone();
    if let Some(object) = body.as_object_mut()
        && let Some(seed) = object.remove("seed")
    {
        object.insert("random_seed".to_string(), seed);
    }
    body
}

#[cfg(test)]
#[path = "tests/mistral_test.rs"]
mod tests;
