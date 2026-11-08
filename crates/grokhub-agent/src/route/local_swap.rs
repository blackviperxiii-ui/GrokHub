//! Hot-swap between the chat models on disk, and the learned idle timer.
//!
//! The Settings pick is the home model. Another downloaded chat model runs
//! instead only for a reason the running-model rows can say: the battery is
//! low (the smallest one on disk saves power), or the home model ran out of
//! memory at start (the largest smaller one stands in). A model in use is
//! never swapped: a request in flight keeps its model, and the swap waits for
//! the next request with nothing in flight. Only one model runs at a time,
//! because two models on one memory bus lose more than a third of their speed.

use std::path::{Path, PathBuf};

use grokhub_core::local_setup::{self, LocalModel, ModelKind};

/// Why a model other than the Settings pick runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StandIn {
    /// The battery is low, so the smallest model on disk runs.
    LowBattery,
    /// The Settings pick ran out of memory at start.
    OutOfMemory,
}

impl StandIn {
    /// The running-model row that says why.
    pub fn line(self) -> &'static str {
        match self {
            StandIn::LowBattery => {
                "Running a smaller model while the battery is low. Your pick comes back once it charges."
            }
            StandIn::OutOfMemory => {
                "Running a smaller model because your pick ran out of memory. It tries your pick again after the next unload."
            }
        }
    }
}

/// The least time between two swaps of a running model, so a battery reading
/// that flips back and forth doesn't reload the model each time.
pub const SWAP_COOLDOWN_SECS: u64 = 120;

/// What the next load does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Plan {
    /// Use what runs (or start the home model when nothing does).
    Keep,
    /// Stop what runs, if anything, and start `model`.
    Load {
        model: PathBuf,
        stand_in: Option<StandIn>,
    },
}

/// What [`plan`] looks at.
#[derive(Debug, Clone, Copy)]
pub struct Now<'a> {
    /// The Settings pick's file.
    pub home: &'a Path,
    /// What runs, and why when it isn't the pick.
    pub loaded: Option<(&'a Path, Option<StandIn>)>,
    /// The chat models on disk, with their files.
    pub on_disk: &'a [(&'static LocalModel, PathBuf)],
    pub low_battery: bool,
    /// "Keep the model loaded" is on: the pick stays even on a low battery.
    pub kept_loaded: bool,
    /// A model file was forced (`GROKHUB_LOCAL_MODEL`): nothing swaps.
    pub forced: bool,
    pub in_flight: usize,
    pub secs_since_swap: u64,
}

/// The model the next load should run.
pub fn plan(n: Now<'_>) -> Plan {
    let (target, stand_in) = match low_battery_pick(&n) {
        Some(p) => (p, Some(StandIn::LowBattery)),
        None => (n.home.to_path_buf(), None),
    };
    let Some((running, why)) = n.loaded else {
        return Plan::Load {
            model: target,
            stand_in,
        };
    };
    let settled = running == target
        // The stand-in for a pick that ran out of memory stays until the next
        // unload, so a request doesn't retry a start that just failed.
        || (why == Some(StandIn::OutOfMemory) && stand_in.is_none());
    if settled || n.in_flight > 0 || n.secs_since_swap < SWAP_COOLDOWN_SECS {
        return Plan::Keep;
    }
    Plan::Load {
        model: target,
        stand_in,
    }
}

/// On a low battery, the smallest chat model on disk when it is smaller than
/// the pick.
fn low_battery_pick(n: &Now<'_>) -> Option<PathBuf> {
    if !n.low_battery || n.kept_loaded || n.forced {
        return None;
    }
    let home = catalog_for(n.home)?;
    n.on_disk
        .iter()
        .filter(|(m, _)| m.run_mb < home.run_mb)
        .min_by_key(|(m, _)| m.run_mb)
        .map(|(_, p)| p.clone())
}

/// The model to start when `failed` ran out of memory: the largest chat model
/// on disk that needs less memory.
pub fn out_of_memory_stand_in(
    failed: &Path,
    on_disk: &[(&'static LocalModel, PathBuf)],
) -> Option<PathBuf> {
    let failed = catalog_for(failed)?;
    on_disk
        .iter()
        .filter(|(m, _)| m.run_mb < failed.run_mb)
        .max_by_key(|(m, _)| m.run_mb)
        .map(|(_, p)| p.clone())
}

fn catalog_for(file: &Path) -> Option<&'static LocalModel> {
    let name = file.file_name()?.to_str()?;
    local_setup::local_models().iter().find(|m| m.file == name)
}

/// The chat models downloaded into `dirs`, with their files.
pub fn on_disk(dirs: &[PathBuf]) -> Vec<(&'static LocalModel, PathBuf)> {
    local_setup::local_models()
        .iter()
        .filter(|m| m.kind == ModelKind::Chat && !m.is_drafter)
        .filter_map(|m| {
            dirs.iter()
                .map(|d| grokhub_core::model_download::final_path(d, m))
                .find(|p| p.is_file())
                .map(|p| (m, p))
        })
        .collect()
}

/// A reload within this long of an idle unload means the timer was too short.
pub const RELOAD_SOON_SECS: u64 = 60;
/// A reload this long after an idle unload means the timer can come down.
pub const RELOAD_LATE_SECS: u64 = 2 * 60 * 60;
/// The longest a learned timer grows to.
pub const LEARNED_MAX_SECS: u64 = 60 * 60;

/// The idle timer after a reload `gap_secs` after the last idle unload: it
/// doubles when the model came back within a minute and halves when it came
/// back hours later, never below `base` (the default) or above an hour.
pub fn learn(current: u64, base: u64, gap_secs: u64) -> u64 {
    let next = if gap_secs < RELOAD_SOON_SECS {
        current.saturating_mul(2)
    } else if gap_secs > RELOAD_LATE_SECS {
        current / 2
    } else {
        current
    };
    next.clamp(base, LEARNED_MAX_SECS.max(base))
}

const LEARNED_FILE: &str = "local_unload.json";

/// The learned idle timer for `model`, else `base`.
pub fn learned_secs(config_dir: &Path, model: &Path, base: u64) -> u64 {
    key(model)
        .and_then(|k| read_learned(config_dir).get(&k)?.as_u64())
        .map_or(base, |s| s.clamp(base, LEARNED_MAX_SECS.max(base)))
}

/// Store the learned idle timer for `model`.
pub fn save_learned(config_dir: &Path, model: &Path, secs: u64) -> Result<(), String> {
    let k = key(model).ok_or("the model file has no name")?;
    let mut v = read_learned(config_dir);
    v.insert(k, secs.into());
    let text = serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(config_dir).map_err(|e| e.to_string())?;
    std::fs::write(config_dir.join(LEARNED_FILE), format!("{text}\n")).map_err(|e| e.to_string())
}

fn key(model: &Path) -> Option<String> {
    model.file_name()?.to_str().map(str::to_owned)
}

fn read_learned(config_dir: &Path) -> serde_json::Map<String, serde_json::Value> {
    std::fs::read_to_string(config_dir.join(LEARNED_FILE))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

#[cfg(test)]
#[path = "local_swap_tests.rs"]
mod tests;
