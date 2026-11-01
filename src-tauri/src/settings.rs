//! The shell's own settings file, `shell.json` in the GrokHub config folder.
//! The API key is never written here; it lives in the OS keychain.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const FILE: &str = "shell.json";

/// How chat handles tools that change things, as the cabin's permission mode.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Approvals {
    /// A short judge call approves routine steps and asks about the rest.
    Auto,
    /// Writes and commands run without a card. Hard-class steps still ask.
    Always,
    /// Every write and command waits for Allow or Deny. A value this build
    /// does not know reads as Ask, the safe default (serde wants it last).
    #[default]
    #[serde(other)]
    Ask,
}

impl Approvals {
    pub fn mode(self) -> grokhub_agent::PermMode {
        match self {
            Self::Ask => grokhub_agent::PermMode::Ask,
            Self::Auto => grokhub_agent::PermMode::Auto,
            Self::Always => grokhub_agent::PermMode::Always,
        }
    }
}

/// Settings → Appearance. A value this build does not know reads as System.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Theme {
    /// Legacy id; same tokens as Ops Dark.
    Dark,
    OpsDark,
    OledBlack,
    SoftCharcoal,
    Light,
    /// Follows the OS (serde wants the fallback last).
    #[default]
    #[serde(other)]
    System,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ShellSettings {
    pub model: String,
    pub close_to_tray: bool,
    /// The folder the agent works in. Empty means the cabin's default.
    pub workspace: String,
    pub approvals: Approvals,
    /// Settings → Let Grok control the desktop. Off until the person turns it on.
    pub desktop_control: bool,
    /// Settings → Personality: the preset id. Empty is the default voice.
    pub personality: String,
    /// Settings → Personality → Adults-only personalities. Off hides the
    /// explicit presets, and a stored pick of one speaks in the default voice.
    pub explicit_personalities: bool,
    pub theme: Theme,
    /// The chat the window had open, reopened at the next launch. Empty after
    /// New chat.
    pub last_chat: String,
    /// Settings → General → the summon hotkey (`os_extras::SUMMON_HOTKEY`).
    /// Off until the person turns it on.
    pub summon_hotkey: bool,
    /// Settings → General → notifications while the window is in the
    /// background: an OK to give, a reply or an automation that finished.
    pub notifications: bool,
}

impl Default for ShellSettings {
    fn default() -> Self {
        Self {
            model: grokhub_agent::DEFAULT_MODEL.to_string(),
            close_to_tray: true,
            workspace: String::new(),
            approvals: Approvals::Ask,
            desktop_control: false,
            personality: String::new(),
            explicit_personalities: false,
            theme: Theme::System,
            last_chat: String::new(),
            summon_hotkey: false,
            notifications: true,
        }
    }
}

pub fn path(config_dir: &Path) -> PathBuf {
    config_dir.join(FILE)
}

/// A missing or unreadable file gives the defaults, so a bad edit never blocks startup.
/// A new install (no shell.json and no cabin app.json) starts on the companion's voice;
/// anyone who already has either keeps their pick.
pub fn load(config_dir: &Path) -> ShellSettings {
    if !path(config_dir).exists() && !config_dir.join("app.json").exists() {
        return ShellSettings {
            personality: grokhub_core::personality::DEFAULT_ID.to_string(),
            ..ShellSettings::default()
        };
    }
    let mut settings = std::fs::read_to_string(path(config_dir))
        .ok()
        .and_then(|raw| serde_json::from_str::<ShellSettings>(&raw).ok())
        .map(|mut s| {
            if s.model.trim().is_empty() {
                s.model = ShellSettings::default().model;
            }
            s
        })
        .unwrap_or_default();
    // The plain "Default" preset is gone; a shell still on it speaks as Nyx.
    if settings.personality.trim() == grokhub_core::personality::LEGACY_DEFAULT_ID {
        settings.personality = grokhub_core::personality::DEFAULT_ID.to_string();
        let _ = save(config_dir, &settings);
    }
    settings
}

/// Writes through a temp file and a rename, so a crash never leaves half a file.
pub fn save(config_dir: &Path, settings: &ShellSettings) -> Result<(), String> {
    if settings.model.trim().is_empty() {
        return Err("Pick a model.".into());
    }
    std::fs::create_dir_all(config_dir).map_err(|e| e.to_string())?;
    let raw = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    let tmp = config_dir.join(format!("{FILE}.tmp"));
    std::fs::write(&tmp, raw).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path(config_dir)).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "grokhub-shell-{name}-{}-{}",
            std::process::id(),
            grokhub_core::now_ms()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn missing_file_gives_defaults() {
        let dir = temp_dir("missing");
        assert_eq!(
            load(&dir),
            ShellSettings {
                model: "grok-4.7".into(),
                close_to_tray: true,
                workspace: String::new(),
                approvals: Approvals::Ask,
                desktop_control: false,
                personality: "nyx".into(),
                explicit_personalities: false,
                theme: Theme::System,
                last_chat: String::new(),
                summon_hotkey: false,
                notifications: true,
            }
        );
    }

    #[test]
    fn only_a_new_install_starts_on_nyx() {
        let dir = temp_dir("nyx");
        assert_eq!(load(&dir).personality, "nyx");
        // A cabin user opening the shell for the first time keeps the default voice.
        std::fs::create_dir_all(&dir).expect("dir");
        std::fs::write(dir.join("app.json"), "{}").expect("write");
        assert_eq!(load(&dir).personality, "");
        std::fs::remove_file(dir.join("app.json")).expect("rm");
        // A saved pick, including the default voice, is kept.
        std::fs::write(path(&dir), r#"{"model":"grok-5"}"#).expect("write");
        assert_eq!(load(&dir).personality, "");
        std::fs::write(path(&dir), r#"{"personality":"pirate"}"#).expect("write");
        assert_eq!(load(&dir).personality, "pirate");
        let fresh = ShellSettings {
            personality: "nyx".into(),
            ..ShellSettings::default()
        };
        save(&dir, &fresh).expect("save");
        assert_eq!(load(&dir).personality, "nyx");
        // A shell still on the retired "default" preset upgrades to Nyx on disk.
        std::fs::write(path(&dir), r#"{"personality":"default"}"#).expect("write");
        assert_eq!(load(&dir).personality, "nyx");
        let raw = std::fs::read_to_string(path(&dir)).expect("read");
        assert!(raw.contains("\"personality\": \"nyx\""), "{raw}");
        assert!(!raw.contains("\"default\""), "{raw}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = temp_dir("roundtrip");
        let wanted = ShellSettings {
            model: "grok-5".into(),
            close_to_tray: false,
            workspace: "/work/notes".into(),
            approvals: Approvals::Always,
            desktop_control: true,
            personality: "pirate".into(),
            explicit_personalities: true,
            theme: Theme::Dark,
            last_chat: "shell-abc-123".into(),
            summon_hotkey: true,
            notifications: false,
        };
        save(&dir, &wanted).expect("save");
        assert_eq!(load(&dir), wanted);
        let raw = std::fs::read_to_string(path(&dir)).expect("file");
        assert!(raw.contains("\"closeToTray\": false"), "{raw}");
        assert!(raw.contains("\"workspace\": \"/work/notes\""), "{raw}");
        assert!(raw.contains("\"approvals\": \"always\""), "{raw}");
        assert!(raw.contains("\"desktopControl\": true"), "{raw}");
        assert!(raw.contains("\"personality\": \"pirate\""), "{raw}");
        assert!(raw.contains("\"explicitPersonalities\": true"), "{raw}");
        assert!(raw.contains("\"theme\": \"dark\""), "{raw}");
        assert!(raw.contains("\"lastChat\": \"shell-abc-123\""), "{raw}");
        assert!(raw.contains("\"summonHotkey\": true"), "{raw}");
        assert!(raw.contains("\"notifications\": false"), "{raw}");
        assert!(!dir.join("shell.json.tmp").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn junk_or_blank_model_falls_back() {
        let dir = temp_dir("junk");
        std::fs::create_dir_all(&dir).expect("dir");
        std::fs::write(path(&dir), "{not json").expect("write");
        assert_eq!(load(&dir).model, "grok-4.7");
        std::fs::write(path(&dir), r#"{"model":"  ","closeToTray":false}"#).expect("write");
        assert_eq!(
            load(&dir),
            ShellSettings {
                model: "grok-4.7".into(),
                close_to_tray: false,
                workspace: String::new(),
                approvals: Approvals::Ask,
                desktop_control: false,
                personality: String::new(),
                explicit_personalities: false,
                theme: Theme::System,
                last_chat: String::new(),
                summon_hotkey: false,
                notifications: true,
            }
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn blank_model_is_refused() {
        let dir = temp_dir("blank");
        let blank = ShellSettings {
            model: " ".into(),
            close_to_tray: true,
            workspace: String::new(),
            approvals: Approvals::Ask,
            desktop_control: false,
            personality: String::new(),
            explicit_personalities: false,
            theme: Theme::System,
            last_chat: String::new(),
            summon_hotkey: false,
            notifications: true,
        };
        assert_eq!(save(&dir, &blank), Err("Pick a model.".to_string()));
        assert!(!path(&dir).exists());
    }

    #[test]
    fn an_unknown_approvals_value_reads_as_ask() {
        let dir = temp_dir("approvals");
        std::fs::create_dir_all(&dir).expect("dir");
        std::fs::write(path(&dir), r#"{"model":"grok-5","approvals":"yolo"}"#).expect("write");
        let loaded = load(&dir);
        assert_eq!(
            (loaded.model.as_str(), loaded.approvals),
            ("grok-5", Approvals::Ask)
        );
        std::fs::write(path(&dir), r#"{"approvals":"auto"}"#).expect("write");
        assert_eq!(load(&dir).approvals, Approvals::Auto);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_unknown_theme_reads_as_system() {
        let dir = temp_dir("theme");
        std::fs::create_dir_all(&dir).expect("dir");
        std::fs::write(path(&dir), r#"{"theme":"sepia"}"#).expect("write");
        assert_eq!(load(&dir).theme, Theme::System);
        std::fs::write(path(&dir), r#"{"theme":"light"}"#).expect("write");
        assert_eq!(load(&dir).theme, Theme::Light);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn hotkey_starts_off_and_notifications_on() {
        let fresh = ShellSettings::default();
        assert_eq!((fresh.summon_hotkey, fresh.notifications), (false, true));
        // A shell.json from before these switches keeps both defaults.
        let dir = temp_dir("basics");
        std::fs::create_dir_all(&dir).expect("dir");
        std::fs::write(path(&dir), r#"{"model":"grok-5","closeToTray":false}"#).expect("write");
        let old = load(&dir);
        assert_eq!(
            (old.model.as_str(), old.summon_hotkey, old.notifications),
            ("grok-5", false, true)
        );
        std::fs::write(path(&dir), r#"{"summonHotkey":true,"notifications":false}"#)
            .expect("write");
        let set = load(&dir);
        assert_eq!((set.summon_hotkey, set.notifications), (true, false));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
