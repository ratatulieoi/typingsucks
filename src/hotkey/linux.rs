use super::HotkeyEvent;
use anyhow::{Context, Result};
use crossbeam_channel::{Receiver, Sender};
use evdev::{Device, InputEventKind, Key};
use std::fs;
use std::thread;
use tracing::{debug, info};

/// A hotkey can be a single key or modifier+key combo
#[derive(Debug, Clone, Copy)]
pub struct Hotkey {
    pub modifier: Option<Key>,
    pub key: Key,
}

pub struct HotkeyListener {
    rx: Receiver<HotkeyEvent>,
}

impl HotkeyListener {
    pub fn new(key_name: &str) -> Result<Self> {
        let hotkey = parse_hotkey(key_name)
            .with_context(|| format!("Unknown key name: {}", key_name))?;

        let (tx, rx) = crossbeam_channel::unbounded();

        // Find all keyboard devices
        let devices = find_keyboard_devices()?;
        if devices.is_empty() {
            anyhow::bail!(
                "No keyboard devices found. Make sure your user is in the 'input' group:\n  \
                 sudo usermod -aG input $USER\n  \
                 Then log out and back in."
            );
        }

        for path in devices {
            let tx = tx.clone();
            let hotkey = hotkey;
            thread::spawn(move || {
                if let Err(e) = listen_device(&path, hotkey, &tx) {
                    debug!("Device listener ended for {}: {}", path, e);
                }
            });
        }

        Ok(HotkeyListener { rx })
    }

    pub fn recv(&self) -> Result<HotkeyEvent> {
        self.rx.recv().context("Hotkey channel closed")
    }

    #[allow(dead_code)]
    pub fn try_recv(&self) -> Option<HotkeyEvent> {
        self.rx.try_recv().ok()
    }
}

fn listen_device(path: &str, hotkey: Hotkey, tx: &Sender<HotkeyEvent>) -> Result<()> {
    let mut device = Device::open(path)
        .with_context(|| format!("Failed to open {}", path))?;

    info!("Listening on: {} ({})", path, device.name().unwrap_or("unknown"));

    let mut modifier_held = false;
    let mut combo_active = false;

    loop {
        for event in device.fetch_events()? {
            if let InputEventKind::Key(key) = event.kind() {
                let value = event.value(); // 1=down, 0=up, 2=repeat

                if let Some(mod_key) = hotkey.modifier {
                    // Combo mode: modifier+key
                    // Track both left and right variants of the modifier
                    let is_modifier = key == mod_key || is_same_modifier(key, mod_key);
                    let is_target = key == hotkey.key;

                    if is_modifier {
                        modifier_held = value >= 1;
                        // If modifier released while combo was active, release
                        if !modifier_held && combo_active {
                            combo_active = false;
                            debug!("Combo released (modifier up)");
                            let _ = tx.send(HotkeyEvent::Released);
                        }
                    } else if is_target {
                        if value == 1 && modifier_held && !combo_active {
                            combo_active = true;
                            debug!("Combo pressed");
                            let _ = tx.send(HotkeyEvent::Pressed);
                        } else if value == 0 && combo_active {
                            combo_active = false;
                            debug!("Combo released (key up)");
                            let _ = tx.send(HotkeyEvent::Released);
                        }
                    }
                } else {
                    // Single key mode (original behavior)
                    if key == hotkey.key {
                        let ev = match value {
                            1 => Some(HotkeyEvent::Pressed),
                            0 => Some(HotkeyEvent::Released),
                            _ => None,
                        };
                        if let Some(ev) = ev {
                            debug!("Key event: {:?}", ev);
                            let _ = tx.send(ev);
                        }
                    }
                }
            }
        }
    }
}

/// Check if two keys are the same modifier (left/right variants)
fn is_same_modifier(a: Key, b: Key) -> bool {
    let meta = [Key::KEY_LEFTMETA, Key::KEY_RIGHTMETA];
    let alt = [Key::KEY_LEFTALT, Key::KEY_RIGHTALT];
    let ctrl = [Key::KEY_LEFTCTRL, Key::KEY_RIGHTCTRL];
    let shift = [Key::KEY_LEFTSHIFT, Key::KEY_RIGHTSHIFT];

    for group in &[&meta[..], &alt[..], &ctrl[..], &shift[..]] {
        if group.contains(&a) && group.contains(&b) {
            return true;
        }
    }
    false
}

fn find_keyboard_devices() -> Result<Vec<String>> {
    let mut keyboards = Vec::new();
    let entries = fs::read_dir("/dev/input")
        .context("Cannot read /dev/input")?;

    for entry in entries.flatten() {
        let path = entry.path();
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        if !name.starts_with("event") {
            continue;
        }

        if let Ok(device) = Device::open(&path) {
            // Check if this device has keyboard keys
            if let Some(keys) = device.supported_keys() {
                if keys.contains(Key::KEY_A) && keys.contains(Key::KEY_Z) {
                    keyboards.push(path.to_string_lossy().to_string());
                }
            }
        }
    }

    Ok(keyboards)
}

/// Parse a hotkey string. Supports:
/// - Single keys: "ScrollLock", "F9", "Pause"
/// - Combos: "Meta+Z", "Ctrl+Space", "Alt+R", "Ctrl+Shift+Z"
/// For multi-modifier combos, the first modifier is used for evdev tracking.
fn parse_hotkey(name: &str) -> Option<Hotkey> {
    if name.contains('+') {
        let parts: Vec<&str> = name.split('+').map(|s| s.trim()).collect();
        if parts.len() >= 2 {
            // Last part is the key, everything before is modifiers
            let key = parse_key_name(parts.last().unwrap())?;
            // Use the first modifier for evdev tracking
            let modifier = parse_modifier(parts[0])?;
            return Some(Hotkey {
                modifier: Some(modifier),
                key,
            });
        }
        None
    } else {
        let key = parse_key_name(name)?;
        Some(Hotkey {
            modifier: None,
            key,
        })
    }
}

fn parse_modifier(name: &str) -> Option<Key> {
    match name.to_lowercase().as_str() {
        "meta" | "super" | "win" => Some(Key::KEY_LEFTMETA),
        "alt" => Some(Key::KEY_LEFTALT),
        "ctrl" | "control" => Some(Key::KEY_LEFTCTRL),
        "shift" => Some(Key::KEY_LEFTSHIFT),
        _ => None,
    }
}

fn parse_key_name(name: &str) -> Option<Key> {
    match name.to_lowercase().as_str() {
        "scrolllock" | "scroll_lock" => Some(Key::KEY_SCROLLLOCK),
        "pause" | "break" => Some(Key::KEY_PAUSE),
        "insert" => Some(Key::KEY_INSERT),
        "home" => Some(Key::KEY_HOME),
        "end" => Some(Key::KEY_END),
        "pageup" => Some(Key::KEY_PAGEUP),
        "pagedown" => Some(Key::KEY_PAGEDOWN),
        "space" => Some(Key::KEY_SPACE),
        "f1" => Some(Key::KEY_F1),
        "f2" => Some(Key::KEY_F2),
        "f3" => Some(Key::KEY_F3),
        "f4" => Some(Key::KEY_F4),
        "f5" => Some(Key::KEY_F5),
        "f6" => Some(Key::KEY_F6),
        "f7" => Some(Key::KEY_F7),
        "f8" => Some(Key::KEY_F8),
        "f9" => Some(Key::KEY_F9),
        "f10" => Some(Key::KEY_F10),
        "f11" => Some(Key::KEY_F11),
        "f12" => Some(Key::KEY_F12),
        "capslock" | "caps_lock" => Some(Key::KEY_CAPSLOCK),
        "numlock" | "num_lock" => Some(Key::KEY_NUMLOCK),
        "rightalt" | "right_alt" | "altgr" => Some(Key::KEY_RIGHTALT),
        "rightctrl" | "right_ctrl" => Some(Key::KEY_RIGHTCTRL),
        "leftmeta" | "super" | "super_l" => Some(Key::KEY_LEFTMETA),
        "rightmeta" | "super_r" => Some(Key::KEY_RIGHTMETA),
        "escape" | "esc" => Some(Key::KEY_ESC),
        "tab" => Some(Key::KEY_TAB),
        "minus" => Some(Key::KEY_MINUS),
        "semicolon" => Some(Key::KEY_SEMICOLON),
        // Number keys
        "0" => Some(Key::KEY_0),
        "1" => Some(Key::KEY_1),
        "2" => Some(Key::KEY_2),
        "3" => Some(Key::KEY_3),
        "4" => Some(Key::KEY_4),
        "5" => Some(Key::KEY_5),
        "6" => Some(Key::KEY_6),
        "7" => Some(Key::KEY_7),
        "8" => Some(Key::KEY_8),
        "9" => Some(Key::KEY_9),
        // Letter keys
        "a" => Some(Key::KEY_A),
        "b" => Some(Key::KEY_B),
        "c" => Some(Key::KEY_C),
        "d" => Some(Key::KEY_D),
        "e" => Some(Key::KEY_E),
        "f" => Some(Key::KEY_F),
        "g" => Some(Key::KEY_G),
        "h" => Some(Key::KEY_H),
        "i" => Some(Key::KEY_I),
        "j" => Some(Key::KEY_J),
        "k" => Some(Key::KEY_K),
        "l" => Some(Key::KEY_L),
        "m" => Some(Key::KEY_M),
        "n" => Some(Key::KEY_N),
        "o" => Some(Key::KEY_O),
        "p" => Some(Key::KEY_P),
        "q" => Some(Key::KEY_Q),
        "r" => Some(Key::KEY_R),
        "s" => Some(Key::KEY_S),
        "t" => Some(Key::KEY_T),
        "u" => Some(Key::KEY_U),
        "v" => Some(Key::KEY_V),
        "w" => Some(Key::KEY_W),
        "x" => Some(Key::KEY_X),
        "y" => Some(Key::KEY_Y),
        "z" => Some(Key::KEY_Z),
        _ => None,
    }
}

/// Convert an evdev Key to a display name
fn key_to_name(key: Key) -> Option<&'static str> {
    Some(match key {
        Key::KEY_SCROLLLOCK => "ScrollLock",
        Key::KEY_PAUSE => "Pause",
        Key::KEY_INSERT => "Insert",
        Key::KEY_HOME => "Home",
        Key::KEY_END => "End",
        Key::KEY_PAGEUP => "PageUp",
        Key::KEY_PAGEDOWN => "PageDown",
        Key::KEY_SPACE => "Space",
        Key::KEY_ESC => "Escape",
        Key::KEY_TAB => "Tab",
        Key::KEY_F1 => "F1",
        Key::KEY_F2 => "F2",
        Key::KEY_F3 => "F3",
        Key::KEY_F4 => "F4",
        Key::KEY_F5 => "F5",
        Key::KEY_F6 => "F6",
        Key::KEY_F7 => "F7",
        Key::KEY_F8 => "F8",
        Key::KEY_F9 => "F9",
        Key::KEY_F10 => "F10",
        Key::KEY_F11 => "F11",
        Key::KEY_F12 => "F12",
        Key::KEY_CAPSLOCK => "CapsLock",
        Key::KEY_NUMLOCK => "NumLock",
        Key::KEY_MINUS => "Minus",
        Key::KEY_SEMICOLON => "Semicolon",
        Key::KEY_0 => "0",
        Key::KEY_1 => "1",
        Key::KEY_2 => "2",
        Key::KEY_3 => "3",
        Key::KEY_4 => "4",
        Key::KEY_5 => "5",
        Key::KEY_6 => "6",
        Key::KEY_7 => "7",
        Key::KEY_8 => "8",
        Key::KEY_9 => "9",
        Key::KEY_A => "A",
        Key::KEY_B => "B",
        Key::KEY_C => "C",
        Key::KEY_D => "D",
        Key::KEY_E => "E",
        Key::KEY_F => "F",
        Key::KEY_G => "G",
        Key::KEY_H => "H",
        Key::KEY_I => "I",
        Key::KEY_J => "J",
        Key::KEY_K => "K",
        Key::KEY_L => "L",
        Key::KEY_M => "M",
        Key::KEY_N => "N",
        Key::KEY_O => "O",
        Key::KEY_P => "P",
        Key::KEY_Q => "Q",
        Key::KEY_R => "R",
        Key::KEY_S => "S",
        Key::KEY_T => "T",
        Key::KEY_U => "U",
        Key::KEY_V => "V",
        Key::KEY_W => "W",
        Key::KEY_X => "X",
        Key::KEY_Y => "Y",
        Key::KEY_Z => "Z",
        _ => return None,
    })
}

#[allow(dead_code)]
fn modifier_key_to_name(key: Key) -> Option<&'static str> {
    Some(match key {
        Key::KEY_LEFTMETA | Key::KEY_RIGHTMETA => "Meta",
        Key::KEY_LEFTALT | Key::KEY_RIGHTALT => "Alt",
        Key::KEY_LEFTCTRL | Key::KEY_RIGHTCTRL => "Ctrl",
        Key::KEY_LEFTSHIFT | Key::KEY_RIGHTSHIFT => "Shift",
        _ => return None,
    })
}

fn is_modifier_key(key: Key) -> bool {
    matches!(
        key,
        Key::KEY_LEFTMETA
            | Key::KEY_RIGHTMETA
            | Key::KEY_LEFTALT
            | Key::KEY_RIGHTALT
            | Key::KEY_LEFTCTRL
            | Key::KEY_RIGHTCTRL
            | Key::KEY_LEFTSHIFT
            | Key::KEY_RIGHTSHIFT
    )
}

/// Record a hotkey combo using evdev. Spawns background threads on all keyboard
/// devices that capture the next key combo pressed by the user. Returns a Receiver
/// that will receive the hotkey string (e.g. "Meta+Z") once captured.
pub fn record_hotkey() -> Result<Receiver<String>> {
    let devices = find_keyboard_devices()?;
    if devices.is_empty() {
        anyhow::bail!("No keyboard devices found");
    }

    let (result_tx, result_rx) = crossbeam_channel::bounded::<String>(1);

    // Listen on ALL keyboard devices — first one to capture wins
    for path in devices {
        let tx = result_tx.clone();
        thread::spawn(move || {
            if let Err(e) = record_from_device(&path, &tx) {
                debug!("Record hotkey error on {}: {}", path, e);
            }
        });
    }

    Ok(result_rx)
}

fn record_from_device(path: &str, tx: &Sender<String>) -> Result<()> {
    let mut device = Device::open(path)
        .with_context(|| format!("Failed to open {}", path))?;

    let mut held_modifiers: Vec<Key> = Vec::new();

    loop {
        for event in device.fetch_events()? {
            if let InputEventKind::Key(key) = event.kind() {
                let value = event.value();

                if is_modifier_key(key) {
                    if value == 1 {
                        // Modifier pressed — track it (dedup left/right)
                        if !held_modifiers.iter().any(|k| is_same_modifier(*k, key) || *k == key) {
                            held_modifiers.push(key);
                        }
                    } else if value == 0 {
                        // Modifier released
                        // If no non-modifier key was pressed, and this was a lone modifier,
                        // treat the modifier itself as the hotkey (e.g. just "ScrollLock")
                        held_modifiers.retain(|k| !is_same_modifier(*k, key) && *k != key);
                    }
                } else if value == 1 {
                    // Non-modifier key pressed — this completes the combo
                    let key_name = match key_to_name(key) {
                        Some(n) => n,
                        None => return Ok(()), // Unknown key, ignore
                    };

                    let mut parts: Vec<&str> = Vec::new();
                    // Add modifiers in a consistent order: Meta, Ctrl, Alt, Shift
                    let mod_order = [
                        (Key::KEY_LEFTMETA, "Meta"),
                        (Key::KEY_LEFTCTRL, "Ctrl"),
                        (Key::KEY_LEFTALT, "Alt"),
                        (Key::KEY_LEFTSHIFT, "Shift"),
                    ];
                    for (mod_key, mod_name) in &mod_order {
                        if held_modifiers.iter().any(|k| is_same_modifier(*k, *mod_key) || *k == *mod_key) {
                            parts.push(mod_name);
                        }
                    }
                    parts.push(key_name);

                    let combo = parts.join("+");
                    let _ = tx.send(combo);
                    return Ok(());
                }
            }
        }
    }
}
