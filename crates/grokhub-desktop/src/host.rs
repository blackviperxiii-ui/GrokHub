//! What the app that hosts the desktop tools tells them: where its config
//! lives, its desktop switches, and the screen readers it owns. The cabin and
//! the Tauri shell each install one; until then the switches are off.

use std::path::{Path, PathBuf};

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

/// The desktop switches as the two apps saved them in `dir`, for a process
/// with no app of its own (`grokhub-hub --mcp-desktop` and `--mcp-cua`).
/// Desktop control is on when the cabin's `app.json` or the shell's
/// `shell.json` has it on. Cua and its path come from `app.json` only: the
/// shell has no Cua flag. A missing or unreadable file counts as all off.
pub fn saved_settings(dir: &Path) -> Settings {
    let read = |name: &str| -> serde_json::Value {
        std::fs::read_to_string(dir.join(name))
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or(serde_json::Value::Null)
    };
    let on = |v: &serde_json::Value, key: &str| v.get(key).and_then(|b| b.as_bool()) == Some(true);
    let cabin = read("app.json");
    let shell = read("shell.json");
    Settings {
        desktop_control: on(&cabin, "desktopControl") || on(&shell, "desktopControl"),
        cua_driver: on(&cabin, "cuaDriver"),
        cua_driver_path: cabin
            .get("cuaDriverPath")
            .and_then(|p| p.as_str())
            .unwrap_or_default()
            .to_string(),
    }
}

/// The host for a headless process: the shared config folder, the switches
/// from [`saved_settings`], and this crate's own screen readers (as the
/// shell uses). Pins the start time so an earlier Halt keeps steps off.
pub fn install_standalone() {
    crate::process_started_ms();
    install(Host {
        config_dir: grokhub_agent::perm::config_dir,
        settings: || saved_settings(&grokhub_agent::perm::config_dir()),
        #[cfg(target_os = "linux")]
        lock_titles: crate::screen::lock_titles,
        #[cfg(not(target_os = "linux"))]
        lock_titles: Vec::new,
        #[cfg(target_os = "linux")]
        target_at: crate::screen::atspi_target_at,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("desk-host-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn saved_settings_read_both_apps_files() {
        let d = dir("none");
        assert_eq!(saved_settings(&d), Settings::default());

        std::fs::write(d.join("shell.json"), r#"{"desktopControl":true}"#).unwrap();
        assert_eq!(
            saved_settings(&d),
            Settings {
                desktop_control: true,
                cua_driver: false,
                cua_driver_path: String::new(),
            }
        );

        std::fs::write(d.join("shell.json"), r#"{"desktopControl":false}"#).unwrap();
        std::fs::write(
            d.join("app.json"),
            r#"{"desktopControl":true,"cuaDriver":true,"cuaDriverPath":"/opt/cua/cua-driver"}"#,
        )
        .unwrap();
        assert_eq!(
            saved_settings(&d),
            Settings {
                desktop_control: true,
                cua_driver: true,
                cua_driver_path: "/opt/cua/cua-driver".into(),
            }
        );

        // The shell has no Cua flag: one in shell.json is ignored.
        std::fs::write(d.join("app.json"), "{}").unwrap();
        std::fs::write(d.join("shell.json"), r#"{"cuaDriver":true}"#).unwrap();
        assert_eq!(saved_settings(&d), Settings::default());

        // A broken file is all off, never an error.
        std::fs::write(d.join("app.json"), "{not json").unwrap();
        std::fs::write(d.join("shell.json"), r#"{"desktopControl":"yes"}"#).unwrap();
        assert_eq!(saved_settings(&d), Settings::default());
        let _ = std::fs::remove_dir_all(&d);
    }
}
