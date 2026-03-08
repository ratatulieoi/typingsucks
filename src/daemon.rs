use crate::audio::capture::AudioCapture;
use crate::audio::denoise;
use crate::audio::resample;
use crate::config::{self, Config, TranscriptionMode};
use crate::hotkey::{HotkeyEvent, HotkeyListener};
use crate::model;
use crate::output;
use crate::state::State;
use crate::transcribe::api::ApiTranscriber;
use crate::transcribe::whisper::WhisperTranscriber;
use crate::transcribe::Transcriber;
use crate::ui::popup;

use anyhow::{Context, Result};
use crossbeam_channel::{Receiver, Sender};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Arc;
use std::thread;
use tracing::{error, info, warn};

/// Commands sent from the GUI to the daemon thread
#[derive(Debug)]
#[allow(dead_code)]
pub enum DaemonCommand {
    Stop,
    Reload(Config),
}

fn pid_file() -> PathBuf {
    config::runtime_dir().join("daemon.pid")
}

/// Start the daemon in a background thread. Returns a JoinHandle.
/// The daemon listens for DaemonCommand on `cmd_rx`.
/// Sets `running` to true when started, false when stopped.
pub fn start_background(
    config: Config,
    cmd_rx: Receiver<DaemonCommand>,
    running: Arc<AtomicBool>,
    ui_state: Arc<AtomicU8>,
) -> Result<thread::JoinHandle<()>> {
    // Build transcriber before spawning thread (so we can report errors immediately)
    let transcriber = build_transcriber(&config)?;

    let handle = thread::spawn(move || {
        if let Err(e) = daemon_loop(config, transcriber, cmd_rx, running.clone(), ui_state) {
            error!("Daemon error: {}", e);
        }
        running.store(false, Ordering::SeqCst);
    });

    Ok(handle)
}

fn build_transcriber(config: &Config) -> Result<Transcriber> {
    match config.transcription.mode {
        TranscriptionMode::Api => {
            let t = ApiTranscriber::new(
                &config.transcription.api_url,
                &config.transcription.api_key,
                &config.transcription.api_model,
                &config.model.language,
            )?;
            Ok(Transcriber::Api(t))
        }
        TranscriptionMode::Local => {
            let model_path = model::resolve_model_path(config)
                .map_err(|_| anyhow::anyhow!("No model found. Download one first."))?;
            let t = WhisperTranscriber::new(&model_path, &config.model.language)
                .context("Failed to load whisper model")?;
            Ok(Transcriber::Local(t))
        }
    }
}

fn daemon_loop(
    cfg: Config,
    transcriber: Transcriber,
    cmd_rx: Receiver<DaemonCommand>,
    running: Arc<AtomicBool>,
    ui_state: Arc<AtomicU8>,
) -> Result<()> {
    let transcriber = Arc::new(transcriber);

    // Init audio capture
    let audio = Arc::new(std::sync::Mutex::new(
        AudioCapture::new().context("Failed to initialize audio capture")?,
    ));
    let sample_rate;
    let channels;
    {
        let a = audio.lock().unwrap();
        sample_rate = a.sample_rate;
        channels = a.channels;
    }

    // Hotkey listener
    let hotkey = HotkeyListener::new(&cfg.hotkey.key)
        .context("Failed to start hotkey listener")?;

    running.store(true, Ordering::SeqCst);

    info!(
        "Daemon started. Hold [{}] to record, release to transcribe.",
        cfg.hotkey.key
    );

    // Event channel from hotkey to main loop
    let (event_tx, event_rx): (Sender<HotkeyEvent>, Receiver<HotkeyEvent>) =
        crossbeam_channel::unbounded();

    // Hotkey thread -> event channel
    let running_hotkey = running.clone();
    thread::spawn(move || {
        loop {
            if !running_hotkey.load(Ordering::SeqCst) {
                break;
            }
            match hotkey.recv() {
                Ok(ev) => {
                    let _ = event_tx.send(ev);
                }
                Err(_) => break,
            }
        }
    });

    let auto_paste = cfg.behavior.auto_paste;
    let mut current_state = State::Idle;

    loop {
        // Check for commands from GUI
        if let Ok(cmd) = cmd_rx.try_recv() {
            match cmd {
                DaemonCommand::Stop => {
                    info!("Received stop command");
                    break;
                }
                DaemonCommand::Reload(_new_cfg) => {
                    info!("Config reload requested (requires restart for full effect)");
                    // For a full reload we'd need to reinit hotkey/model —
                    // for now, stop so the GUI can restart with new config
                    break;
                }
            }
        }

        // Process hotkey events with timeout
        match event_rx.recv_timeout(std::time::Duration::from_millis(100)) {
            Ok(HotkeyEvent::Pressed) => {
                if current_state == State::Idle {
                    current_state = current_state.on_key_down();
                    ui_state.store(popup::state_to_u8(current_state), Ordering::Relaxed);

                    let mut a = audio.lock().unwrap();
                    a.clear_buffer();
                    a.start_recording();
                    info!("Recording...");
                }
            }
            Ok(HotkeyEvent::Released) => {
                if current_state == State::Recording {
                    current_state = current_state.on_key_up();
                    ui_state.store(popup::state_to_u8(current_state), Ordering::Relaxed);

                    let raw_samples = {
                        let mut a = audio.lock().unwrap();
                        a.stop_recording()
                    };

                    info!("Captured {} samples, transcribing...", raw_samples.len());

                    if raw_samples.is_empty() {
                        warn!("No audio captured");
                        current_state = State::Idle;
                        ui_state.store(popup::state_to_u8(current_state), Ordering::Relaxed);
                        continue;
                    }

                    // Mix to mono for denoising
                    let mono: Vec<f32> = if channels == 1 {
                        raw_samples.clone()
                    } else {
                        raw_samples
                            .chunks(channels as usize)
                            .map(|frame| frame.iter().sum::<f32>() / channels as f32)
                            .collect()
                    };

                    // Denoise (removes background music/noise, keeps voice)
                    let denoised = match denoise::suppress_noise(&mono, sample_rate) {
                        Ok(d) => d,
                        Err(e) => {
                            warn!("Denoise failed ({}), using raw audio", e);
                            mono
                        }
                    };

                    // Resample denoised mono to 16kHz for Whisper
                    let whisper_samples = match resample::to_whisper_format(
                        &denoised,
                        sample_rate,
                        1, // already mono
                    ) {
                        Ok(s) => s,
                        Err(e) => {
                            error!("Resample failed: {}", e);
                            current_state = State::Idle;
                            ui_state.store(popup::state_to_u8(current_state), Ordering::Relaxed);
                            continue;
                        }
                    };

                    // Transcribe
                    match transcriber.transcribe(&whisper_samples) {
                        Ok(text) => {
                            let text: String = text;
                            if text.is_empty() || text == "[BLANK_AUDIO]" {
                                info!("No speech detected");
                                current_state = State::Idle;
                                ui_state.store(
                                    popup::state_to_u8(current_state),
                                    Ordering::Relaxed,
                                );
                                continue;
                            }

                            info!("Transcribed: {}", text);
                            current_state = current_state.on_transcription_done();

                            // Clipboard + paste
                            if let Err(e) = output::clipboard::set_text(&text) {
                                error!("Clipboard failed: {}", e);
                            } else if auto_paste {
                                if let Err(e) = output::paste::simulate_paste() {
                                    warn!("Auto-paste failed: {}", e);
                                }
                            }

                            current_state = current_state.on_paste_done();
                            ui_state.store(popup::state_to_u8(current_state), Ordering::Relaxed);
                        }
                        Err(e) => {
                            error!("Transcription failed: {}", e);
                            current_state = State::Idle;
                            ui_state.store(popup::state_to_u8(current_state), Ordering::Relaxed);
                        }
                    }
                }
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
        }
    }

    running.store(false, Ordering::SeqCst);
    info!("Daemon stopped.");
    Ok(())
}

/// CLI-only: run the daemon in the foreground (blocking, with PID file + signal handling)
pub fn run() -> Result<()> {
    let cfg = Config::load()?;

    // Check for existing daemon
    if is_running() {
        anyhow::bail!("Daemon is already running. Use 'typingsucks stop' first.");
    }

    // For local mode, auto-download if no model found
    if cfg.transcription.mode == TranscriptionMode::Local {
        if model::resolve_model_path(&cfg).is_err() {
            println!("No model found. Downloading base model...");
            model::download_model("base")?;
        }
    }

    // Write PID file
    write_pid()?;

    // Set up signal handler
    let (shutdown_tx, shutdown_rx) = crossbeam_channel::bounded::<()>(1);
    let shutdown_tx_signal = shutdown_tx.clone();
    ctrlc_handler(move || {
        let _ = shutdown_tx_signal.send(());
    });

    // Shared state
    let running = Arc::new(AtomicBool::new(false));
    let ui_state = Arc::new(AtomicU8::new(popup::state_to_u8(State::Idle)));

    // Command channel (for CLI mode, we only use Stop via shutdown signal)
    let (cmd_tx, cmd_rx) = crossbeam_channel::unbounded();

    // Build transcriber
    let transcriber = build_transcriber(&cfg)?;

    // Spawn daemon in background thread
    let handle = thread::spawn({
        let cfg = cfg.clone();
        let running = running.clone();
        let ui_state = ui_state.clone();
        move || {
            if let Err(e) = daemon_loop(cfg, transcriber, cmd_rx, running.clone(), ui_state) {
                error!("Daemon error: {}", e);
            }
            running.store(false, Ordering::SeqCst);
        }
    });

    println!(
        "typingsucks daemon running. Hold [{}] to talk. Press Ctrl+C to stop.",
        cfg.hotkey.key
    );

    // Wait for shutdown signal
    let _ = shutdown_rx.recv();
    info!("Shutting down...");
    let _ = cmd_tx.send(DaemonCommand::Stop);
    let _ = handle.join();

    cleanup_pid();
    info!("Daemon stopped.");
    Ok(())
}

pub fn stop() -> Result<()> {
    let pid_path = pid_file();
    if !pid_path.exists() {
        println!("No daemon running (no PID file found).");
        return Ok(());
    }

    let pid_str = std::fs::read_to_string(&pid_path)?;
    let pid: i32 = pid_str.trim().parse().context("Invalid PID file")?;

    // Send SIGTERM
    unsafe {
        if libc::kill(pid, libc::SIGTERM) == 0 {
            println!("Sent stop signal to daemon (PID {}).", pid);
        } else {
            println!("Process {} not found. Cleaning up stale PID file.", pid);
        }
    }

    cleanup_pid();
    Ok(())
}

pub fn status() -> Result<()> {
    if is_running() {
        let pid = std::fs::read_to_string(pid_file())?.trim().to_string();
        println!("Daemon is running (PID {}).", pid);
    } else {
        println!("Daemon is not running.");
    }
    Ok(())
}

fn write_pid() -> Result<()> {
    let path = pid_file();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, std::process::id().to_string())?;
    Ok(())
}

fn cleanup_pid() {
    let _ = std::fs::remove_file(pid_file());
}

fn is_running() -> bool {
    let path = pid_file();
    if !path.exists() {
        return false;
    }
    if let Ok(pid_str) = std::fs::read_to_string(&path) {
        if let Ok(pid) = pid_str.trim().parse::<i32>() {
            unsafe {
                return libc::kill(pid, 0) == 0;
            }
        }
    }
    false
}

fn ctrlc_handler<F: FnOnce() + Send + 'static>(f: F) {
    let f = std::sync::Mutex::new(Some(f));
    ctrlc::set_handler(move || {
        if let Some(f) = f.lock().unwrap().take() {
            f();
        }
    })
    .ok();
}
