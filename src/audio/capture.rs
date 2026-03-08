use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, SampleFormat, Stream};
use ringbuf::traits::{Consumer, Observer, Producer, Split};
use ringbuf::HeapRb;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tracing::{error, info, warn};

const RING_BUFFER_SECONDS: usize = 30;

pub struct AudioCapture {
    #[allow(dead_code)] // kept alive for Drop — stops recording when dropped
    stream: Stream,
    consumer: ringbuf::HeapCons<f32>,
    recording: Arc<AtomicBool>,
    pub sample_rate: u32,
    pub channels: u16,
}

/// Find a real microphone device, skipping monitor/loopback sources.
fn find_microphone(host: &cpal::Host) -> Option<Device> {
    if let Ok(devices) = host.input_devices() {
        let mut fallback: Option<Device> = None;

        for device in devices {
            let name = device.name().unwrap_or_default().to_lowercase();

            // Skip monitor/loopback sources — these capture system audio
            if name.contains("monitor") || name.contains("loopback") {
                info!("Skipping monitor source: {}", device.name().unwrap_or_default());
                continue;
            }

            // Prefer devices that look like actual microphones
            if name.contains("mic")
                || name.contains("input")
                || name.contains("capture")
                || name.contains("webcam")
                || name.contains("headset")
            {
                info!("Selected microphone: {}", device.name().unwrap_or_default());
                return Some(device);
            }

            // Keep first non-monitor device as fallback
            if fallback.is_none() {
                fallback = Some(device);
            }
        }

        // Use fallback if no obvious mic found
        if let Some(ref dev) = fallback {
            info!(
                "Using input device (no obvious mic found): {}",
                dev.name().unwrap_or_default()
            );
        }
        return fallback;
    }
    None
}

impl AudioCapture {
    pub fn new() -> Result<Self> {
        let host = cpal::default_host();

        // Try to find a real microphone first, fall back to default
        let device = match find_microphone(&host) {
            Some(dev) => dev,
            None => {
                warn!("Could not enumerate input devices, using default");
                host.default_input_device()
                    .context("No input device found. Is a microphone connected?")?
            }
        };

        let config = device
            .default_input_config()
            .context("Failed to get default input config")?;

        info!(
            "Audio device: {} ({}Hz, {} ch, {:?})",
            device.name().unwrap_or_default(),
            config.sample_rate().0,
            config.channels(),
            config.sample_format()
        );

        let sample_rate = config.sample_rate().0;
        let channels = config.channels();
        let buffer_size = sample_rate as usize * channels as usize * RING_BUFFER_SECONDS;
        let rb = HeapRb::<f32>::new(buffer_size);
        let (mut producer, consumer) = rb.split();

        let recording = Arc::new(AtomicBool::new(false));
        let recording_flag = recording.clone();

        let err_fn = |err: cpal::StreamError| {
            error!("Audio stream error: {}", err);
        };

        let stream = match config.sample_format() {
            SampleFormat::F32 => device.build_input_stream(
                &config.into(),
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    if recording_flag.load(Ordering::Relaxed) {
                        for &sample in data {
                            let _ = producer.try_push(sample);
                        }
                    }
                },
                err_fn,
                None,
            )?,
            SampleFormat::I16 => {
                let recording_flag = recording.clone();
                device.build_input_stream(
                    &config.into(),
                    move |data: &[i16], _: &cpal::InputCallbackInfo| {
                        if recording_flag.load(Ordering::Relaxed) {
                            for &sample in data {
                                let _ = producer.try_push(sample as f32 / i16::MAX as f32);
                            }
                        }
                    },
                    err_fn,
                    None,
                )?
            }
            format => anyhow::bail!("Unsupported sample format: {:?}", format),
        };

        stream.play().context("Failed to start audio stream")?;

        Ok(AudioCapture {
            stream,
            consumer,
            recording,
            sample_rate,
            channels,
        })
    }

    pub fn start_recording(&self) {
        self.recording.store(true, Ordering::Relaxed);
    }

    pub fn stop_recording(&mut self) -> Vec<f32> {
        self.recording.store(false, Ordering::Relaxed);
        let available = self.consumer.occupied_len();
        let mut samples = Vec::with_capacity(available);
        for _ in 0..available {
            if let Some(s) = self.consumer.try_pop() {
                samples.push(s);
            }
        }
        samples
    }

    pub fn clear_buffer(&mut self) {
        while self.consumer.try_pop().is_some() {}
    }
}
