//! The shell's own settings file, `shell.json` in the GrokHub config folder.
//! The API key is never written here; it lives in the OS keychain.

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
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
    /// Settings → General → the summon hotkey (`os_extras::SUMMON_HOTKEY`).
    /// Off until the person turns it on.
    pub summon_hotkey: bool,
    /// Settings → General → notifications while the window is in the
    /// background: an OK to give, a reply or an automation that finished.
    pub notifications: bool,
    /// Settings → Behavior. A value this build cannot read loads as its default.
    #[serde(deserialize_with = "lenient_behavior")]
    pub behavior: Behavior,
}

/// Settings → Behavior: the composer and the limits on scheduled work.
/// Everything starts off, and empty quiet hours are none.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Behavior {
    /// Sentence autocomplete in the composer.
    pub autocomplete: bool,
    /// Quiet hours for automations, as `HH:MM`. Both empty is none.
    pub quiet_start: String,
    pub quiet_end: String,
    /// Tokens a day, one of [`grokhub_core::TOKEN_BUDGETS`]. 0 is off.
    pub daily_token_budget: u64,
    /// Pause scheduled work over the daily budget.
    pub pause_over_budget: bool,
}

impl Behavior {
    /// The same choices with the quiet hours written as `HH:MM`, or why not.
    pub fn checked(self) -> Result<Self, String> {
        let (quiet_start, quiet_end) = quiet_hours(&self.quiet_start, &self.quiet_end)?;
        if !grokhub_core::TOKEN_BUDGETS.contains(&self.daily_token_budget) {
            return Err("Pick a daily token budget from the list.".into());
        }
        Ok(Self {
            quiet_start,
            quiet_end,
            ..self
        })
    }
}

/// Quiet hours as typed: both empty is none, otherwise two different clocks.
pub fn quiet_hours(start: &str, end: &str) -> Result<(String, String), String> {
    let (start, end) = (start.trim(), end.trim());
    if start.is_empty() && end.is_empty() {
        return Ok((String::new(), String::new()));
    }
    if start.is_empty() || end.is_empty() {
        return Err("Set both times, or clear both for no quiet hours.".into());
    }
    let clock = |text: &str| {
        let fixed = grokhub_core::normalize_hm(text, "");
        if fixed.is_empty() {
            Err(format!("{text} isn't a time. Use one like 22:00."))
        } else {
            Ok(fixed)
        }
    };
    let (start, end) = (clock(start)?, clock(end)?);
    if start == end {
        return Err("Start and end are the same. Clear both for no quiet hours.".into());
    }
    Ok((start, end))
}

/// Reads each Behavior field on its own, so one value this build can't read
/// (a typo, a newer build's choice) resets that field, never the whole file.
fn lenient_behavior<'de, D: Deserializer<'de>>(d: D) -> Result<Behavior, D::Error> {
    let raw = Value::deserialize(d)?;
    let flag = |key: &str| raw.get(key).and_then(Value::as_bool).unwrap_or(false);
    let text = |key: &str| raw.get(key).and_then(Value::as_str).unwrap_or("");
    let (quiet_start, quiet_end) =
        quiet_hours(text("quietStart"), text("quietEnd")).unwrap_or_default();
    Ok(Behavior {
        autocomplete: flag("autocomplete"),
        quiet_start,
        quiet_end,
        daily_token_budget: raw
            .get("dailyTokenBudget")
            .and_then(Value::as_u64)
            .filter(|b| grokhub_core::TOKEN_BUDGETS.contains(b))
            .unwrap_or(0),
        pause_over_budget: flag("pauseOverBudget"),
    })
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
            behavior: Behavior::default(),
            summon_hotkey: false,
            notifications: true,
        }
    }
}

/// One row of the daily token budget list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BudgetChoice {
    pub value: u64,
    /// "Off" or "1.0M tokens a day".
    pub label: String,
}

/// Settings → Behavior as the page shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BehaviorView {
    pub close_to_tray: bool,
    pub behavior: Behavior,
    pub budgets: Vec<BudgetChoice>,
}

pub fn behavior_view(settings: &ShellSettings) -> BehaviorView {
    BehaviorView {
        close_to_tray: settings.close_to_tray,
        behavior: settings.behavior.clone(),
        budgets: grokhub_core::TOKEN_BUDGETS
            .iter()
            .map(|&value| BudgetChoice {
                value,
                label: grokhub_core::token_budget_label(value),
            })
            .collect(),
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
                behavior: Behavior::default(),
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
            behavior: Behavior {
                autocomplete: true,
                quiet_start: "22:00".into(),
                quiet_end: "07:00".into(),
                daily_token_budget: 1_000_000,
                pause_over_budget: true,
            },
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
        assert!(raw.contains("\"quietStart\": \"22:00\""), "{raw}");
        assert!(raw.contains("\"dailyTokenBudget\": 1000000"), "{raw}");
        assert!(raw.contains("\"pauseOverBudget\": true"), "{raw}");
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
                behavior: Behavior::default(),
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
            behavior: Behavior::default(),
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
    fn behavior_values_this_build_cannot_read_load_as_the_defaults() {
        let dir = temp_dir("behavior");
        std::fs::create_dir_all(&dir).expect("dir");
        std::fs::write(
            path(&dir),
            r#"{"model":"grok-5","behavior":{"autocomplete":"yes","quietStart":"bedtime","quietEnd":"07:00","dailyTokenBudget":123,"pauseOverBudget":1}}"#,
        )
        .expect("write");
        let loaded = load(&dir);
        assert_eq!(
            (loaded.model.as_str(), loaded.behavior),
            (
                "grok-5",
                Behavior {
                    autocomplete: false,
                    quiet_start: String::new(),
                    quiet_end: String::new(),
                    daily_token_budget: 0,
                    pause_over_budget: false,
                }
            )
        );
        std::fs::write(path(&dir), r#"{"model":"grok-5","behavior":"loud"}"#).expect("write");
        assert_eq!(load(&dir).behavior, Behavior::default());
        // One bad field leaves the good ones alone.
        std::fs::write(
            path(&dir),
            r#"{"behavior":{"autocomplete":true,"quietStart":"23:00","quietEnd":"6:30","dailyTokenBudget":"lots"}}"#,
        )
        .expect("write");
        assert_eq!(
            load(&dir).behavior,
            Behavior {
                autocomplete: true,
                quiet_start: "23:00".into(),
                quiet_end: "06:30".into(),
                daily_token_budget: 0,
                pause_over_budget: false,
            }
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn quiet_hours_are_both_or_neither() {
        assert_eq!(quiet_hours(" ", ""), Ok((String::new(), String::new())));
        assert_eq!(
            quiet_hours("22:00", " 7:05 "),
            Ok(("22:00".to_string(), "07:05".to_string()))
        );
        assert_eq!(
            quiet_hours("22:00", ""),
            Err("Set both times, or clear both for no quiet hours.".to_string())
        );
        assert_eq!(
            quiet_hours("25:00", "07:00"),
            Err("25:00 isn't a time. Use one like 22:00.".to_string())
        );
        assert_eq!(
            quiet_hours("07:00", "07:00"),
            Err("Start and end are the same. Clear both for no quiet hours.".to_string())
        );
        let picked = Behavior {
            quiet_start: "9:00".into(),
            quiet_end: "17:00".into(),
            daily_token_budget: 500_000,
            ..Behavior::default()
        };
        assert_eq!(
            picked.clone().checked().map(|b| b.quiet_start),
            Ok("09:00".to_string())
        );
        assert_eq!(
            Behavior {
                daily_token_budget: 7,
                ..picked
            }
            .checked(),
            Err("Pick a daily token budget from the list.".to_string())
        );
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
