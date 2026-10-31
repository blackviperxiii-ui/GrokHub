//! The cabin's side of the desktop tools in `grokhub-desktop`: it installs
//! the cabin's config folder, switches and screen readers as the crate's
//! host, and keeps the Settings glue that reads `AppConfig`.

pub(crate) use grokhub_desktop::{
    cua_wanted, desktop_panel_lines, process_started_ms, read_line_capped, screen_grabber,
    take_desktop_test_status,
};
#[cfg(windows)]
pub(crate) use grokhub_desktop::{windows_hands, windows_primary_frame};

/// The game guard now (cached 2 s): desktop control, the eyes' capture and
/// window checks wait while the user is gaming or an anti-cheat runs.
pub(crate) fn game_guard() -> grokhub_desktop::game_guard::GameGuard {
    install_host();
    grokhub_desktop::game_guard::current()
}

/// Before any desktop call. The first install wins, so this is cheap to repeat.
fn install_host() {
    grokhub_desktop::host::install(grokhub_desktop::host::Host {
        config_dir: crate::config::config_dir,
        settings: || {
            let cfg = crate::config::load();
            grokhub_desktop::host::Settings {
                desktop_control: cfg.desktop_control,
                cua_driver: cfg.cua_driver,
                cua_driver_path: cfg.cua_driver_path,
            }
        },
        lock_titles: crate::desktop::lock_titles,
        #[cfg(target_os = "linux")]
        target_at: crate::desktop::atspi_target_at,
    });
}

pub fn run_stdio() -> i32 {
    install_host();
    grokhub_desktop::run_stdio()
}

pub fn run_cua_stdio() -> i32 {
    install_host();
    grokhub_desktop::run_cua_stdio()
}

/// The desktop tools chat calls in process, through the hard gate.
pub(crate) fn native_desktop() -> grokhub_desktop::NativeDesktop {
    install_host();
    grokhub_desktop::NativeDesktop::new()
}

pub(crate) fn read_halt_stamp() -> Option<u64> {
    install_host();
    grokhub_desktop::read_halt_stamp()
}

pub(crate) fn write_halt_stamp() {
    install_host();
    grokhub_desktop::write_halt_stamp();
}

pub(crate) fn request_desktop_test(enabled: bool) -> String {
    install_host();
    grokhub_desktop::request_desktop_test(enabled)
}

/// Cabin tests never start the Linux broker, as before the move.
pub(crate) fn set_desktop_enabled(on: bool) {
    #[cfg(not(test))]
    grokhub_desktop::set_desktop_enabled(on);
    #[cfg(test)]
    let _ = on;
}

pub(crate) fn note_halt() {
    #[cfg(not(test))]
    grokhub_desktop::note_halt();
}

/// Status after the Settings desktop switch.
pub(crate) fn desktop_switch_note(on: bool) -> &'static str {
    if on {
        "Desktop control is on."
    } else {
        "Desktop control is off."
    }
}

/// The native engine's Cua server follows the desktop switch and the
/// `cuaDriver` flag (Linux only).
pub(crate) fn sync_native_cua(cfg: &crate::config::AppConfig) {
    let exe = native_cua_wanted(cfg)
        .then(|| std::env::current_exe().ok())
        .flatten();
    grokhub_agent::mcp::set_cabin_cua(exe);
}

fn native_cua_wanted(cfg: &crate::config::AppConfig) -> bool {
    cua_wanted(cfg.desktop_control, cfg.cua_driver)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cua_flag_is_off_by_default() {
        let cfg = crate::config::AppConfig::default();
        assert!(!cua_wanted(true, cfg.cua_driver));
    }

    #[test]
    fn the_click_target_read_is_capped() {
        #[cfg(target_os = "linux")]
        assert_eq!(
            crate::desktop::ATSPI_HIT_CAP,
            std::time::Duration::from_millis(300)
        );
    }

    #[test]
    fn the_host_reads_the_cabin_config_folder() {
        let root = crate::config::test_config_root("desktop-host");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let _pin = crate::config::TestConfigDir::set(root.clone());
        assert!(read_halt_stamp().is_none());
        write_halt_stamp();
        assert!(root.join("desktop-halt.stamp").is_file());
        assert!(read_halt_stamp().is_some());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn settings_desktop_panel_reports_backends() {
        let status = grokhub_core::desktop_mcp::desktop_control_status(
            "RemoteDesktop portal (libei)",
            "KWin ScreenShot2",
        );
        assert_eq!(
            status,
            "Input: RemoteDesktop portal (libei)\nCapture: KWin ScreenShot2"
        );
        let settings = include_str!("app/settings.rs");
        assert!(settings.contains("desktop_panel_lines"));
        assert!(settings.contains("\"Test\""));
        assert!(settings.contains("request_desktop_test"));
    }

    #[test]
    fn the_native_engine_runs_cua_itself_and_registers_nothing_in_grok_build() {
        let on = crate::config::AppConfig {
            desktop_control: true,
            cua_driver: true,
            ..crate::config::AppConfig::default()
        };
        // Cua is a Linux-only spike; elsewhere the native engine never starts it.
        assert_eq!(native_cua_wanted(&on), cfg!(target_os = "linux"));
        let no_flag = crate::config::AppConfig {
            cua_driver: false,
            ..on.clone()
        };
        assert!(!native_cua_wanted(&no_flag));
        let switch_off = crate::config::AppConfig {
            desktop_control: false,
            ..on.clone()
        };
        assert!(!native_cua_wanted(&switch_off));
        assert_eq!(desktop_switch_note(true), "Desktop control is on.");
        assert_eq!(desktop_switch_note(false), "Desktop control is off.");

        let start = include_str!("app/mod.rs").replace("\r\n", "\n");
        assert!(
            !start.contains("maybe_register_on_start("),
            "nothing registers in Grok Build at startup"
        );
        let settings = include_str!("app/settings.rs").replace("\r\n", "\n");
        let switch = settings
            .split("desktop_switch_note(on)")
            .nth(1)
            .expect("desktop switch");
        assert!(switch.contains("sync_native_cua(&self.cfg)"));
    }
}
