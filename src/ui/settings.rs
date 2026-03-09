use crate::config::{Config, TranscriptionMode};
use crate::daemon::{self, DaemonCommand};
use crate::hotkey;
use crate::model;
use crate::state::State;
use crate::ui::popup;

use crossbeam_channel::{Receiver, Sender};
use eframe::egui;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use tracing::{error, info};

const LANGUAGE_OPTIONS: &[&str] = &[
    "en", "id", "ja", "zh", "ko", "es", "fr", "de", "pt", "ru", "ar", "hi", "it", "nl", "pl",
    "tr", "vi", "th",
];

const MODEL_SIZES: &[(&str, u64)] = &[
    ("tiny", 75),
    ("tiny.en", 75),
    ("base", 142),
    ("base.en", 142),
    ("small", 466),
    ("small.en", 466),
    ("medium", 1500),
    ("medium.en", 1500),
    ("large-v1", 2900),
    ("large-v2", 2900),
    ("large-v3", 2900),
    ("large-v3-turbo", 1600),
];

struct DownloadState {
    model_name: String,
    progress: Arc<std::sync::Mutex<f32>>,
    done: Arc<AtomicBool>,
    error: Arc<std::sync::Mutex<Option<String>>>,
}

struct ApiTestState {
    done: Arc<AtomicBool>,
    error: Arc<std::sync::Mutex<Option<String>>>,
    models: Arc<std::sync::Mutex<Vec<String>>>,
}

pub struct SettingsApp {
    config: Config,
    daemon_running: Arc<AtomicBool>,
    daemon_ui_state: Arc<AtomicU8>,
    daemon_cmd_tx: Option<Sender<DaemonCommand>>,
    daemon_handle: Option<JoinHandle<()>>,
    downloaded_models: Vec<String>,
    status_msg: String,
    download: Option<DownloadState>,
    needs_input_group: bool,
    needs_relogin: bool,
    // Hotkey recorder
    hotkey_string: String,
    recording_hotkey: bool,
    record_rx: Option<Receiver<String>>,
    // Indices for combo boxes
    model_idx: usize,
    language_idx: usize,
    // API transcription fields
    api_url: String,
    api_key: String,
    api_model: String,
    // API test state
    api_test: Option<ApiTestState>,
    api_fetched_models: Vec<String>,
    api_model_idx: usize,
    api_tested_url: String,
    api_tested_key: String,
    // Scan results: (model_name, source_path, size_mb)
    scan_results: Vec<(String, std::path::PathBuf, u64)>,
}

fn user_in_input_group() -> bool {
    std::process::Command::new("id")
        .arg("-Gn")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("input"))
        .unwrap_or(false)
}

impl SettingsApp {
    pub fn new(config: Config) -> Self {
        let downloaded = model::list_downloaded();
        let needs_input_group = !user_in_input_group();

        let hotkey_string = config.hotkey.key.clone();

        let model_idx = if config.model.path.is_empty() {
            downloaded.iter().position(|m| m == "base.en").unwrap_or(0)
        } else {
            downloaded
                .iter()
                .enumerate()
                .find(|(_, m)| {
                    model::find_model(m)
                        .map(|info| config.model.path.contains(info.filename))
                        .unwrap_or(false)
                })
                .map(|(i, _)| i)
                .unwrap_or(0)
        };

        let language_idx = LANGUAGE_OPTIONS
            .iter()
            .position(|&l| l == config.model.language)
            .unwrap_or(0);

        let api_url = config.transcription.api_url.clone();
        let api_key = config.transcription.api_key.clone();
        let api_model = config.transcription.api_model.clone();

        SettingsApp {
            config,
            daemon_running: Arc::new(AtomicBool::new(false)),
            daemon_ui_state: Arc::new(AtomicU8::new(popup::state_to_u8(State::Idle))),
            daemon_cmd_tx: None,
            daemon_handle: None,
            downloaded_models: downloaded,
            status_msg: String::new(),
            download: None,
            needs_input_group,
            needs_relogin: false,
            hotkey_string,
            recording_hotkey: false,
            record_rx: None,
            model_idx,
            language_idx,
            api_url,
            api_key,
            api_model,
            api_test: None,
            api_fetched_models: Vec::new(),
            api_model_idx: 0,
            api_tested_url: String::new(),
            api_tested_key: String::new(),
            scan_results: Vec::new(),
        }
    }

    fn start_daemon(&mut self) {
        if self.daemon_running.load(Ordering::SeqCst) {
            return;
        }

        let (cmd_tx, cmd_rx) = crossbeam_channel::unbounded();

        match daemon::start_background(
            self.config.clone(),
            cmd_rx,
            self.daemon_running.clone(),
            self.daemon_ui_state.clone(),
        ) {
            Ok(handle) => {
                self.daemon_cmd_tx = Some(cmd_tx);
                self.daemon_handle = Some(handle);
                self.status_msg = String::new();
                info!("Daemon started from GUI");
            }
            Err(e) => {
                self.status_msg = format!("Failed to start: {}", e);
                error!("Failed to start daemon: {}", e);
            }
        }
    }

    fn stop_daemon(&mut self) {
        if let Some(tx) = self.daemon_cmd_tx.take() {
            let _ = tx.send(DaemonCommand::Stop);
        }
        if let Some(handle) = self.daemon_handle.take() {
            let _ = handle.join();
        }
        self.daemon_running.store(false, Ordering::SeqCst);
        self.status_msg = String::new();
        info!("Daemon stopped from GUI");
    }

    fn save_config(&mut self) {
        self.config.hotkey.key = self.hotkey_string.clone();

        if self.config.transcription.mode == TranscriptionMode::Local {
            if !self.downloaded_models.is_empty() && self.model_idx < self.downloaded_models.len() {
                let model_name = &self.downloaded_models[self.model_idx];
                if let Some(info) = model::find_model(model_name) {
                    let path = crate::config::models_dir().join(info.filename);
                    self.config.model.path = path.to_string_lossy().to_string();
                }
            }
        }

        self.config.transcription.api_url = self.api_url.clone();
        self.config.transcription.api_key = self.api_key.clone();
        self.config.transcription.api_model = self.api_model.clone();
        self.config.model.language = LANGUAGE_OPTIONS[self.language_idx].to_string();

        match self.config.save() {
            Ok(()) => {
                self.status_msg = "Settings saved.".to_string();
                info!("Config saved");

                if self.daemon_running.load(Ordering::SeqCst) {
                    self.stop_daemon();
                    self.start_daemon();
                    if self.status_msg.is_empty() {
                        self.status_msg = "Settings saved. Daemon restarted.".to_string();
                    }
                }
            }
            Err(e) => {
                self.status_msg = format!("Save failed: {}", e);
                error!("Config save failed: {}", e);
            }
        }
    }

    fn start_model_download(&mut self, model_name: String) {
        let progress = Arc::new(std::sync::Mutex::new(0.0f32));
        let done = Arc::new(AtomicBool::new(false));
        let err = Arc::new(std::sync::Mutex::new(None::<String>));

        let progress_clone = progress.clone();
        let done_clone = done.clone();
        let err_clone = err.clone();
        let name = model_name.clone();

        thread::spawn(move || {
            match model::find_model(&name) {
                Some(info) => {
                    let dest = crate::config::models_dir().join(info.filename);
                    if let Some(parent) = dest.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }

                    let rt = tokio::runtime::Runtime::new().unwrap();
                    let result = rt.block_on(download_with_progress(
                        info.url,
                        &dest,
                        progress_clone.clone(),
                    ));

                    if let Err(e) = result {
                        *err_clone.lock().unwrap() = Some(format!("{}", e));
                    }
                }
                None => {
                    *err_clone.lock().unwrap() = Some(format!("Unknown model: {}", name));
                }
            }
            done_clone.store(true, Ordering::SeqCst);
        });

        self.download = Some(DownloadState {
            model_name,
            progress,
            done,
            error: err,
        });
    }

    fn fix_input_permissions(&mut self) {
        let user = std::env::var("USER").unwrap_or_else(|_| "unknown".to_string());
        let result = std::process::Command::new("pkexec")
            .args(["usermod", "-aG", "input", &user])
            .status();

        match result {
            Ok(status) if status.success() => {
                self.needs_input_group = false;
                self.needs_relogin = true;
                self.status_msg = String::new();
                info!("Added user to input group");
            }
            Ok(_) => {
                self.status_msg = "Permission denied or cancelled.".to_string();
            }
            Err(e) => {
                self.status_msg = format!("Failed to run pkexec: {}", e);
                error!("pkexec failed: {}", e);
            }
        }
    }

    fn refresh_models(&mut self) {
        self.downloaded_models = model::list_downloaded();
        if self.model_idx >= self.downloaded_models.len() {
            self.model_idx = 0;
        }
    }

    fn start_api_test(&mut self) {
        let done = Arc::new(AtomicBool::new(false));
        let error = Arc::new(std::sync::Mutex::new(None::<String>));
        let models = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));

        let done_c = done.clone();
        let error_c = error.clone();
        let models_c = models.clone();
        let url = self.api_url.clone();
        let key = self.api_key.clone();

        self.api_tested_url = url.clone();
        self.api_tested_key = key.clone();

        thread::spawn(move || {
            match fetch_api_models(&url, &key) {
                Ok(list) => {
                    *models_c.lock().unwrap() = list;
                }
                Err(e) => {
                    *error_c.lock().unwrap() = Some(format!("{}", e));
                }
            }
            done_c.store(true, Ordering::SeqCst);
        });

        self.api_test = Some(ApiTestState {
            done,
            error,
            models,
        });
    }

    fn start_recording_hotkey(&mut self) {
        match hotkey::record_hotkey() {
            Ok(rx) => {
                self.record_rx = Some(rx);
                self.recording_hotkey = true;
            }
            Err(e) => {
                self.status_msg = format!("Can't record hotkey: {}", e);
                error!("record_hotkey failed: {}", e);
            }
        }
    }
}

async fn download_with_progress(
    url: &str,
    dest: &std::path::Path,
    progress: Arc<std::sync::Mutex<f32>>,
) -> anyhow::Result<()> {
    use futures_util::StreamExt;
    use tokio::io::AsyncWriteExt;

    let client = reqwest::Client::new();
    let response = client.get(url).send().await?;
    let total = response.content_length().unwrap_or(0);
    let mut downloaded: u64 = 0;

    let mut stream = response.bytes_stream();
    let mut file = tokio::fs::File::create(dest).await?;

    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        file.write_all(&chunk).await?;
        downloaded += chunk.len() as u64;
        if total > 0 {
            *progress.lock().unwrap() = downloaded as f32 / total as f32;
        }
    }

    *progress.lock().unwrap() = 1.0;
    Ok(())
}

fn fetch_api_models(base_url: &str, api_key: &str) -> anyhow::Result<Vec<String>> {
    let url = format!("{}/v1/models", base_url.trim_end_matches('/'));

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()?;

    let response = match client
        .get(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .send()
    {
        Ok(r) => r,
        Err(e) => {
            if e.is_connect() || e.is_timeout() {
                anyhow::bail!("Cannot reach API — check the URL");
            }
            anyhow::bail!("Request failed: {}", e);
        }
    };

    let status = response.status();
    match status.as_u16() {
        200 => {
            let body: serde_json::Value = response.json().map_err(|e| {
                anyhow::anyhow!("Failed to parse response: {}", e)
            })?;

            let data = body.get("data").and_then(|d| d.as_array());
            let mut models: Vec<String> = match data {
                Some(arr) => arr
                    .iter()
                    .filter_map(|m| {
                        m.get("id")
                            .and_then(|id| id.as_str())
                            .map(|s| s.to_string())
                    })
                    .collect(),
                None => Vec::new(),
            };

            // Try to filter to whisper/audio/speech related models
            let audio_models: Vec<String> = models
                .iter()
                .filter(|m: &&String| {
                    let lower = m.to_lowercase();
                    lower.contains("whisper")
                        || lower.contains("audio")
                        || lower.contains("speech")
                        || lower.contains("transcri")
                })
                .cloned()
                .collect();

            if !audio_models.is_empty() {
                models = audio_models;
            }

            models.sort();
            Ok(models)
        }
        401 | 403 => {
            anyhow::bail!("Authentication failed — check your API key");
        }
        404 => {
            // API doesn't support /v1/models — connection is valid though
            Ok(vec![])
        }
        _ => {
            let body_text = response
                .text()
                .unwrap_or_default();
            let truncated = if body_text.len() > 200 {
                format!("{}...", &body_text[..200])
            } else {
                body_text
            };
            anyhow::bail!("HTTP {}: {}", status.as_u16(), truncated);
        }
    }
}

impl eframe::App for SettingsApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Check if download finished
        let dl_finished = self
            .download
            .as_ref()
            .map(|dl| dl.done.load(Ordering::SeqCst))
            .unwrap_or(false);

        if dl_finished {
            let dl = self.download.take().unwrap();
            let err_msg = dl.error.lock().unwrap().take();
            if let Some(err) = err_msg {
                self.status_msg = format!("Download failed: {}", err);
            } else {
                self.status_msg = format!("Downloaded {} successfully.", dl.model_name);
                self.refresh_models();
            }
        }

        // Check if API test finished
        let api_test_finished = self
            .api_test
            .as_ref()
            .map(|t| t.done.load(Ordering::SeqCst))
            .unwrap_or(false);

        if api_test_finished {
            let test = self.api_test.take().unwrap();
            let err_msg = test.error.lock().unwrap().take();
            if let Some(err) = err_msg {
                self.status_msg = format!("API test failed: {}", err);
                self.api_fetched_models.clear();
            } else {
                let fetched = test.models.lock().unwrap().clone();
                if fetched.is_empty() {
                    self.status_msg =
                        "API connected. Enter model name manually.".to_string();
                    self.api_fetched_models.clear();
                } else {
                    let count = fetched.len();
                    self.status_msg =
                        format!("API connected — {} model{} loaded.", count, if count == 1 { "" } else { "s" });
                    self.api_fetched_models = fetched;
                    // Pre-select saved model if it exists in the list
                    self.api_model_idx = self
                        .api_fetched_models
                        .iter()
                        .position(|m| m == &self.api_model)
                        .unwrap_or(0);
                }
            }
        }

        // Invalidate fetched models if URL or key changed since last test
        if !self.api_fetched_models.is_empty()
            && (self.api_url != self.api_tested_url || self.api_key != self.api_tested_key)
        {
            self.api_fetched_models.clear();
            self.api_model_idx = 0;
        }

        // Check evdev hotkey recording result
        if self.recording_hotkey {
            if let Some(ref rx) = self.record_rx {
                if let Ok(combo) = rx.try_recv() {
                    if combo == "Escape" {
                        // Esc cancels recording
                    } else {
                        self.hotkey_string = combo;
                    }
                    self.recording_hotkey = false;
                    self.record_rx = None;
                }
            }
        }

        let is_running = self.daemon_running.load(Ordering::SeqCst);
        let daemon_state = self.daemon_ui_state.load(Ordering::Relaxed);

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.spacing_mut().item_spacing.y = 8.0;

            // ── Status Bar ──
            ui.horizontal(|ui| {
                let (dot_color, status_text) = if is_running {
                    match daemon_state {
                        1 => (egui::Color32::from_rgb(255, 100, 100), "Recording..."),
                        2 => (egui::Color32::from_rgb(100, 150, 255), "Transcribing..."),
                        _ => (egui::Color32::from_rgb(80, 200, 80), "Running"),
                    }
                } else {
                    (egui::Color32::from_rgb(180, 60, 60), "Stopped")
                };

                let (rect, _) = ui.allocate_exact_size(
                    egui::vec2(12.0, 12.0),
                    egui::Sense::hover(),
                );
                ui.painter().circle_filled(rect.center(), 6.0, dot_color);

                ui.label(egui::RichText::new(status_text).size(16.0).strong());

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if is_running {
                        if ui.button("Stop").clicked() {
                            self.stop_daemon();
                        }
                    } else {
                        let can_start = if self.config.transcription.mode == TranscriptionMode::Api {
                            !self.api_url.is_empty() && !self.api_key.is_empty()
                        } else {
                            !self.downloaded_models.is_empty()
                        };
                        ui.add_enabled_ui(can_start, |ui| {
                            if ui.button("Start").clicked() {
                                self.start_daemon();
                            }
                        });
                    }
                });
            });

            // ── Permission banner ──
            if self.needs_input_group {
                ui.add_space(4.0);
                egui::Frame::default()
                    .fill(egui::Color32::from_rgb(80, 50, 20))
                    .rounding(6.0)
                    .inner_margin(8.0)
                    .show(ui, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(
                                egui::RichText::new("Keyboard access required.")
                                    .color(egui::Color32::from_rgb(255, 200, 100))
                                    .strong(),
                            );
                            if ui.button("Fix Permissions").clicked() {
                                self.fix_input_permissions();
                            }
                        });
                    });
            }

            if self.needs_relogin {
                ui.add_space(4.0);
                egui::Frame::default()
                    .fill(egui::Color32::from_rgb(20, 60, 30))
                    .rounding(6.0)
                    .inner_margin(8.0)
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new("Done! Log out and back in, then restart the app.")
                                .color(egui::Color32::from_rgb(100, 220, 120)),
                        );
                    });
            }

            ui.separator();

            // ── Settings ──
            ui.label(egui::RichText::new("Settings").size(14.0).strong());

            let is_api = self.config.transcription.mode == TranscriptionMode::Api;

            egui::Grid::new("settings_grid")
                .num_columns(2)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    // Hotkey — recorder button
                    ui.label("Hotkey:");
                    if self.recording_hotkey {
                        let btn = ui.button(
                            egui::RichText::new("  Press a key combo...  ")
                                .color(egui::Color32::from_rgb(255, 200, 100)),
                        );
                        if btn.clicked() {
                            self.recording_hotkey = false;
                            self.record_rx = None;
                        }
                    } else {
                        let label = format!("  {}  ", self.hotkey_string);
                        if ui.button(&label).clicked() {
                            self.start_recording_hotkey();
                        }
                    }
                    ui.end_row();

                    // Mode toggle
                    ui.label("Mode:");
                    ui.horizontal(|ui| {
                        ui.radio_value(
                            &mut self.config.transcription.mode,
                            TranscriptionMode::Local,
                            "Local",
                        );
                        ui.radio_value(
                            &mut self.config.transcription.mode,
                            TranscriptionMode::Api,
                            "API",
                        );
                    });
                    ui.end_row();

                    if is_api {
                        // API fields
                        ui.label("API URL:");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.api_url)
                                .desired_width(200.0)
                                .hint_text("https://api.groq.com/openai"),
                        );
                        ui.end_row();

                        ui.label("API Key:");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.api_key)
                                .desired_width(200.0)
                                .password(true),
                        );
                        ui.end_row();

                        // Test API button
                        ui.label("");
                        ui.horizontal(|ui| {
                            let testing = self.api_test.is_some();
                            let can_test = !self.api_url.is_empty()
                                && !self.api_key.is_empty()
                                && !testing;
                            ui.add_enabled_ui(can_test, |ui| {
                                if ui.button("Test API").clicked() {
                                    self.start_api_test();
                                }
                            });
                            if testing {
                                ui.spinner();
                            }
                        });
                        ui.end_row();

                        // Model — dropdown if models fetched, text input otherwise
                        ui.label("Model:");
                        if !self.api_fetched_models.is_empty() {
                            let changed = egui::ComboBox::from_id_salt("api_model_combo")
                                .selected_text(
                                    self.api_fetched_models
                                        .get(self.api_model_idx)
                                        .map(|s| s.as_str())
                                        .unwrap_or("—"),
                                )
                                .width(200.0)
                                .show_ui(ui, |ui| {
                                    let mut changed = false;
                                    for (i, name) in
                                        self.api_fetched_models.iter().enumerate()
                                    {
                                        if ui
                                            .selectable_value(
                                                &mut self.api_model_idx,
                                                i,
                                                name.as_str(),
                                            )
                                            .changed()
                                        {
                                            changed = true;
                                        }
                                    }
                                    changed
                                });
                            // Sync selected model back to the string field
                            if let Some(selected) =
                                self.api_fetched_models.get(self.api_model_idx)
                            {
                                self.api_model = selected.clone();
                            }
                            let _ = changed;
                        } else {
                            ui.add(
                                egui::TextEdit::singleline(&mut self.api_model)
                                    .desired_width(200.0)
                                    .hint_text("whisper-large-v3"),
                            );
                        }
                        ui.end_row();
                    } else {
                        // Local model selector
                        ui.label("Model:");
                        if self.downloaded_models.is_empty() {
                            if self.download.is_some() {
                                let progress = self
                                    .download
                                    .as_ref()
                                    .unwrap()
                                    .progress
                                    .lock()
                                    .unwrap()
                                    .clone();
                                ui.add(
                                    egui::ProgressBar::new(progress)
                                        .text(format!(
                                            "Downloading {}...",
                                            self.download.as_ref().unwrap().model_name
                                        ))
                                        .desired_width(200.0),
                                );
                            } else if ui.button("No models — click to download base.en").clicked() {
                                self.start_model_download("base.en".to_string());
                            }
                        } else {
                            egui::ComboBox::from_id_salt("model_combo")
                                .selected_text(
                                    self.downloaded_models
                                        .get(self.model_idx)
                                        .map(|s| s.as_str())
                                        .unwrap_or("—"),
                                )
                                .show_ui(ui, |ui| {
                                    for (i, name) in self.downloaded_models.iter().enumerate() {
                                        ui.selectable_value(&mut self.model_idx, i, name.as_str());
                                    }
                                });
                        }
                        ui.end_row();
                    }

                    // Language
                    ui.label("Language:");
                    egui::ComboBox::from_id_salt("language_combo")
                        .selected_text(LANGUAGE_OPTIONS[self.language_idx])
                        .show_ui(ui, |ui| {
                            for (i, lang) in LANGUAGE_OPTIONS.iter().enumerate() {
                                ui.selectable_value(&mut self.language_idx, i, *lang);
                            }
                        });
                    ui.end_row();

                    // Auto-paste
                    ui.label("Auto-paste:");
                    ui.checkbox(&mut self.config.behavior.auto_paste, "");
                    ui.end_row();
                });

            ui.add_space(4.0);

            // ── Scan for existing models (local mode only) ──
            if self.config.transcription.mode == TranscriptionMode::Local
                && self.download.is_none()
            {
                ui.collapsing("Scan for existing models", |ui| {
                    if ui.button("Scan known directories").clicked() {
                        let found = model::scan_for_models();
                        self.scan_results = found
                            .into_iter()
                            .map(|m| (m.name, m.path, m.size_mb))
                            .collect();
                        if self.scan_results.is_empty() {
                            self.status_msg = "No new models found on disk.".to_string();
                        }
                    }
                    if !self.scan_results.is_empty() {
                        ui.add_space(4.0);
                        let mut to_import = None;
                        for (i, (name, path, size_mb)) in self.scan_results.iter().enumerate() {
                            ui.horizontal(|ui| {
                                ui.label(format!("{} ({}MB)", name, size_mb));
                                if ui.small_button("Link").clicked() {
                                    to_import = Some(i);
                                }
                            });
                            ui.label(
                                egui::RichText::new(format!("  {}", path.display()))
                                    .size(11.0)
                                    .color(egui::Color32::GRAY),
                            );
                        }
                        if let Some(idx) = to_import {
                            let (name, path, _) = &self.scan_results[idx];
                            match model::import_model(path, name) {
                                Ok(()) => {
                                    self.status_msg = format!("Linked {} ✓", name);
                                    self.scan_results.remove(idx);
                                    self.refresh_models();
                                }
                                Err(e) => {
                                    self.status_msg = format!("Link failed: {}", e);
                                }
                            }
                        }
                    }
                });
            }

            // ── Download extra models (local mode only) ──
            if self.config.transcription.mode == TranscriptionMode::Local
                && !self.downloaded_models.is_empty()
                && self.download.is_none()
            {
                ui.collapsing("Download more models", |ui| {
                    for &(name, size_mb) in MODEL_SIZES {
                        if !self.downloaded_models.iter().any(|d| d == name) {
                            ui.horizontal(|ui| {
                                ui.label(format!("{} (~{}MB)", name, size_mb));
                                if ui.small_button("Download").clicked() {
                                    self.start_model_download(name.to_string());
                                }
                            });
                        }
                    }
                });
            }

            // Download progress (when models already exist but downloading extra)
            if let Some(ref dl) = self.download {
                if !self.downloaded_models.is_empty() {
                    let progress = dl.progress.lock().unwrap().clone();
                    ui.add(
                        egui::ProgressBar::new(progress)
                            .text(format!("Downloading {}...", dl.model_name))
                            .desired_width(ui.available_width()),
                    );
                }
            }

            ui.add_space(4.0);

            // ── Save Button ──
            ui.vertical_centered(|ui| {
                if ui.button("  Save  ").clicked() {
                    self.save_config();
                }
            });

            // ── Status message ──
            if !self.status_msg.is_empty() {
                ui.add_space(4.0);
                let color = if self.status_msg.contains("failed") || self.status_msg.contains("Failed") {
                    egui::Color32::from_rgb(220, 80, 80)
                } else {
                    egui::Color32::from_rgb(80, 200, 80)
                };
                ui.colored_label(color, &self.status_msg);
            }
        });

        // Repaint periodically to update status or during recording
        if is_running || self.download.is_some() || self.recording_hotkey || self.api_test.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
    }
}

/// Launch the settings GUI window. This blocks until the window is closed.
pub fn run_settings_gui(config: Config) -> anyhow::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([340.0, 380.0])
            .with_min_inner_size([300.0, 320.0])
            .with_resizable(true),
        ..Default::default()
    };

    eframe::run_native(
        "typingsucks",
        options,
        Box::new(move |cc| {
            let mut visuals = egui::Visuals::dark();
            visuals.override_text_color = Some(egui::Color32::WHITE);
            cc.egui_ctx.set_visuals(visuals);

            let mut fonts = egui::FontDefinitions::default();
            for path in &[
                "/usr/share/fonts/noto/NotoSans-Regular.ttf",
                "/usr/share/fonts/TTF/DejaVuSans.ttf",
                "/usr/share/fonts/gnu-free/FreeSans.ttf",
            ] {
                if let Ok(data) = std::fs::read(path) {
                    fonts.font_data.insert(
                        "system_font".to_owned(),
                        egui::FontData::from_owned(data),
                    );
                    fonts
                        .families
                        .get_mut(&egui::FontFamily::Proportional)
                        .unwrap()
                        .insert(0, "system_font".to_owned());
                    fonts
                        .families
                        .get_mut(&egui::FontFamily::Monospace)
                        .unwrap()
                        .insert(0, "system_font".to_owned());
                    break;
                }
            }
            cc.egui_ctx.set_fonts(fonts);

            Ok(Box::new(SettingsApp::new(config)))
        }),
    )
    .map_err(|e| anyhow::anyhow!("GUI error: {}", e))?;

    Ok(())
}
