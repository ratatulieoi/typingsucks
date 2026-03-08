pub mod download;

use crate::config::{self, Config};
use anyhow::{Context, Result};
use std::path::PathBuf;

pub struct ModelInfo {
    pub name: &'static str,
    pub filename: &'static str,
    pub url: &'static str,
    pub size_mb: u64,
}

pub const MODELS: &[ModelInfo] = &[
    ModelInfo {
        name: "tiny",
        filename: "ggml-tiny.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.bin",
        size_mb: 75,
    },
    ModelInfo {
        name: "tiny.en",
        filename: "ggml-tiny.en.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.en.bin",
        size_mb: 75,
    },
    ModelInfo {
        name: "base",
        filename: "ggml-base.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.bin",
        size_mb: 142,
    },
    ModelInfo {
        name: "base.en",
        filename: "ggml-base.en.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin",
        size_mb: 142,
    },
    ModelInfo {
        name: "small",
        filename: "ggml-small.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.bin",
        size_mb: 466,
    },
    ModelInfo {
        name: "small.en",
        filename: "ggml-small.en.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.en.bin",
        size_mb: 466,
    },
];

pub fn find_model(name: &str) -> Option<&'static ModelInfo> {
    MODELS.iter().find(|m| m.name == name)
}

pub fn resolve_model_path(cfg: &Config) -> Result<PathBuf> {
    if !cfg.model.path.is_empty() {
        let p = PathBuf::from(&cfg.model.path);
        if p.exists() {
            return Ok(p);
        }
        anyhow::bail!("Configured model path does not exist: {}", p.display());
    }

    // Try to find any downloaded model, preferring base.en > base > others
    let models_dir = config::models_dir();
    let preferred = ["ggml-base.en.bin", "ggml-base.bin", "ggml-tiny.en.bin", "ggml-tiny.bin", "ggml-small.en.bin", "ggml-small.bin"];

    for name in &preferred {
        let path = models_dir.join(name);
        if path.exists() {
            return Ok(path);
        }
    }

    anyhow::bail!(
        "No whisper model found. Download one first:\n  typingsucks model download base"
    )
}

pub fn download_model(size: &str) -> Result<()> {
    let model = find_model(size)
        .with_context(|| format!("Unknown model size: {}. Available: tiny, tiny.en, base, base.en, small, small.en", size))?;

    let dest = config::models_dir().join(model.filename);
    if dest.exists() {
        println!("Model already downloaded: {}", dest.display());
        return Ok(());
    }

    println!("Downloading {} (~{}MB)...", model.name, model.size_mb);
    download::download_file(model.url, &dest)?;
    println!("Saved to: {}", dest.display());
    Ok(())
}

/// Returns names of models that are already downloaded (e.g. ["base.en", "tiny"])
pub fn list_downloaded() -> Vec<String> {
    let models_dir = config::models_dir();
    MODELS
        .iter()
        .filter(|m| models_dir.join(m.filename).exists())
        .map(|m| m.name.to_string())
        .collect()
}

pub fn list_models() -> Result<()> {
    let models_dir = config::models_dir();
    println!("Available models:");
    for model in MODELS {
        let path = models_dir.join(model.filename);
        let status = if path.exists() { "✓ downloaded" } else { "  not downloaded" };
        println!("  {} ({:>3}MB) {}", model.name, model.size_mb, status);
    }
    Ok(())
}
