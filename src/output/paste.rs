use anyhow::Result;
use tracing::{info, warn};

/// Simulate paste. Uses Ctrl+Shift+V for terminals, Ctrl+V for everything else.
pub fn simulate_paste() -> Result<()> {
    let use_terminal_paste = is_terminal_focused();
    match try_enigo_paste(use_terminal_paste) {
        Ok(()) => {
            if use_terminal_paste {
                info!("Pasted via Ctrl+Shift+V (terminal)");
            } else {
                info!("Pasted via Ctrl+V");
            }
            Ok(())
        }
        Err(e) => {
            warn!("enigo paste failed ({}), text is in clipboard — paste manually", e);
            Ok(())
        }
    }
}

/// Check if the currently focused window is a terminal emulator
fn is_terminal_focused() -> bool {
    // Try wlrctl (Wayland) first, fall back to xdotool (X11)
    if let Some(class) = get_focused_window_class() {
        let class_lower = class.to_lowercase();
        let terminals = [
            "konsole",
            "alacritty",
            "kitty",
            "foot",
            "wezterm",
            "gnome-terminal",
            "xfce4-terminal",
            "terminator",
            "tilix",
            "urxvt",
            "xterm",
            "st-256color",
            "sakura",
            "terminology",
            "yakuake",
            "guake",
            "cool-retro-term",
            "blackbox",
            "contour",
            "rio",
            "ghostty",
        ];
        return terminals.iter().any(|t| class_lower.contains(t));
    }
    false
}

fn get_focused_window_class() -> Option<String> {
    // Try kdotool (KDE Wayland)
    if let Ok(output) = std::process::Command::new("kdotool")
        .args(["getactivewindow", "getwindowclassname"])
        .output()
    {
        if output.status.success() {
            let class = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !class.is_empty() {
                return Some(class);
            }
        }
    }

    // Try xdotool (X11 / XWayland)
    if let Ok(output) = std::process::Command::new("xdotool")
        .args(["getactivewindow", "getwindowclassname"])
        .output()
    {
        if output.status.success() {
            let class = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !class.is_empty() {
                return Some(class);
            }
        }
    }

    None
}

fn try_enigo_paste(terminal: bool) -> Result<()> {
    use enigo::{
        Direction::{Click, Press, Release},
        Enigo, Key, Keyboard, Settings,
    };

    let mut enigo = Enigo::new(&Settings::default())
        .map_err(|e| anyhow::anyhow!("Failed to create enigo: {:?}", e))?;

    // Small delay to ensure the key release from hotkey doesn't interfere
    std::thread::sleep(std::time::Duration::from_millis(50));

    if terminal {
        // Ctrl+Shift+V for terminals
        enigo
            .key(Key::Control, Press)
            .map_err(|e| anyhow::anyhow!("ctrl press: {:?}", e))?;
        enigo
            .key(Key::Shift, Press)
            .map_err(|e| anyhow::anyhow!("shift press: {:?}", e))?;
        enigo
            .key(Key::Unicode('v'), Click)
            .map_err(|e| anyhow::anyhow!("v click: {:?}", e))?;
        enigo
            .key(Key::Shift, Release)
            .map_err(|e| anyhow::anyhow!("shift release: {:?}", e))?;
        enigo
            .key(Key::Control, Release)
            .map_err(|e| anyhow::anyhow!("ctrl release: {:?}", e))?;
    } else {
        // Ctrl+V for everything else
        enigo
            .key(Key::Control, Press)
            .map_err(|e| anyhow::anyhow!("ctrl press: {:?}", e))?;
        enigo
            .key(Key::Unicode('v'), Click)
            .map_err(|e| anyhow::anyhow!("v click: {:?}", e))?;
        enigo
            .key(Key::Control, Release)
            .map_err(|e| anyhow::anyhow!("ctrl release: {:?}", e))?;
    }

    Ok(())
}
