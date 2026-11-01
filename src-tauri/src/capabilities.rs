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
    /// The Library page: this computer's Steam games and Play.
    pub library: bool,
}

/// The report for an OS name as `std::env::consts::OS` spells it. Linux,
/// Windows and macOS get everything; any other OS gets chat, memory,
/// automations and connectors only.
pub fn for_os(os: &str) -> Capabilities {
    let desktop = matches!(os, "linux" | "windows" | "macos");
    Capabilities {
        desktop_control: desktop,
        pulse: desktop,
        local_ai: desktop,
        tray: desktop,
        web_access: desktop,
        plugins: desktop,
        library: desktop,
    }
}

/// The report for the OS this build runs on.
pub fn current() -> Capabilities {
    for_os(std::env::consts::OS)
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
            library: true,
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
            library: false,
        };
        assert_eq!(for_os("android"), none);
        assert_eq!(for_os("ios"), none);
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
                "library": false,
            })
        );
    }
}
