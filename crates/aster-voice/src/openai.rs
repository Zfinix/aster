//! OpenAI speech to text.
//! See <https://platform.openai.com/docs/api-reference/audio/createTranscription>.

const URL: &str = "https://api.openai.com/v1/audio/transcriptions";
const MODEL: &str = "gpt-4o-transcribe";

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
            .text("model", MODEL)
            .part("file", crate::wav_part(wav));
        crate::send(self.http.post(URL).bearer_auth(&self.key).multipart(form)).await
    }
}
