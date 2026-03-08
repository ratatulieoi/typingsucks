use anyhow::{Context, Result};
use reqwest::blocking::multipart;
use tracing::info;

pub struct ApiTranscriber {
    client: reqwest::blocking::Client,
    url: String,
    api_key: String,
    model: String,
    language: String,
}

impl ApiTranscriber {
    pub fn new(url: &str, api_key: &str, model: &str, language: &str) -> Result<Self> {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .context("Failed to create HTTP client")?;

        // Ensure URL ends with /v1/audio/transcriptions
        let url = url.trim_end_matches('/').to_string();
        let url = if url.ends_with("/v1/audio/transcriptions") {
            url
        } else if url.ends_with("/v1") {
            format!("{}/audio/transcriptions", url)
        } else {
            format!("{}/v1/audio/transcriptions", url)
        };

        info!("API transcriber configured: {} model={}", url, model);
        Ok(ApiTranscriber {
            client,
            url,
            api_key: api_key.to_string(),
            model: model.to_string(),
            language: language.to_string(),
        })
    }

    /// Transcribe 16kHz mono f32 audio samples via API.
    pub fn transcribe(&self, samples: &[f32]) -> Result<String> {
        let wav = encode_wav(samples, 16000);

        let mut form = multipart::Form::new()
            .part(
                "file",
                multipart::Part::bytes(wav)
                    .file_name("audio.wav")
                    .mime_str("audio/wav")?,
            )
            .text("model", self.model.clone())
            .text("response_format", "text");

        if self.language != "auto" && !self.language.is_empty() {
            form = form.text("language", self.language.clone());
        }

        let response = self
            .client
            .post(&self.url)
            .bearer_auth(&self.api_key)
            .multipart(form)
            .send()
            .context("API request failed")?;

        let status = response.status();
        let body = response.text().context("Failed to read API response")?;

        if !status.is_success() {
            anyhow::bail!("API error ({}): {}", status, body);
        }

        Ok(body.trim().to_string())
    }
}

/// Encode f32 samples as a WAV file (16-bit PCM, mono).
/// Writes a 44-byte header followed by PCM16 data. No extra crates needed.
fn encode_wav(samples: &[f32], sample_rate: u32) -> Vec<u8> {
    let num_samples = samples.len();
    let data_size = (num_samples * 2) as u32; // 16-bit = 2 bytes per sample
    let file_size = 36 + data_size;

    let mut buf = Vec::with_capacity(44 + data_size as usize);

    // RIFF header
    buf.extend_from_slice(b"RIFF");
    buf.extend_from_slice(&file_size.to_le_bytes());
    buf.extend_from_slice(b"WAVE");

    // fmt chunk
    buf.extend_from_slice(b"fmt ");
    buf.extend_from_slice(&16u32.to_le_bytes()); // chunk size
    buf.extend_from_slice(&1u16.to_le_bytes()); // PCM format
    buf.extend_from_slice(&1u16.to_le_bytes()); // mono
    buf.extend_from_slice(&sample_rate.to_le_bytes());
    buf.extend_from_slice(&(sample_rate * 2).to_le_bytes()); // byte rate
    buf.extend_from_slice(&2u16.to_le_bytes()); // block align
    buf.extend_from_slice(&16u16.to_le_bytes()); // bits per sample

    // data chunk
    buf.extend_from_slice(b"data");
    buf.extend_from_slice(&data_size.to_le_bytes());

    for &sample in samples {
        let clamped = sample.clamp(-1.0, 1.0);
        let pcm16 = (clamped * 32767.0) as i16;
        buf.extend_from_slice(&pcm16.to_le_bytes());
    }

    buf
}
