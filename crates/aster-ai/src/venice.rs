//! Venice serves the OpenAI shape but rejects a `seed` below 1, which is the
//! default Aster sends.

pub(crate) fn is_venice(base_url: &str) -> bool {
    base_url.contains("api.venice.ai")
}

pub(crate) fn adapt_request(body: &serde_json::Value) -> serde_json::Value {
    let mut body = body.clone();
    if let Some(object) = body.as_object_mut()
        && object
            .get("seed")
            .and_then(serde_json::Value::as_i64)
            .is_some_and(|s| s < 1)
    {
        object.remove("seed");
    }
    body
}

#[cfg(test)]
#[path = "tests/venice_test.rs"]
mod tests;
