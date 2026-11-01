//! What this platform can run (cross-platform recode, Phase 4). The UI hides
//! a feature the report leaves off, and the commands behind it refuse, so a
//! phone never offers desktop control, local AI or the desktop-only feeds.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Capabilities {
    /// Settings → Let Grok control the desktop (computer-use).
    pub desktop_control: bool,
    /// The Pulse page, which reads the desktop indexers.
    pub pulse: bool,
    /// Settings → Local AI (on-device models).
    pub local_ai: bool,
    /// The tray icon and "Closing the window keeps GrokHub in the tray".
    pub tray: bool,
    /// Settings → Web access: this computer serves the web app (Phase 5).
    pub web_access: bool,
    /// Settings → Labs → Plugins: bundles install from git or a folder here.
    pub plugins: bool,
    /// Settings → General → start at login (the OS's login items).
    pub autostart: bool,
    /// Settings → General → the summon hotkey. Wayland gives an app no global
    /// hotkey, so it is off there.
    pub summon_hotkey: bool,
    /// Settings → General → notifications while the window is in the background.
    pub notifications: bool,
}

/// The report for an OS name as `std::env::consts::OS` spells it. Linux,
/// Windows and macOS get everything; any other OS gets chat, memory,
/// automations and connectors only.
pub fn for_os(os: &str) -> Capabilities {
    for_env(os, false)
}

/// [`for_os`] in a session that is (`wayland`) or isn't a Wayland one, which
/// drops the summon hotkey.
pub fn for_env(os: &str, wayland: bool) -> Capabilities {
    let desktop = matches!(os, "linux" | "windows" | "macos");
    Capabilities {
        desktop_control: desktop,
        pulse: desktop,
        local_ai: desktop,
        tray: desktop,
        web_access: desktop,
        plugins: desktop,
        autostart: desktop,
        summon_hotkey: desktop && !(os == "linux" && wayland),
        notifications: desktop,
    }
}

/// Whether this Linux session is a Wayland one, from the variables the
/// session sets.
pub fn wayland_session(session_type: Option<&str>, wayland_display: Option<&str>) -> bool {
    session_type.is_some_and(|kind| kind.eq_ignore_ascii_case("wayland"))
        || wayland_display.is_some_and(|display| !display.is_empty())
}

/// The report for the OS this build runs on.
pub fn current() -> Capabilities {
    let session_type = std::env::var("XDG_SESSION_TYPE").ok();
    let wayland_display = std::env::var("WAYLAND_DISPLAY").ok();
    for_env(
        std::env::consts::OS,
        wayland_session(session_type.as_deref(), wayland_display.as_deref()),
    )
}

/// `Err` with a plain note when this platform lacks the feature, for the
/// commands behind a hidden page.
pub fn needs(available: bool, feature: &str) -> Result<(), String> {
    if available {
        Ok(())
    } else {
        Err(format!("{feature} isn't available on this device."))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktops_get_every_feature() {
        let all = Capabilities {
            desktop_control: true,
            pulse: true,
            local_ai: true,
            tray: true,
            web_access: true,
            plugins: true,
            autostart: true,
            summon_hotkey: true,
            notifications: true,
        };
        assert_eq!(for_os("linux"), all);
        assert_eq!(for_os("windows"), all);
        assert_eq!(for_os("macos"), all);
    }

    #[test]
    fn phones_get_no_desktop_only_feature() {
        let none = Capabilities {
            desktop_control: false,
            pulse: false,
            local_ai: false,
            tray: false,
            web_access: false,
            plugins: false,
            autostart: false,
            summon_hotkey: false,
            notifications: false,
        };
        assert_eq!(for_os("android"), none);
        assert_eq!(for_os("ios"), none);
        assert_eq!(for_env("android", true), none);
    }

    #[test]
    fn wayland_drops_only_the_summon_hotkey() {
        let x11 = for_env("linux", false);
        let wayland = for_env("linux", true);
        assert_eq!((x11.summon_hotkey, wayland.summon_hotkey), (true, false));
        assert_eq!(
            (wayland.autostart, wayland.notifications, wayland.tray),
            (true, true, true)
        );
        // The variables only mean something on Linux.
        assert!(for_env("windows", true).summon_hotkey);
        assert!(for_env("macos", true).summon_hotkey);
    }

    #[test]
    fn a_wayland_session_is_told_by_its_variables() {
        assert!(wayland_session(Some("wayland"), None));
        assert!(wayland_session(Some("Wayland"), Some("")));
        assert!(wayland_session(Some("x11"), Some("wayland-0")));
        assert!(!wayland_session(Some("x11"), None));
        assert!(!wayland_session(Some("tty"), Some("")));
        assert!(!wayland_session(None, None));
    }

    #[test]
    fn a_missing_feature_is_refused_in_words() {
        assert_eq!(needs(true, "Pulse"), Ok(()));
        assert_eq!(
            needs(false, "Local AI"),
            Err("Local AI isn't available on this device.".to_string())
        );
    }

    #[test]
    fn the_report_reaches_the_ui_in_camel_case() {
        let json = serde_json::to_value(for_os("android")).expect("serializes");
        assert_eq!(
            json,
            serde_json::json!({
                "desktopControl": false,
                "pulse": false,
                "localAi": false,
                "tray": false,
                "webAccess": false,
                "plugins": false,
                "autostart": false,
                "summonHotkey": false,
                "notifications": false,
            })
        );
    }
}
