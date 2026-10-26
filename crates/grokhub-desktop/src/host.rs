//! What the app that hosts the desktop tools tells them: where its config
//! lives, its desktop switches, and the screen readers it owns. The cabin and
//! the Tauri shell each install one; until then the switches are off.

use std::path::PathBuf;

/// The app's desktop switches, read fresh on every call.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Settings {
    /// The Settings desktop control switch.
    pub desktop_control: bool,
    /// The `cuaDriver` flag (Linux only).
    pub cua_driver: bool,
    /// `cuaDriverPath`, empty for PATH.
    pub cua_driver_path: String,
}

#[derive(Clone, Copy)]
pub struct Host {
    pub config_dir: fn() -> PathBuf,
    pub settings: fn() -> Settings,
    /// Lock or login window titles on screen now; any one means locked.
    pub lock_titles: fn() -> Vec<String>,
    /// The control at a screen point, for the hard gate's click hints.
    #[cfg(target_os = "linux")]
    pub target_at: fn(i32, i32) -> Option<grokhub_core::desktop_mcp::ClickTarget>,
}

static HOST: std::sync::OnceLock<Host> = std::sync::OnceLock::new();

/// The first call wins; later calls are ignored.
pub fn install(host: Host) {
    let _ = HOST.set(host);
}

fn get() -> Option<&'static Host> {
    HOST.get()
}

pub fn config_dir() -> PathBuf {
    match get() {
        Some(host) => (host.config_dir)(),
        None => grokhub_agent::perm::config_dir(),
    }
}

pub fn settings() -> Settings {
    get().map(|host| (host.settings)()).unwrap_or_default()
}

pub fn lock_titles() -> Vec<String> {
    get().map(|host| (host.lock_titles)()).unwrap_or_default()
}

#[cfg(target_os = "linux")]
pub fn target_at(x: i32, y: i32) -> Option<grokhub_core::desktop_mcp::ClickTarget> {
    get().and_then(|host| (host.target_at)(x, y))
}
