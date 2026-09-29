//! ElevenLabs Scribe speech to text.
//! See <https://elevenlabs.io/docs/api-reference/speech-to-text/convert>.

const URL: &str = "https://api.elevenlabs.io/v1/speech-to-text";
const MODEL: &str = "scribe_v2";

#[derive(Clone)]
pub struct Client {
    key: String,
    http: reqwest::Client,
}

impl Client {
    pub fn new(key: String) -> Self {
        Self {
            key,
            http: crate::http(),
        }
    }

    pub async fn transcribe(&self, wav: Vec<u8>) -> Result<String, String> {
        let form = reqwest::multipart::Form::new()
            .text("model_id", MODEL)
            .text("tag_audio_events", "false")
            .part("file", crate::wav_part(wav));
        crate::send(
            self.http
                .post(URL)
                .header("xi-api-key", &self.key)
                .multipart(form),
        )
        .await
    }
}
