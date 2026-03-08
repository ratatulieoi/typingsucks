pub mod api;
pub mod whisper;

use anyhow::Result;

use self::api::ApiTranscriber;
use self::whisper::WhisperTranscriber;

pub enum Transcriber {
    Local(WhisperTranscriber),
    Api(ApiTranscriber),
}

impl Transcriber {
    pub fn transcribe(&self, samples: &[f32]) -> Result<String> {
        match self {
            Transcriber::Local(w) => w.transcribe(samples),
            Transcriber::Api(a) => a.transcribe(samples),
        }
    }
}
