pub mod download;

use crate::config::{self, Config};
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

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
    ModelInfo {
        name: "medium",
        filename: "ggml-medium.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-medium.bin",
        size_mb: 1500,
    },
    ModelInfo {
        name: "medium.en",
        filename: "ggml-medium.en.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-medium.en.bin",
        size_mb: 1500,
    },
    ModelInfo {
        name: "large-v1",
        filename: "ggml-large-v1.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v1.bin",
        size_mb: 2900,
    },
    ModelInfo {
        name: "large-v2",
        filename: "ggml-large-v2.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v2.bin",
        size_mb: 2900,
    },
    ModelInfo {
        name: "large-v3",
        filename: "ggml-large-v3.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3.bin",
        size_mb: 2900,
    },
    ModelInfo {
        name: "large-v3-turbo",
        filename: "ggml-large-v3-turbo.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo.bin",
        size_mb: 1600,
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
    let preferred = [
        "ggml-large-v3-turbo.bin",
        "ggml-large-v3.bin",
        "ggml-large-v2.bin",
        "ggml-large-v1.bin",
        "ggml-medium.en.bin",
        "ggml-medium.bin",
        "ggml-small.en.bin",
        "ggml-small.bin",
        "ggml-base.en.bin",
        "ggml-base.bin",
        "ggml-tiny.en.bin",
        "ggml-tiny.bin",
    ];

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
        .with_context(|| format!("Unknown model size: {}. Available: tiny, tiny.en, base, base.en, small, small.en, medium, medium.en, large-v1, large-v2, large-v3, large-v3-turbo", size))?;

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

/// Minimum file size in bytes for a valid model (50MB).
/// Smallest real model (ggml-tiny.bin) is ~75MB. This filters out test stubs.
const MIN_MODEL_SIZE: u64 = 50 * 1024 * 1024;

pub struct ScannedModel {
    pub name: String,
    pub path: PathBuf,
    pub size_mb: u64,
}

/// Known directories where whisper models might already exist.
fn scan_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(home) = dirs::home_dir() {
        dirs.push(home.join("whisper.cpp/models"));
        dirs.push(home.join(".cache/whisper"));
        dirs.push(home.join(".cache/whisper-cpp"));
    }
    dirs
}

/// Scan known directories for existing whisper model files.
/// Only checks known filenames in a few hardcoded directories.
/// Skips models already present in the app's models dir.
pub fn scan_for_models() -> Vec<ScannedModel> {
    let models_dir = config::models_dir();
    let mut found = Vec::new();

    for dir in scan_dirs() {
        if !dir.is_dir() {
            continue;
        }
        for model in MODELS {
            let path = dir.join(model.filename);
            if !path.is_file() {
                continue;
            }
            let dest = models_dir.join(model.filename);
            if dest.exists() {
                continue;
            }
            if let Ok(meta) = path.metadata() {
                if meta.len() >= MIN_MODEL_SIZE {
                    found.push(ScannedModel {
                        name: model.name.to_string(),
                        path,
                        size_mb: meta.len() / (1024 * 1024),
                    });
                }
            }
        }
    }

    found
}

/// Symlink an existing model file into the app's models directory.
pub fn import_model(source: &Path, model_name: &str) -> Result<()> {
    let model = find_model(model_name)
        .with_context(|| format!("Unknown model: {}", model_name))?;

    let dest = config::models_dir().join(model.filename);
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }

    if dest.exists() {
        anyhow::bail!("Model {} already exists at {}", model_name, dest.display());
    }

    std::os::unix::fs::symlink(source, &dest)
        .with_context(|| format!("Failed to symlink {} -> {}", source.display(), dest.display()))?;

    Ok(())
}
