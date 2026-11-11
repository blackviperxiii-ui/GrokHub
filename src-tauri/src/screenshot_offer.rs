//! Settings → Behavior → Screenshot explain and the composer's Explain chip,
//! the cabin's `screenshot_offer_ui`. Off by default. The switch is the
//! cabin's own `screenshotExplain` in `app.json` and the offer is its
//! `screenshot-offer.json`, so both apps agree. Only names and modified times
//! are read while watching; the picture is read when Explain is tapped, and
//! nothing is sent until the person sends.

use std::path::{Path, PathBuf};

use grokhub_core::screenshot_offer::pictures_dir;
use grokhub_core::{
    dismiss_screenshot_offer, newest_in_watch_dirs, tick_screenshot_offer, watch_dirs,
    ScreenshotExplainSettings, ScreenshotOfferLedger, CHIP_LABEL, DISMISS_LABEL, EXPLAIN_PROMPT,
};
use serde::Serialize;

use crate::attach::Outcome;

/// The cabin's key in `app.json`.
pub const SETTING_KEY: &str = "screenshotExplain";
/// The cabin's offer file in the config folder.
pub const LEDGER_FILE: &str = "screenshot-offer.json";
/// Explain was tapped after the file was moved or deleted.
pub const GONE: &str = "That screenshot is gone.";
/// Explain was tapped after the offer ran out or was turned off.
pub const NO_OFFER: &str = "That screenshot offer has ended.";

/// The chip as the composer shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Offer {
    /// The file's name, for the chip's title.
    pub name: String,
    /// "Explain this?"
    pub label: String,
    /// "Not now"
    pub dismiss: String,
    /// What goes in an empty composer once the picture is attached.
    pub prompt: String,
}

/// The switch, and the live offer while it is on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct View {
    pub enabled: bool,
    pub offer: Option<Offer>,
}

fn ledger_path(config_dir: &Path) -> PathBuf {
    config_dir.join(LEDGER_FILE)
}

/// The saved offer; a missing or unreadable file is a fresh ledger.
pub fn load_ledger(config_dir: &Path) -> ScreenshotOfferLedger {
    std::fs::read_to_string(ledger_path(config_dir))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn save_ledger(config_dir: &Path, ledger: &ScreenshotOfferLedger) -> Result<(), String> {
    let text = serde_json::to_string_pretty(ledger).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(config_dir).map_err(|e| e.to_string())?;
    std::fs::write(ledger_path(config_dir), text).map_err(|e| e.to_string())
}

/// The switch as the cabin saved it. Absent or unreadable is off.
pub fn enabled(config_dir: &Path) -> bool {
    grokhub_core::app_json::read(config_dir)
        .ok()
        .and_then(|m| m.get(SETTING_KEY).cloned())
        .and_then(|v| serde_json::from_value::<ScreenshotExplainSettings>(v).ok())
        .is_some_and(|s| s.enabled)
}

fn offer_view(ledger: &ScreenshotOfferLedger, now_ms: u64) -> Option<Offer> {
    let pending = ledger.offer_live(now_ms)?;
    let name = Path::new(&pending.path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    Some(Offer {
        name,
        label: CHIP_LABEL.into(),
        dismiss: DISMISS_LABEL.into(),
        prompt: EXPLAIN_PROMPT.into(),
    })
}

/// Turn the switch on or off, keeping every other key in `app.json`. Off
/// takes down any offer that is up.
pub fn set_enabled(config_dir: &Path, on: bool, now_ms: u64) -> Result<View, String> {
    let value = serde_json::to_value(ScreenshotExplainSettings { enabled: on })
        .map_err(|e| e.to_string())?;
    grokhub_core::app_json::patch(config_dir, &[(SETTING_KEY, value)])?;
    let mut ledger = load_ledger(config_dir);
    if !on && ledger.pending.is_some() {
        dismiss_screenshot_offer(&mut ledger);
        save_ledger(config_dir, &ledger)?;
    }
    Ok(View {
        enabled: on,
        offer: if on { offer_view(&ledger, now_ms) } else { None },
    })
}

/// One look at `dirs`: a screenshot saved in the last two minutes, newer than
/// any seen before, puts up an offer (the cabin's tick). With the switch off
/// nothing is listed and any offer comes down.
pub fn check(config_dir: &Path, dirs: &[PathBuf], now_ms: u64) -> View {
    let settings = ScreenshotExplainSettings {
        enabled: enabled(config_dir),
    };
    let mut ledger = load_ledger(config_dir);
    let newest = if settings.enabled {
        newest_in_watch_dirs(dirs, now_ms)
    } else {
        None
    };
    let before = ledger.clone();
    let _ = tick_screenshot_offer(&mut ledger, &settings, newest, now_ms);
    if settings.enabled {
        ledger.set_last_tick_ms(now_ms);
    }
    if ledger != before {
        let _ = save_ledger(config_dir, &ledger);
    }
    View {
        enabled: settings.enabled,
        offer: if settings.enabled {
            offer_view(&ledger, now_ms)
        } else {
            None
        },
    }
}

/// The folders the cabin watches on this computer.
pub fn this_pc_dirs() -> Vec<PathBuf> {
    let Some(home) = grokhub_core::user_home() else {
        return Vec::new();
    };
    let os = if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    };
    let pictures = pictures_dir(&home);
    watch_dirs(&home, pictures.as_deref(), os)
}

/// Not now: the offer comes down; the next new screenshot offers again.
pub fn dismiss(config_dir: &Path) -> View {
    let mut ledger = load_ledger(config_dir);
    if ledger.pending.is_some() {
        dismiss_screenshot_offer(&mut ledger);
        let _ = save_ledger(config_dir, &ledger);
    }
    View {
        enabled: enabled(config_dir),
        offer: None,
    }
}

/// Explain: `stage` attaches the offered file to the composer. The offer
/// comes down once it is attached, or when the file is gone; a refusal (the
/// tray is full) keeps it up so Explain can be tapped again.
pub fn explain(
    config_dir: &Path,
    now_ms: u64,
    stage: impl FnOnce(&Path) -> Outcome,
) -> Result<Outcome, String> {
    let mut ledger = load_ledger(config_dir);
    let path = match ledger.offer_live(now_ms) {
        Some(p) if enabled(config_dir) => PathBuf::from(&p.path),
        _ => return Err(NO_OFFER.into()),
    };
    if !path.is_file() {
        dismiss_screenshot_offer(&mut ledger);
        save_ledger(config_dir, &ledger)?;
        return Err(GONE.into());
    }
    let outcome = stage(&path);
    if outcome.staged.is_some() {
        // Read again: a scan may have saved a newer offer while this one loaded.
        let mut ledger = load_ledger(config_dir);
        if ledger
            .pending
            .as_ref()
            .is_some_and(|p| Path::new(&p.path) == path)
        {
            dismiss_screenshot_offer(&mut ledger);
            save_ledger(config_dir, &ledger)?;
        }
    }
    Ok(outcome)
}

#[cfg(test)]
#[path = "screenshot_offer_tests.rs"]
mod tests;
