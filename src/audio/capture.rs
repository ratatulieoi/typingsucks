use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, SampleFormat, Stream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tracing::{error, info, warn};

pub struct AudioCapture {
    #[allow(dead_code)] // kept alive for Drop — stops recording when dropped
    stream: Stream,
    buffer: Arc<Mutex<Vec<f32>>>,
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

        // Growable buffer — no recording length limit
        let buffer = Arc::new(Mutex::new(Vec::<f32>::new()));

        let recording = Arc::new(AtomicBool::new(false));

        let err_fn = |err: cpal::StreamError| {
            error!("Audio stream error: {}", err);
        };

        let stream = match config.sample_format() {
            SampleFormat::F32 => {
                let recording_flag = recording.clone();
                let buf = buffer.clone();
                device.build_input_stream(
                    &config.into(),
                    move |data: &[f32], _: &cpal::InputCallbackInfo| {
                        if recording_flag.load(Ordering::Relaxed) {
                            if let Ok(mut b) = buf.try_lock() {
                                b.extend_from_slice(data);
                            }
                        }
                    },
                    err_fn,
                    None,
                )?
            }
            SampleFormat::I16 => {
                let recording_flag = recording.clone();
                let buf = buffer.clone();
                device.build_input_stream(
                    &config.into(),
                    move |data: &[i16], _: &cpal::InputCallbackInfo| {
                        if recording_flag.load(Ordering::Relaxed) {
                            if let Ok(mut b) = buf.try_lock() {
                                b.extend(data.iter().map(|&s| s as f32 / i16::MAX as f32));
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
            buffer,
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
        let mut buf = self.buffer.lock().unwrap();
        std::mem::take(&mut *buf)
    }

    pub fn clear_buffer(&mut self) {
        let mut buf = self.buffer.lock().unwrap();
        buf.clear();
    }
}

