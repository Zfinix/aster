//! Deepgram speech to text.
//! See <https://developers.deepgram.com/reference/speech-to-text/listen-pre-recorded>.

const URL: &str = "https://api.deepgram.com/v1/listen";
pub const MODEL: &str = "nova-3";

#[derive(Clone)]
pub struct Client {
    key: String,
    model: String,
    http: reqwest::Client,
}

impl Client {
    pub fn new(key: String, model: String) -> Self {
        Self {
            key,
            model,
            http: crate::http(),
        }
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    pub async fn transcribe(&self, wav: Vec<u8>, language: Option<&str>) -> Result<String, String> {
        let mut query = vec![("model", self.model.as_str()), ("smart_format", "true")];
        if let Some(language) = language {
            query.push(("language", language));
        }
        let res = self
            .http
            .post(URL)
            .query(&query)
            .header("Authorization", format!("Token {}", self.key))
            .header("Content-Type", "audio/wav")
            .body(wav)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let status = res.status();
        let body = res.text().await.map_err(|e| e.to_string())?;
        if !status.is_success() {
            return Err(format!("{status}: {body}"));
        }
        transcript(&body)
    }
}

fn transcript(body: &str) -> Result<String, String> {
    let reply: serde_json::Value =
        serde_json::from_str(body).map_err(|e| format!("unreadable reply ({e}): {body}"))?;
    reply
        .pointer("/results/channels/0/alternatives/0/transcript")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| format!("reply has no transcript: {body}"))
}
