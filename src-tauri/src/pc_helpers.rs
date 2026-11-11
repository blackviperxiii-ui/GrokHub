//! Settings → Your PC → PC helpers: the wellbeing and focus switches the
//! cabin has, kept in the same files with the same defaults so the two apps
//! agree. Focus help is `focusHelp` in `app.json` (off by default, written
//! through `grokhub_core::app_json` so nothing else in the file is touched).
//! Wind-down is `wind-down.json` and the daily rhythm is `rhythm.json`, both
//! the cores' own files (`grokhub_core::{wind_down, rhythm}`).

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};

use grokhub_core::rhythm::{self, hour_label, Rhythm};
use grokhub_core::wind_down::{self, idle_label, IDLE_CHOICES};
use serde::Serialize;

const FOCUS_KEY: &str = "focusHelp";

static FILES: Mutex<()> = Mutex::new(());

/// The last wind-down read couldn't see a game, a fullscreen app or GPU work.
static BLIND: AtomicBool = AtomicBool::new(false);

fn files() -> MutexGuard<'static, ()> {
    FILES.lock().unwrap_or_else(|e| e.into_inner())
}

/// Run `job` while no settings save is in the middle of writing these files.
pub fn locked<T>(job: impl FnOnce() -> T) -> T {
    let _files = files();
    job()
}

/// Record whether the last read left wind-down unable to see what's running.
pub fn note_blind(blind: bool) {
    BLIND.store(blind, Ordering::Relaxed);
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SleepChoice {
    pub minutes: u32,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PcHelpers {
    /// Suggest help for the app the user was just in. Off by default.
    pub focus_help: bool,
    /// Sleep the PC after it sits idle. Off by default.
    pub wind_down: bool,
    /// The saved idle time in minutes, once one is picked.
    pub auto_sleep_min: Option<u32>,
    pub sleep_choices: Vec<SleepChoice>,
    /// This PC didn't report a game, fullscreen or GPU signal at the last
    /// read, so it won't sleep on its own.
    pub sleep_blind: bool,
    /// Learn the daily rhythm. On by default, as in the cabin.
    pub rhythm: bool,
    /// Also count when games were last played.
    pub rhythm_games: bool,
    pub rhythm_minutes: u32,
    /// What the rhythm has learned so far, in a sentence.
    pub rhythm_summary: String,
}

/// "8 AM, 6 PM–10 PM" from the usually-active hours.
fn active_ranges(r: &Rhythm) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut h = 0;
    while h < 24 {
        if !r.usually_active(h) {
            h += 1;
            continue;
        }
        let start = h;
        while h + 1 < 24 && r.usually_active(h + 1) {
            h += 1;
        }
        out.push(if start == h {
            hour_label(start)
        } else {
            format!("{}–{}", hour_label(start), hour_label(h))
        });
        h += 1;
    }
    out.join(", ")
}

fn rhythm_summary(r: &Rhythm) -> String {
    if !r.enough() {
        return "Still learning. A few days of use fills this in.".into();
    }
    let mut line = format!("Usually on: {}.", active_ranges(r));
    if let Some((start, end)) = r.suggested_quiet() {
        line.push_str(&format!(
            " Usually away: {}–{}.",
            hour_label(start),
            hour_label(end)
        ));
    }
    line
}

fn rhythm_path(config_dir: &Path) -> std::path::PathBuf {
    config_dir.join(rhythm::FILE_NAME)
}

pub fn read(config_dir: &Path) -> PcHelpers {
    let _files = files();
    view(config_dir)
}

fn view(config_dir: &Path) -> PcHelpers {
    let tracker = wind_down::load(config_dir);
    let r = rhythm::load(&rhythm_path(config_dir));
    PcHelpers {
        focus_help: grokhub_core::app_json::bool_or(config_dir, FOCUS_KEY, false),
        wind_down: tracker.on,
        auto_sleep_min: tracker.auto_sleep_min,
        sleep_choices: IDLE_CHOICES
            .iter()
            .map(|&minutes| SleepChoice {
                minutes,
                label: idle_label(u64::from(minutes)),
            })
            .collect(),
        sleep_blind: BLIND.load(Ordering::Relaxed),
        rhythm: r.on,
        rhythm_games: r.games_on,
        rhythm_minutes: r.total_minutes(),
        rhythm_summary: rhythm_summary(&r),
    }
}

/// Turn one switch on or off: `focusHelp`, `windDown`, `rhythm` or
/// `rhythmGames`. Nothing changes when the file can't be written.
pub fn set_switch(config_dir: &Path, key: &str, on: bool) -> Result<PcHelpers, String> {
    let _files = files();
    match key {
        FOCUS_KEY => grokhub_core::app_json::patch(config_dir, &[(FOCUS_KEY, on.into())])?,
        "windDown" => {
            let mut tracker = wind_down::load(config_dir);
            if on {
                tracker.on = true;
            } else {
                tracker.turn_off();
            }
            wind_down::save(config_dir, &tracker)?;
        }
        "rhythm" | "rhythmGames" => {
            let path = rhythm_path(config_dir);
            let mut r = rhythm::load(&path);
            if key == "rhythm" {
                r.on = on;
            } else {
                r.games_on = on;
            }
            rhythm::save(&path, &r)?;
        }
        other => return Err(format!("Unknown PC helper: {other}")),
    }
    Ok(view(config_dir))
}

/// Save an idle time; the PC sleeps after that long idle from now on.
pub fn set_auto_sleep(config_dir: &Path, min: u32) -> Result<PcHelpers, String> {
    if !IDLE_CHOICES.contains(&min) {
        return Err(format!("{min} minutes isn't one of the idle times."));
    }
    let _files = files();
    let mut tracker = wind_down::load(config_dir);
    tracker.set_auto_sleep(min);
    wind_down::save(config_dir, &tracker)?;
    Ok(view(config_dir))
}

/// Forget every learned count; the switches and the brief hour stay.
pub fn forget_rhythm(config_dir: &Path) -> Result<PcHelpers, String> {
    let _files = files();
    let path = rhythm_path(config_dir);
    let mut r = rhythm::load(&path);
    r.reset();
    rhythm::save(&path, &r)?;
    Ok(view(config_dir))
}

#[cfg(test)]
#[path = "pc_helpers_tests.rs"]
mod tests;
