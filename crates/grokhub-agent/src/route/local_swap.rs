//! Hot-swap between the chat models on disk, and the learned idle timer.
//!
//! The Settings pick is the home model. Another downloaded chat model runs
//! instead only for a reason the running-model rows can say: the battery is
//! low, a game is running, the machine saves power or memory, less memory is
//! free than the pick needs, the pick ran out of memory at start, or the
//! computer is free and the task is hard ([`super::local_scale`]). A model in
//! use is never swapped: a request in flight keeps its model, and the swap
//! waits for the next request with nothing in flight. A step up to a bigger
//! model never makes a reply wait: the reply starts on the model that runs,
//! and the bigger one loads once the chat goes quiet. Only one model runs at
//! a time, because two models on one memory bus lose more than a third of
//! their speed.

use std::path::{Path, PathBuf};

use grokhub_core::local_setup::{self, LocalModel};

use super::local_scale::{self, Need, Scale};

/// Why a model other than the Settings pick runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StandIn {
    /// The battery is low, so the smallest model on disk runs.
    LowBattery,
    /// The Settings pick ran out of memory at start.
    OutOfMemory,
    /// A game runs: the lightest model on disk.
    Gaming,
    /// The machine saves power or memory, and the task is light.
    Lighter,
    /// Less memory is free than the pick needs right now.
    Tight,
    /// The computer is free and the task is hard: a bigger model.
    Performance,
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
            StandIn::Gaming => {
                "Running the lightest model while a game runs. Your pick comes back after the game."
            }
            StandIn::Lighter => {
                "Running a smaller model for light tasks while the computer saves power. Hard tasks get your pick."
            }
            StandIn::Tight => {
                "Running a smaller model because less memory is free than your pick needs. Your pick comes back when there's room."
            }
            StandIn::Performance => {
                "Running a bigger model than your pick for hard tasks while the computer is free. It steps back down when the computer gets busy."
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
    /// A bigger model fits: this request keeps what runs, and `model` loads
    /// once the chat is quiet.
    Later {
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
    /// "Keep the model loaded" is on: the pick stays, whatever happens.
    pub kept_loaded: bool,
    /// A model file was forced (`GROKHUB_LOCAL_MODEL`): nothing swaps.
    pub forced: bool,
    pub in_flight: usize,
    pub secs_since_swap: u64,
    /// What this request's task needs.
    pub need: Need,
    /// The governor and the room the machine has.
    pub scale: Scale,
}

/// The model the next load should run.
pub fn plan(n: Now<'_>) -> Plan {
    // Background work rides on what is loaded; it never swaps a model.
    if n.need == Need::Background && n.loaded.is_some() {
        return Plan::Keep;
    }
    let (target, stand_in) = target(&n);
    let Some((running, why)) = n.loaded else {
        return Plan::Load {
            model: target,
            stand_in,
        };
    };
    let settled = running == target
        // The stand-in for a pick that ran out of memory stays until the next
        // unload, so a request doesn't retry a start that just failed.
        || (why == Some(StandIn::OutOfMemory) && stand_in.is_none())
        || stays_big(&n, running, why, stand_in);
    if settled || n.in_flight > 0 || n.secs_since_swap < SWAP_COOLDOWN_SECS {
        return Plan::Keep;
    }
    let bigger = match (catalog_for(&target), catalog_for(running)) {
        (Some(t), Some(r)) => t.run_mb > r.run_mb,
        _ => false,
    };
    if bigger {
        return Plan::Later {
            model: target,
            stand_in,
        };
    }
    Plan::Load {
        model: target,
        stand_in,
    }
}

/// A bigger model loaded for hard work stays for lighter asks in between
/// while the computer is still free and it still fits: swapping down and
/// back up would cost two loads.
fn stays_big(
    n: &Now<'_>,
    running: &Path,
    why: Option<StandIn>,
    target_why: Option<StandIn>,
) -> bool {
    if why != Some(StandIn::Performance) || target_why.is_some() {
        return false;
    }
    let (Some(room), Some(m)) = (n.scale.room, catalog_for(running)) else {
        return false;
    };
    !n.scale.gaming
        && !n.scale.saving
        && local_scale::performance_ready(&room)
        && local_scale::fits(m, &room, m.run_mb, 0)
}

/// The smallest chat model on disk lighter than `home`: what a game, a low
/// battery or a light ask runs on.
fn smallest_below(
    home: &LocalModel,
    on_disk: &[(&'static LocalModel, PathBuf)],
) -> Option<PathBuf> {
    on_disk
        .iter()
        .filter(|(m, _)| m.run_mb < home.run_mb)
        .min_by_key(|(m, _)| m.run_mb)
        .map(|(_, p)| p.clone())
}

/// The model a game would swap `home` for, if one is on disk.
pub fn gaming_model(home: &Path, on_disk: &[(&'static LocalModel, PathBuf)]) -> Option<PathBuf> {
    smallest_below(catalog_for(home)?, on_disk)
}

/// The model this request should run on, and why when it isn't the pick.
fn target(n: &Now<'_>) -> (PathBuf, Option<StandIn>) {
    let home_path = n.home.to_path_buf();
    if n.kept_loaded || n.forced {
        return (home_path, None);
    }
    let Some(home) = catalog_for(n.home) else {
        return (home_path, None);
    };
    let smallest = || smallest_below(home, n.on_disk);
    if n.low_battery {
        if let Some(p) = smallest() {
            return (p, Some(StandIn::LowBattery));
        }
    }
    if n.scale.gaming {
        return smallest().map_or((home_path, None), |p| (p, Some(StandIn::Gaming)));
    }
    let resident = n
        .loaded
        .and_then(|(p, _)| catalog_for(p))
        .map_or(0, |m| m.run_mb);
    if let Some(room) = n.scale.room {
        if n.need == Need::Hard && !n.scale.saving && local_scale::performance_ready(&room) {
            let bigger = n
                .on_disk
                .iter()
                .filter(|(m, _)| m.run_mb > home.run_mb)
                .filter(|(m, _)| local_scale::fits(m, &room, resident, local_scale::HEADROOM_MB))
                .filter(|(m, _)| {
                    local_scale::est_tok_s(m, &room, resident)
                        >= local_scale::PERFORMANCE_FLOOR_TOK_S
                })
                .max_by_key(|(m, _)| m.run_mb);
            if let Some((_, p)) = bigger {
                return (p.clone(), Some(StandIn::Performance));
            }
        }
        if !local_scale::fits(home, &room, resident, 0) {
            let fitting = n
                .on_disk
                .iter()
                .filter(|(m, _)| m.run_mb < home.run_mb)
                .filter(|(m, _)| local_scale::fits(m, &room, resident, 0))
                .max_by_key(|(m, _)| m.run_mb);
            if let Some((_, p)) = fitting {
                return (p.clone(), Some(StandIn::Tight));
            }
        }
    }
    if n.scale.saving {
        let lighter = match n.need {
            Need::Hard => None,
            Need::Light | Need::Background => smallest(),
            Need::Normal => n
                .on_disk
                .iter()
                .filter(|(m, _)| m.run_mb * 2 <= home.run_mb)
                .max_by_key(|(m, _)| m.run_mb)
                .map(|(_, p)| p.clone()),
        };
        if let Some(p) = lighter {
            return (p, Some(StandIn::Lighter));
        }
    }
    (home_path, None)
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

/// The running-model row's mode: Gaming while a game holds the GPU,
/// Performance while a bigger model runs for hard work, else Balanced.
pub fn mode(stand_in: Option<StandIn>, scale: &Scale) -> local_scale::Mode {
    if scale.gaming || stand_in == Some(StandIn::Gaming) {
        local_scale::Mode::Gaming
    } else if stand_in == Some(StandIn::Performance) {
        local_scale::Mode::Performance
    } else {
        local_scale::Mode::Balanced
    }
}

pub(crate) fn catalog_for(file: &Path) -> Option<&'static LocalModel> {
    let name = file.file_name()?.to_str()?;
    local_setup::local_models().iter().find(|m| m.file == name)
}

/// The chat models downloaded into `dirs`, with their files.
pub fn on_disk(dirs: &[PathBuf]) -> Vec<(&'static LocalModel, PathBuf)> {
    local_setup::local_models()
        .iter()
        .filter(|m| m.is_chat())
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
