use anyhow::{Context, Result};
use arboard::Clipboard;

pub fn set_text(text: &str) -> Result<()> {
    let mut clipboard = Clipboard::new().context("Failed to access clipboard")?;
    clipboard
        .set_text(text)
        .context("Failed to set clipboard text")?;
    Ok(())
}
