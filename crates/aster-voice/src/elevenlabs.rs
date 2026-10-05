//! ElevenLabs Scribe speech to text and Flash text to speech.
//! See <https://elevenlabs.io/docs/api-reference/speech-to-text/convert>.

const STT_URL: &str = "https://api.elevenlabs.io/v1/speech-to-text";
const TTS_URL: &str = "https://api.elevenlabs.io/v1/text-to-speech";
pub const STT_MODEL: &str = "scribe_v2";
pub const TTS_MODEL: &str = "eleven_flash_v2_5";
pub const TTS_VOICE: &str = "JBFqnCBsd6RMkjVDRZzb";
const PCM_RATE: u32 = 24_000;

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
        let mut form = reqwest::multipart::Form::new()
            .text("model_id", self.model.clone())
            .text("tag_audio_events", "false")
            .part("file", crate::wav_part(wav));
        if let Some(language) = language {
            form = form.text("language_code", language.to_string());
        }
        crate::send(
            self.http
                .post(STT_URL)
                .header("xi-api-key", &self.key)
                .multipart(form),
        )
        .await
    }

    pub async fn speak(&self, text: &str, voice: &str) -> Result<crate::Clip, String> {
        let body = serde_json::json!({ "text": text, "model_id": self.model });
        let pcm = crate::fetch_bytes(
            self.http
                .post(format!("{TTS_URL}/{voice}?output_format=pcm_{PCM_RATE}"))
                .header("xi-api-key", &self.key)
                .json(&body),
        )
        .await?;
        Ok(crate::Clip::from_pcm(&pcm, PCM_RATE))
    }
}
