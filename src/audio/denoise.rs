use anyhow::Result;
use nnnoiseless::DenoiseState;
use tracing::info;

const RNNOISE_SAMPLE_RATE: u32 = 48000;
const FRAME_SIZE: usize = DenoiseState::FRAME_SIZE; // 480 samples

/// Apply RNNoise-based noise suppression to audio.
/// Input: mono f32 samples at any sample rate.
/// Output: denoised mono f32 samples at the same sample rate.
///
/// Internally resamples to 48kHz (RNNoise requirement), denoises, then resamples back.
pub fn suppress_noise(samples: &[f32], sample_rate: u32) -> Result<Vec<f32>> {
    if samples.is_empty() {
        return Ok(Vec::new());
    }

    // Step 1: Resample to 48kHz if needed
    let samples_48k = if sample_rate == RNNOISE_SAMPLE_RATE {
        samples.to_vec()
    } else {
        resample_simple(samples, sample_rate, RNNOISE_SAMPLE_RATE)
    };

    // Step 2: Apply RNNoise denoising in 480-sample frames
    let mut state = DenoiseState::new();
    let mut output = Vec::with_capacity(samples_48k.len());
    let mut frame_in = [0.0f32; FRAME_SIZE];
    let mut frame_out = [0.0f32; FRAME_SIZE];

    let mut pos = 0;
    while pos < samples_48k.len() {
        let remaining = samples_48k.len() - pos;
        let chunk = remaining.min(FRAME_SIZE);

        // Fill frame, zero-pad if needed
        frame_in[..chunk].copy_from_slice(&samples_48k[pos..pos + chunk]);
        if chunk < FRAME_SIZE {
            frame_in[chunk..].fill(0.0);
        }

        // nnnoiseless expects samples in [-32768, 32767] range (i16 scale)
        for s in &mut frame_in {
            *s *= 32767.0;
        }

        state.process_frame(&mut frame_out, &frame_in);

        // Convert back to [-1.0, 1.0] range
        for s in &mut frame_out[..chunk] {
            *s /= 32767.0;
        }

        output.extend_from_slice(&frame_out[..chunk]);
        pos += chunk;
    }

    info!("Noise suppression applied ({} samples at 48kHz)", output.len());

    // Step 3: Resample back to original rate if needed
    if sample_rate == RNNOISE_SAMPLE_RATE {
        Ok(output)
    } else {
        Ok(resample_simple(&output, RNNOISE_SAMPLE_RATE, sample_rate))
    }
}

/// Simple linear interpolation resampler for rate conversion
fn resample_simple(samples: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    if from_rate == to_rate || samples.is_empty() {
        return samples.to_vec();
    }

    let ratio = from_rate as f64 / to_rate as f64;
    let out_len = (samples.len() as f64 / ratio).ceil() as usize;
    let mut output = Vec::with_capacity(out_len);

    for i in 0..out_len {
        let src_pos = i as f64 * ratio;
        let idx = src_pos as usize;
        let frac = (src_pos - idx as f64) as f32;

        let sample = if idx + 1 < samples.len() {
            samples[idx] * (1.0 - frac) + samples[idx + 1] * frac
        } else if idx < samples.len() {
            samples[idx]
        } else {
            0.0
        };

        output.push(sample);
    }

    output
}
