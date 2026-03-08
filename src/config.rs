use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub enum TranscriptionMode {
    Local,
    Api,
}

impl Default for TranscriptionMode {
    fn default() -> Self {
        TranscriptionMode::Local
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TranscriptionConfig {
    #[serde(default)]
    pub mode: TranscriptionMode,
    #[serde(default)]
    pub api_url: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub api_model: String,
}

impl Default for TranscriptionConfig {
    fn default() -> Self {
        TranscriptionConfig {
            mode: TranscriptionMode::Local,
            api_url: String::new(),
            api_key: String::new(),
            api_model: String::new(),
        }
    }
}

fn default_transcription() -> TranscriptionConfig {
    TranscriptionConfig::default()
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Config {
    #[serde(default = "default_hotkey")]
    pub hotkey: HotkeyConfig,
    #[serde(default = "default_model")]
    pub model: ModelConfig,
    #[serde(default = "default_audio")]
    pub audio: AudioConfig,
    #[serde(default = "default_behavior")]
    pub behavior: BehaviorConfig,
    #[serde(default = "default_transcription")]
    pub transcription: TranscriptionConfig,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct HotkeyConfig {
    #[serde(default = "default_key")]
    pub key: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ModelConfig {
    #[serde(default)]
    pub path: String,
    #[serde(default = "default_language")]
    pub language: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AudioConfig {
    #[serde(default = "default_sample_rate")]
    pub sample_rate: u32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BehaviorConfig {
    #[serde(default = "default_auto_paste")]
    pub auto_paste: bool,
}

fn default_hotkey() -> HotkeyConfig {
    HotkeyConfig {
        key: default_key(),
    }
}
fn default_model() -> ModelConfig {
    ModelConfig {
        path: String::new(),
        language: default_language(),
    }
}
fn default_audio() -> AudioConfig {
    AudioConfig {
        sample_rate: default_sample_rate(),
    }
}
fn default_behavior() -> BehaviorConfig {
    BehaviorConfig {
        auto_paste: default_auto_paste(),
    }
}

fn default_key() -> String {
    "Meta+Z".to_string()
}
fn default_language() -> String {
    "en".to_string()
}
fn default_sample_rate() -> u32 {
    16000
}
fn default_auto_paste() -> bool {
    true
}

pub fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"))
        .join("typingsucks")
}

pub fn config_path() -> PathBuf {
    config_dir().join("config.toml")
}

pub fn data_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("~/.local/share"))
        .join("typingsucks")
}

pub fn models_dir() -> PathBuf {
    data_dir().join("models")
}

pub fn runtime_dir() -> PathBuf {
    dirs::runtime_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("typingsucks")
}

impl Config {
    pub fn load() -> Result<Self> {
        let path = config_path();
        if path.exists() {
            let content = std::fs::read_to_string(&path)
                .with_context(|| format!("Failed to read config: {}", path.display()))?;
            let cfg: Config =
                toml::from_str(&content).with_context(|| "Failed to parse config")?;
            Ok(cfg)
        } else {
            let cfg = Config::default();
            cfg.save()?;
            Ok(cfg)
        }
    }

    pub fn save(&self) -> Result<()> {
        let path = config_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(self)?;
        std::fs::write(&path, content)?;
        Ok(())
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            hotkey: default_hotkey(),
            model: default_model(),
            audio: default_audio(),
            behavior: default_behavior(),
            transcription: default_transcription(),
        }
    }
}
