use anyhow::{Context, Result};
use rubato::{FftFixedInOut, Resampler};

const TARGET_RATE: u32 = 16000;

/// Convert multi-channel audio at arbitrary sample rate to 16kHz mono f32.
pub fn to_whisper_format(samples: &[f32], source_rate: u32, channels: u16) -> Result<Vec<f32>> {
    // Mix down to mono
    let mono: Vec<f32> = if channels == 1 {
        samples.to_vec()
    } else {
        samples
            .chunks(channels as usize)
            .map(|frame| frame.iter().sum::<f32>() / channels as f32)
            .collect()
    };

    // No resampling needed
    if source_rate == TARGET_RATE {
        return Ok(mono);
    }

    // Resample to 16kHz
    let chunk_size = 1024;
    let mut resampler = FftFixedInOut::<f32>::new(
        source_rate as usize,
        TARGET_RATE as usize,
        chunk_size,
        1, // mono
    )
    .context("Failed to create resampler")?;

    let mut output = Vec::with_capacity(
        (mono.len() as f64 * TARGET_RATE as f64 / source_rate as f64) as usize + chunk_size,
    );

    let frames_needed = resampler.input_frames_next();

    // Process full chunks
    let mut pos = 0;
    while pos + frames_needed <= mono.len() {
        let input = vec![mono[pos..pos + frames_needed].to_vec()];
        let result = resampler.process(&input, None).context("Resample failed")?;
        output.extend_from_slice(&result[0]);
        pos += frames_needed;
    }

    // Handle remaining samples by zero-padding
    if pos < mono.len() {
        let mut last_chunk = mono[pos..].to_vec();
        last_chunk.resize(frames_needed, 0.0);
        let input = vec![last_chunk];
        let result = resampler.process(&input, None).context("Resample failed")?;
        // Only take the proportional amount
        let expected = ((mono.len() - pos) as f64 * TARGET_RATE as f64 / source_rate as f64) as usize;
        let take = expected.min(result[0].len());
        output.extend_from_slice(&result[0][..take]);
    }

    Ok(output)
}
