//! The shell's own settings file, `shell.json` in the GrokHub config folder.
//! The API key is never written here; it lives in the OS keychain. No model or
//! effort is kept: the router picks both, and a model or effort an older build
//! saved is dropped when the file loads.

use grokhub_core::personality::Heat;
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
    /// It only turns on after the one-time 18+ check ([`Self::adult_confirmed`]).
    pub explicit_personalities: bool,
    /// The person confirmed once that they are 18 or older. Asked the first
    /// time adults-only turns on, then never again. No birth date is kept.
    pub adult_confirmed: bool,
    /// When that 18+ yes was given (Unix ms). Empty for a yes carried over
    /// from before it was recorded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub adult_confirmed_at_ms: Option<u64>,
    /// Settings → Personality → Dynamic chat: another voice can step in while
    /// the topic calls for it, then hand back. On until the person turns it off.
    #[serde(default = "on")]
    pub dynamic_chat: bool,
    /// Settings → Personality → Heat: how crude the jokes get, used only while
    /// adults-only is on. Cheeky until the person moves it.
    pub quip_heat: Heat,
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
    /// Settings → Behavior. A value this build cannot read loads as its default.
    #[serde(deserialize_with = "lenient_behavior")]
    pub behavior: Behavior,
    /// Settings → Appearance → Text size, in percent: one of auto setup's
    /// steps. The window zooms to it.
    #[serde(deserialize_with = "text_size")]
    pub text_size: u16,
    /// Settings → Appearance → Reduce motion: no animations or slides.
    pub reduce_motion: bool,
    /// Notifications only for what needs you: an OK to give or a reply, not
    /// a finished automation (auto setup's "only important").
    pub notify_important: bool,
    /// Settings → Code → Pushes and PRs. An unreadable block loads as Ask.
    #[serde(deserialize_with = "crate::code_ship::lenient")]
    pub code_ship: crate::code_ship::CodeShip,
    /// Settings → Labs → Desktop companion. Off until the person turns it on.
    /// Same camelCase key as the cabin (`desktopNyx`).
    #[serde(default)]
    pub desktop_nyx: bool,
    /// She is popped out onto the desktop right now.
    #[serde(default)]
    pub desktop_nyx_undocked: bool,
    /// Her window sits behind other windows instead of floating on top.
    #[serde(default)]
    pub desktop_nyx_behind: bool,
    /// Where her window last settled, in points from the monitor's corner.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub desktop_nyx_pos: Option<[i32; 2]>,
    /// Adults-only outfits may show on the desktop. Off by default.
    #[serde(default)]
    pub desktop_nyx_adult: bool,
    /// Outfit pinned from the wardrobe; `None` lets her default mode choose.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub desktop_nyx_outfit: Option<String>,
}

/// A size this build doesn't offer reads as the step at or below it.
fn text_size<'de, D: Deserializer<'de>>(d: D) -> Result<u16, D::Error> {
    let pct = Value::deserialize(d)?.as_u64().unwrap_or(100).min(400);
    Ok(grokhub_core::autotune::settings::text_step_for(pct as u16))
}

/// Settings → Behavior: the composer and the limits on scheduled work.
/// Everything but Suggestions starts off, and empty quiet hours are none.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Behavior {
    /// Sentence autocomplete in the composer.
    pub autocomplete: bool,
    /// A few next steps to tap, under the box, on Home and after replies.
    /// On until the person turns it off; off, no chip shows and the reply's
    /// chips block leaves the chat's rules.
    #[serde(default = "on")]
    pub suggestions: bool,
    /// Quiet hours for automations, as `HH:MM`. Both empty is none.
    pub quiet_start: String,
    pub quiet_end: String,
    /// Tokens a day, one of [`grokhub_core::TOKEN_BUDGETS`]. 0 is off.
    pub daily_token_budget: u64,
    /// Pause scheduled work over the daily budget.
    pub pause_over_budget: bool,
}

fn on() -> bool {
    true
}

impl Default for Behavior {
    fn default() -> Self {
        Self {
            autocomplete: false,
            suggestions: true,
            quiet_start: String::new(),
            quiet_end: String::new(),
            daily_token_budget: 0,
            pause_over_budget: false,
        }
    }
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
        suggestions: raw
            .get("suggestions")
            .and_then(Value::as_bool)
            .unwrap_or(true),
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
            close_to_tray: true,
            workspace: String::new(),
            approvals: Approvals::Ask,
            desktop_control: false,
            personality: String::new(),
            explicit_personalities: false,
            adult_confirmed: false,
            adult_confirmed_at_ms: None,
            dynamic_chat: true,
            quip_heat: Heat::Cheeky,
            theme: Theme::System,
            last_chat: String::new(),
            behavior: Behavior::default(),
            summon_hotkey: false,
            notifications: true,
            text_size: 100,
            reduce_motion: false,
            notify_important: false,
            code_ship: crate::code_ship::CodeShip::default(),
            desktop_nyx: false,
            desktop_nyx_undocked: false,
            desktop_nyx_behind: false,
            desktop_nyx_pos: None,
            desktop_nyx_adult: false,
            desktop_nyx_outfit: None,
        }
    }
}

impl ShellSettings {
    /// Adults-only personalities in effect: switched on and the 18+ check answered.
    pub fn adults_on(&self) -> bool {
        self.explicit_personalities && self.adult_confirmed
    }

    /// The heat every voice jokes at, or `None` while adults-only is off or the
    /// 18+ check was never answered.
    pub fn adult_heat(&self) -> Option<Heat> {
        self.adults_on().then_some(self.quip_heat)
    }

    /// Adults-only personalities switched `on`, or `None` while turning it on
    /// still needs the one-time 18+ check: nothing changes until it is answered.
    pub fn adults_only(self, on: bool) -> Option<Self> {
        (!on || self.adult_confirmed).then_some(Self {
            explicit_personalities: on,
            ..self
        })
    }

    /// The 18+ check's yes at `now_ms`: remembered for good, with when it was
    /// given (never a birth date), and adults-only turns on.
    pub fn confirm_adult(self, now_ms: u64) -> Self {
        Self {
            adult_confirmed: true,
            adult_confirmed_at_ms: Some(now_ms),
            explicit_personalities: true,
            ..self
        }
    }
}

/// A stop on the Heat dial: anything off the dial (Clean) is Cheeky.
pub fn dial_heat(heat: Heat) -> Heat {
    if Heat::DIAL.contains(&heat) {
        heat
    } else {
        Heat::Cheeky
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
    // A `model` or `effort` an older build saved is an unknown key now: it is
    // ignored here and gone from the file at the next save.
    let mut settings = std::fs::read_to_string(path(config_dir))
        .ok()
        .and_then(|raw| serde_json::from_str::<ShellSettings>(&raw).ok())
        .unwrap_or_default();
    // Adults-only already on from before the 18+ check: that was the person's
    // own opt-in, as the cabin reads it.
    if settings.explicit_personalities {
        settings.adult_confirmed = true;
    }
    // The plain "Default" preset is gone; a shell still on it speaks as Nyx.
    if settings.personality.trim() == grokhub_core::personality::LEGACY_DEFAULT_ID {
        settings.personality = grokhub_core::personality::DEFAULT_ID.to_string();
        let _ = save(config_dir, &settings);
    }
    settings
}

/// Writes through a temp file and a rename, so a crash never leaves half a file.
pub fn save(config_dir: &Path, settings: &ShellSettings) -> Result<(), String> {
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
                close_to_tray: true,
                workspace: String::new(),
                approvals: Approvals::Ask,
                desktop_control: false,
                personality: "nyx".into(),
                explicit_personalities: false,
                adult_confirmed: false,
                adult_confirmed_at_ms: None,
                dynamic_chat: true,
                quip_heat: Heat::Cheeky,
                theme: Theme::System,
                last_chat: String::new(),
                behavior: Behavior::default(),
                summon_hotkey: false,
                notifications: true,
                text_size: 100,
                reduce_motion: false,
                notify_important: false,
                code_ship: crate::code_ship::CodeShip::default(),
                desktop_nyx: false,
                desktop_nyx_undocked: false,
                desktop_nyx_behind: false,
                desktop_nyx_pos: None,
                desktop_nyx_adult: false,
                desktop_nyx_outfit: None,
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
            close_to_tray: false,
            workspace: "/work/notes".into(),
            approvals: Approvals::Always,
            desktop_control: true,
            personality: "pirate".into(),
            explicit_personalities: true,
            adult_confirmed: true,
            adult_confirmed_at_ms: Some(1_700_000_000_000),
            dynamic_chat: false,
            quip_heat: Heat::Spicy,
            theme: Theme::Dark,
            last_chat: "shell-abc-123".into(),
            behavior: Behavior {
                autocomplete: true,
                // Off round-trips, though a missing value reads as on.
                suggestions: false,
                quiet_start: "22:00".into(),
                quiet_end: "07:00".into(),
                daily_token_budget: 1_000_000,
                pause_over_budget: true,
            },
            summon_hotkey: true,
            notifications: false,
            text_size: 100,
            reduce_motion: false,
            notify_important: false,
            code_ship: crate::code_ship::CodeShip::default(),
            desktop_nyx: true,
            desktop_nyx_undocked: true,
            desktop_nyx_behind: false,
            desktop_nyx_pos: Some([100, 200]),
            desktop_nyx_adult: false,
            desktop_nyx_outfit: Some("gaming".into()),
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
        assert!(raw.contains("\"adultConfirmed\": true"), "{raw}");
        assert!(
            raw.contains("\"adultConfirmedAtMs\": 1700000000000"),
            "{raw}"
        );
        assert!(raw.contains("\"dynamicChat\": false"), "{raw}");
        assert!(raw.contains("\"quipHeat\": \"spicy\""), "{raw}");
        assert!(raw.contains("\"theme\": \"dark\""), "{raw}");
        assert!(raw.contains("\"lastChat\": \"shell-abc-123\""), "{raw}");
        assert!(raw.contains("\"quietStart\": \"22:00\""), "{raw}");
        assert!(raw.contains("\"dailyTokenBudget\": 1000000"), "{raw}");
        assert!(raw.contains("\"pauseOverBudget\": true"), "{raw}");
        assert!(raw.contains("\"summonHotkey\": true"), "{raw}");
        assert!(raw.contains("\"notifications\": false"), "{raw}");
        assert!(raw.contains("\"desktopNyx\": true"), "{raw}");
        assert!(raw.contains("\"desktopNyxUndocked\": true"), "{raw}");
        assert!(raw.contains("\"desktopNyxOutfit\": \"gaming\""), "{raw}");
        assert!(!dir.join("shell.json.tmp").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn junk_falls_back_to_the_defaults() {
        let dir = temp_dir("junk");
        std::fs::create_dir_all(&dir).expect("dir");
        std::fs::write(path(&dir), "{not json").expect("write");
        assert_eq!(load(&dir), ShellSettings::default());
        std::fs::write(path(&dir), r#"{"model":"  ","closeToTray":false}"#).expect("write");
        assert_eq!(
            load(&dir),
            ShellSettings {
                close_to_tray: false,
                workspace: String::new(),
                approvals: Approvals::Ask,
                desktop_control: false,
                personality: String::new(),
                explicit_personalities: false,
                adult_confirmed: false,
                adult_confirmed_at_ms: None,
                dynamic_chat: true,
                quip_heat: Heat::Cheeky,
                theme: Theme::System,
                last_chat: String::new(),
                behavior: Behavior::default(),
                summon_hotkey: false,
                notifications: true,
                text_size: 100,
                reduce_motion: false,
                notify_important: false,
                code_ship: crate::code_ship::CodeShip::default(),
                desktop_nyx: false,
                desktop_nyx_undocked: false,
                desktop_nyx_behind: false,
                desktop_nyx_pos: None,
                desktop_nyx_adult: false,
                desktop_nyx_outfit: None,
            }
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_old_model_or_effort_pick_still_loads_and_is_dropped() {
        let dir = temp_dir("retired-pick");
        std::fs::create_dir_all(&dir).expect("dir");
        std::fs::write(
            path(&dir),
            r#"{"model":"grok-5","effort":"high","reasoningEffort":"low","closeToTray":false,"approvals":"auto","personality":"pirate"}"#,
        )
        .expect("write");
        let loaded = load(&dir);
        assert_eq!(
            loaded,
            ShellSettings {
                close_to_tray: false,
                workspace: String::new(),
                approvals: Approvals::Auto,
                desktop_control: false,
                personality: "pirate".into(),
                explicit_personalities: false,
                adult_confirmed: false,
                adult_confirmed_at_ms: None,
                dynamic_chat: true,
                quip_heat: Heat::Cheeky,
                theme: Theme::System,
                last_chat: String::new(),
                behavior: Behavior::default(),
                summon_hotkey: false,
                notifications: true,
                text_size: 100,
                reduce_motion: false,
                notify_important: false,
                code_ship: crate::code_ship::CodeShip::default(),
                desktop_nyx: false,
                desktop_nyx_undocked: false,
                desktop_nyx_behind: false,
                desktop_nyx_pos: None,
                desktop_nyx_adult: false,
                desktop_nyx_outfit: None,
            }
        );
        save(&dir, &loaded).expect("save");
        let raw = std::fs::read_to_string(path(&dir)).expect("file");
        assert!(!raw.contains("\"model\""), "{raw}");
        assert!(!raw.contains("grok-5"), "{raw}");
        assert!(!raw.contains("\"effort\""), "{raw}");
        assert!(!raw.contains("reasoningEffort"), "{raw}");
        assert!(raw.contains("\"approvals\": \"auto\""), "{raw}");
        assert_eq!(load(&dir), loaded);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_unknown_approvals_value_reads_as_ask() {
        let dir = temp_dir("approvals");
        std::fs::create_dir_all(&dir).expect("dir");
        std::fs::write(path(&dir), r#"{"model":"grok-5","approvals":"yolo"}"#).expect("write");
        assert_eq!(load(&dir).approvals, Approvals::Ask);
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
            r#"{"model":"grok-5","behavior":{"autocomplete":"yes","suggestions":"nope","quietStart":"bedtime","quietEnd":"07:00","dailyTokenBudget":123,"pauseOverBudget":1}}"#,
        )
        .expect("write");
        assert_eq!(
            load(&dir).behavior,
            Behavior {
                autocomplete: false,
                suggestions: true,
                quiet_start: String::new(),
                quiet_end: String::new(),
                daily_token_budget: 0,
                pause_over_budget: false,
            }
        );
        std::fs::write(path(&dir), r#"{"model":"grok-5","behavior":"loud"}"#).expect("write");
        assert_eq!(load(&dir).behavior, Behavior::default());
        // One bad field leaves the good ones alone.
        std::fs::write(
            path(&dir),
            r#"{"behavior":{"autocomplete":true,"suggestions":false,"quietStart":"23:00","quietEnd":"6:30","dailyTokenBudget":"lots"}}"#,
        )
        .expect("write");
        assert_eq!(
            load(&dir).behavior,
            Behavior {
                autocomplete: true,
                suggestions: false,
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
        assert_eq!((old.summon_hotkey, old.notifications), (false, true));
        std::fs::write(path(&dir), r#"{"summonHotkey":true,"notifications":false}"#)
            .expect("write");
        let set = load(&dir);
        assert_eq!((set.summon_hotkey, set.notifications), (true, false));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dynamic_chat_starts_on_and_heat_cheeky_even_in_an_old_file() {
        let fresh = ShellSettings::default();
        assert!(fresh.dynamic_chat);
        assert_eq!(fresh.quip_heat, Heat::Cheeky);
        assert!(!fresh.adult_confirmed);
        assert_eq!(fresh.adult_confirmed_at_ms, None);
        // A shell.json from before these settings.
        let dir = temp_dir("voice-flow");
        std::fs::create_dir_all(&dir).expect("dir");
        std::fs::write(
            path(&dir),
            r#"{"closeToTray":false,"personality":"pirate"}"#,
        )
        .expect("write");
        let old = load(&dir);
        assert!(old.dynamic_chat);
        assert_eq!(old.quip_heat, Heat::Cheeky);
        assert!(!old.adult_confirmed);
        // No timestamp is written until there is one.
        save(&dir, &old).expect("save");
        let raw = std::fs::read_to_string(path(&dir)).expect("read");
        assert!(!raw.contains("adultConfirmedAtMs"), "{raw}");
        assert!(raw.contains("\"dynamicChat\": true"), "{raw}");
        assert!(raw.contains("\"quipHeat\": \"cheeky\""), "{raw}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn adults_only_from_before_the_check_counts_as_confirmed() {
        let dir = temp_dir("adult-migrate");
        std::fs::create_dir_all(&dir).expect("dir");
        std::fs::write(path(&dir), r#"{"explicitPersonalities":true}"#).expect("write");
        let old = load(&dir);
        assert!(old.explicit_personalities && old.adult_confirmed);
        assert_eq!(old.adult_confirmed_at_ms, None);
        assert!(old.adults_on());
        assert_eq!(old.adult_heat(), Some(Heat::Cheeky));
        // Off stays unconfirmed.
        std::fs::write(path(&dir), r#"{"explicitPersonalities":false}"#).expect("write");
        let off = load(&dir);
        assert!(!off.adult_confirmed && !off.adults_on());
        assert_eq!(off.adult_heat(), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn heat_needs_both_the_switch_and_the_18_check() {
        let mut s = ShellSettings {
            quip_heat: Heat::Filthy,
            ..ShellSettings::default()
        };
        assert_eq!(s.adult_heat(), None);
        s.explicit_personalities = true;
        assert_eq!(s.adult_heat(), None, "on without the check is still off");
        s.adult_confirmed = true;
        assert_eq!(s.adult_heat(), Some(Heat::Filthy));
        s.explicit_personalities = false;
        assert_eq!(s.adult_heat(), None);
    }

    #[test]
    fn adults_only_waits_for_the_18_check_then_switches_freely() {
        let fresh = ShellSettings::default();
        // Before the check: refused, nothing changes.
        assert_eq!(fresh.clone().adults_only(true), None);
        // Off is always fine.
        let off = fresh.clone().adults_only(false).expect("off");
        assert!(!off.explicit_personalities);
        let yes = fresh.confirm_adult(1_700_000_000_000);
        assert!(yes.adult_confirmed && yes.explicit_personalities && yes.adults_on());
        assert_eq!(yes.adult_confirmed_at_ms, Some(1_700_000_000_000));
        // After it: off and back on with no second check, the yes kept.
        let off = yes.adults_only(false).expect("off");
        assert!(!off.adults_on() && off.adult_confirmed);
        let on = off.adults_only(true).expect("on again");
        assert!(on.adults_on());
        assert_eq!(on.adult_confirmed_at_ms, Some(1_700_000_000_000));
    }

    #[test]
    fn the_dial_has_no_clean_stop() {
        assert_eq!(dial_heat(Heat::Clean), Heat::Cheeky);
        for heat in Heat::DIAL {
            assert_eq!(dial_heat(heat), heat);
        }
    }
}
