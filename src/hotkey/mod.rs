#[cfg(target_os = "linux")]
pub mod linux;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyEvent {
    Pressed,
    Released,
}

#[cfg(target_os = "linux")]
pub use linux::HotkeyListener;

#[cfg(target_os = "linux")]
pub use linux::record_hotkey;
