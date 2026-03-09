use anyhow::{Context, Result};
use std::path::Path;
use tracing::info;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

pub struct WhisperTranscriber {
    ctx: WhisperContext,
    language: Option<String>,
}

impl WhisperTranscriber {
    pub fn new(model_path: &Path, language: &str) -> Result<Self> {
        info!("Loading whisper model: {}", model_path.display());
        let ctx = WhisperContext::new_with_params(
            model_path
                .to_str()
                .context("Invalid model path")?,
            WhisperContextParameters::default(),
        )
        .map_err(|e| anyhow::anyhow!("Failed to load whisper model: {}", e))?;

        let language = if language == "auto" {
            None
        } else {
            Some(language.to_string())
        };

        info!("Whisper model loaded");
        Ok(WhisperTranscriber { ctx, language })
    }

    /// Transcribe 16kHz mono f32 audio samples to text.
    pub fn transcribe(&self, samples: &[f32]) -> Result<String> {
        let mut state = self.ctx.create_state()
            .map_err(|e| anyhow::anyhow!("Failed to create whisper state: {}", e))?;

        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });

        if let Some(ref lang) = self.language {
            params.set_language(Some(lang));
        } else {
            params.set_language(None);
        }

        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        params.set_suppress_blank(true);
        params.set_no_context(true);
        // Allow multi-segment transcription for longer recordings
        params.set_single_segment(false);
        // Suppress non-speech tokens to prevent hallucinations like "hai"
        params.set_suppress_non_speech_tokens(true);

        state
            .full(params, samples)
            .map_err(|e| anyhow::anyhow!("Whisper inference failed: {}", e))?;

        let num_segments = state.full_n_segments()
            .map_err(|e| anyhow::anyhow!("Failed to get segments: {}", e))?;

        let mut text = String::new();
        for i in 0..num_segments {
            if let Ok(segment) = state.full_get_segment_text(i) {
                let seg_text = segment.trim();
                // Skip hallucinated segments (common whisper artifacts)
                if seg_text.is_empty()
                    || seg_text == "[BLANK_AUDIO]"
                    || seg_text == "."
                {
                    continue;
                }
                if !text.is_empty() {
                    text.push(' ');
                }
                text.push_str(seg_text);
            }
        }

        Ok(text.trim().to_string())
    }
}
