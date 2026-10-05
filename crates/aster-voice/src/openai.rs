//! OpenAI speech to text and text to speech, and every server that copies the
//! shape: Groq, speaches, kokoro-fastapi, and other local engines.
//! See <https://platform.openai.com/docs/api-reference/audio>.

pub const OPENAI_URL: &str = "https://api.openai.com/v1";
pub const GROQ_URL: &str = "https://api.groq.com/openai/v1";

/// Sample rate of the raw `pcm` format every OpenAI-shaped speech reply uses.
const PCM_RATE: u32 = 24_000;

#[derive(Clone)]
pub struct Client {
    base: String,
    model: String,
    key: Option<String>,
    http: reqwest::Client,
}

impl Client {
    pub fn new(base: &str, model: String, key: Option<String>) -> Self {
        Self {
            base: base.trim_end_matches('/').to_string(),
            model,
            key,
            http: crate::http(),
        }
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    pub async fn transcribe(&self, wav: Vec<u8>, language: Option<&str>) -> Result<String, String> {
        let mut form = reqwest::multipart::Form::new()
            .text("model", self.model.clone())
            .part("file", crate::wav_part(wav));
        if let Some(language) = language {
            form = form.text("language", language.to_string());
        }
        let req = self
            .http
            .post(format!("{}/audio/transcriptions", self.base))
            .multipart(form);
        crate::send(self.authed(req)).await
    }

    pub async fn speak(&self, text: &str, voice: &str) -> Result<crate::Clip, String> {
        let body = serde_json::json!({
            "model": self.model,
            "voice": voice,
            "input": text,
            "response_format": "pcm",
        });
        let req = self
            .http
            .post(format!("{}/audio/speech", self.base))
            .json(&body);
        let pcm = crate::fetch_bytes(self.authed(req)).await?;
        Ok(crate::Clip::from_pcm(&pcm, PCM_RATE))
    }

    fn authed(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.key {
            Some(key) => req.bearer_auth(key),
            None => req,
        }
    }
}
