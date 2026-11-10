//! `chips.json`: the quick-chip memory every app on this PC shares.
//!
//! One loader and one saver, so the egui cabin and the desktop shell read and
//! write the same file the same way: a lock file serialises writers, the
//! write is private, fsynced and renamed into place, an unreadable file is set
//! aside instead of being overwritten, and every load runs the migration.
//!
//! The memory also keeps a short local log of what the chip row showed and
//! what happened next. It never leaves the PC and holds no chip text: each
//! chip is a short hash of its memory key, its slot and the chance the ranker
//! gave it (the propensity offline evaluation needs).

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::chips::{
    chip_memory_key, prune_retired_chip_memory, ChipMemory, QuickChip, CHIP_MEMORY_VERSION,
};

/// The store's file name inside the config folder.
pub const CHIP_STORE_FILE: &str = "chips.json";
/// A chips.json past this is not ours (80 habits and a few hundred log rows
/// are well under 1 MiB); it is set aside, not parsed.
const CHIP_STORE_CAP: u64 = 8 * 1024 * 1024;
/// Newest rows kept in the local chip log.
pub const CHIP_LOG_MAX: usize = 400;
/// Hex characters of a chip's log key.
const LOG_KEY_HEX: usize = 12;

/// One time the chip row was shown, and what the person did with it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChipEvent {
    /// Unix ms when the row was shown.
    pub at: u64,
    /// Which row: `composer`, `home`, …
    pub surface: String,
    /// The chips in the order shown.
    pub shown: Vec<ShownChip>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<ChipOutcome>,
    /// A row held back to compare: nothing was shown.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub holdout: bool,
    /// Unix ms of the person's first step (a message) after the row.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_at: Option<u64>,
}

/// One chip in a shown row. No text: the key is a hash.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShownChip {
    pub key: String,
    /// Where the chip came from (`pred`, `learn`, `radar`, …): the id's first
    /// word, which code writes, never the person.
    pub source: String,
    /// 0 = first slot.
    pub slot: u8,
    /// The chance the ranker gave this chip this slot: 1.0 while it has no
    /// history to explore with, its real odds after.
    pub p: f32,
    /// The ranker's features for this chip (numbers only), so a pick or a
    /// hide can teach it later. Empty for a row the ranker did not build.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub x: Vec<f32>,
}

/// What the person did with a shown row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum ChipOutcome {
    Picked { key: String, slot: u8, at: u64 },
    Dismissed { key: String, slot: u8, at: u64 },
}

/// The store's path inside a config folder.
pub fn chip_store_path(dir: &Path) -> PathBuf {
    dir.join(CHIP_STORE_FILE)
}

fn lock_path(dir: &Path) -> PathBuf {
    dir.join(format!("{CHIP_STORE_FILE}.lock"))
}

/// Hold the store's lock file: shared to read, exclusive to write. A folder
/// or filesystem without locks still works, just unserialised.
fn hold_lock(dir: &Path, exclusive: bool) -> Option<fs::File> {
    fs::create_dir_all(dir).ok()?;
    let f = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(lock_path(dir))
        .ok()?;
    let held = if exclusive { f.lock() } else { f.lock_shared() };
    held.ok().map(|()| f)
}

/// Read and migrate the chip memory in `dir`. A missing or empty file is a
/// fresh memory; an unreadable one is set aside (and noted) so the next save
/// does not write a blank memory over it.
pub fn load_chip_memory(dir: &Path) -> ChipMemory {
    let path = chip_store_path(dir);
    let read = {
        let _lock = hold_lock(dir, false);
        read_store(&path)
    };
    let mut mem = match read {
        StoreRead::Missing => return ChipMemory::default(),
        StoreRead::Unusable => {
            crate::recovered::quarantine(&path, crate::recovered::RESTORED_DEFAULTS);
            return ChipMemory::default();
        }
        StoreRead::Text(raw) => {
            let raw = raw.strip_prefix('\u{feff}').unwrap_or(&raw);
            if raw.trim().is_empty() {
                return ChipMemory::default();
            }
            match serde_json::from_str::<ChipMemory>(raw) {
                Ok(mem) => mem,
                Err(_) => {
                    crate::recovered::quarantine(&path, crate::recovered::RESTORED_DEFAULTS);
                    return ChipMemory::default();
                }
            }
        }
    };
    if migrate_chip_memory(&mut mem) {
        let _ = save_chip_memory(dir, &mem);
    }
    mem
}

/// Write the chip memory to `dir` under the store lock. Private to the user,
/// fsynced, then renamed into place.
pub fn save_chip_memory(dir: &Path, mem: &ChipMemory) -> Result<(), String> {
    let body = serde_json::to_string_pretty(mem).map_err(|e| e.to_string())?;
    let _lock = hold_lock(dir, true);
    crate::work_file::write_private_atomic(&chip_store_path(dir), body.as_bytes())
}

/// Bring any older memory to this version: the load scrubs (retired chips,
/// shell lines, paths, chat text), a capped log, and the version stamp. True
/// when anything changed and the file should be written back.
pub fn migrate_chip_memory(mem: &mut ChipMemory) -> bool {
    let mut changed = prune_retired_chip_memory(mem);
    if mem.events.len() > CHIP_LOG_MAX {
        let cut = mem.events.len() - CHIP_LOG_MAX;
        mem.events.drain(..cut);
        changed = true;
    }
    if mem.version != CHIP_MEMORY_VERSION {
        mem.version = CHIP_MEMORY_VERSION;
        changed = true;
    }
    changed
}

enum StoreRead {
    Missing,
    Unusable,
    Text(String),
}

fn read_store(path: &Path) -> StoreRead {
    let Ok(meta) = fs::metadata(path) else {
        return StoreRead::Missing;
    };
    if meta.len() > CHIP_STORE_CAP {
        return StoreRead::Unusable;
    }
    let Ok(mut f) = fs::File::open(path) else {
        return StoreRead::Missing;
    };
    let mut raw = String::new();
    match f.read_to_string(&mut raw) {
        Ok(_) => StoreRead::Text(raw),
        Err(_) => StoreRead::Unusable,
    }
}

/// A chip's key in the log: a short hash of its memory key, so the log never
/// holds a project name or anything typed.
pub fn chip_log_key(chip: &QuickChip) -> String {
    let digest = Sha256::digest(chip_memory_key(chip).as_bytes());
    hex::encode(digest)[..LOG_KEY_HEX].to_string()
}

/// The first word of a chip id, which names the code that made it.
fn chip_source(chip: &QuickChip) -> String {
    chip.id
        .split(['-', ':'])
        .next()
        .filter(|w| !w.is_empty() && w.len() <= 16 && w.bytes().all(|b| b.is_ascii_alphanumeric()))
        .unwrap_or("other")
        .to_ascii_lowercase()
}

/// Note that `surface` showed `chips`. A row identical to the last one still
/// waiting for an outcome is the same showing, not a new one, so a repaint or
/// a keystroke that changes nothing adds no row.
pub fn log_chips_shown(mem: &mut ChipMemory, surface: &str, chips: &[QuickChip], now: u64) {
    crate::learn::hub::note_all(shown_signals(mem, surface, chips, now));
}

/// [`log_chips_shown`] without the engine: logs the row and returns what the
/// learning engine is told about it.
pub(crate) fn shown_signals(
    mem: &mut ChipMemory,
    surface: &str,
    chips: &[QuickChip],
    now: u64,
) -> Vec<crate::learn::Signal> {
    let mut told = Vec::new();
    if chips.is_empty() {
        return told;
    }
    let shown: Vec<ShownChip> = chips
        .iter()
        .enumerate()
        .map(|(slot, c)| {
            let key = chip_log_key(c);
            let note = crate::chip_rank::ranked_note(mem, surface, &key);
            ShownChip {
                source: chip_source(c),
                slot: u8::try_from(slot).unwrap_or(u8::MAX),
                p: note.as_ref().map_or(1.0, |n| n.p),
                x: note.map(|n| n.x).unwrap_or_default(),
                key,
            }
        })
        .collect();
    let same = |e: &ChipEvent| {
        e.surface == surface
            && e.outcome.is_none()
            && e.shown.len() == shown.len()
            && e.shown.iter().zip(&shown).all(|(a, b)| a.key == b.key)
    };
    if mem.events.last().is_some_and(same) {
        return told;
    }
    // The last open row on this surface was passed over for this one.
    if let Some(prev) = mem
        .events
        .iter()
        .rev()
        .find(|e| e.surface == surface)
        .filter(|e| e.outcome.is_none())
    {
        let shown = prev.shown.clone();
        crate::chip_rank::learn_from_row(&mut mem.ranker, &shown, None);
        let keys = shown.iter().map(|s| s.key.as_str());
        told.extend(crate::learn::sources::chips_passed_over(surface, keys, now));
    }
    let keys = shown.iter().map(|s| s.key.as_str());
    told.extend(crate::learn::sources::chips_shown(surface, keys, now));
    mem.events.push(ChipEvent {
        at: now,
        surface: surface.to_string(),
        shown,
        ..ChipEvent::default()
    });
    if mem.events.len() > CHIP_LOG_MAX {
        let cut = mem.events.len() - CHIP_LOG_MAX;
        mem.events.drain(..cut);
    }
    told
}

/// Note what the person did with `chip` on `surface`: the newest open showing
/// that held it gets the outcome. A chip no logged row showed adds nothing.
pub fn log_chip_outcome(
    mem: &mut ChipMemory,
    surface: &str,
    chip: &QuickChip,
    picked: bool,
    now: u64,
) {
    crate::learn::hub::note_all(outcome_signals(mem, surface, chip, picked, now));
}

/// [`log_chip_outcome`] without the engine: logs the outcome and returns
/// what the learning engine is told about it.
pub(crate) fn outcome_signals(
    mem: &mut ChipMemory,
    surface: &str,
    chip: &QuickChip,
    picked: bool,
    now: u64,
) -> Vec<crate::learn::Signal> {
    let key = chip_log_key(chip);
    let Some(event) = mem.events.iter_mut().rev().find(|e| {
        e.surface == surface && e.outcome.is_none() && e.shown.iter().any(|s| s.key == key)
    }) else {
        return Vec::new();
    };
    let slot = event
        .shown
        .iter()
        .find(|s| s.key == key)
        .map_or(0, |s| s.slot);
    let told = crate::learn::sources::chip_outcome(surface, &key, picked, now);
    // A habit chip's tap or hide is also the answer to its rule.
    if let Some(rule) = crate::learn::habit::chip_rule(&chip.id) {
        use crate::learn::decide::Outcome;
        let outcome = if picked {
            Outcome::Accepted
        } else {
            Outcome::Declined
        };
        crate::learn::hub::decision_outcome(&rule, outcome, now);
    }
    let outcome = if picked {
        ChipOutcome::Picked { key, slot, at: now }
    } else {
        ChipOutcome::Dismissed { key, slot, at: now }
    };
    let shown = event.shown.clone();
    event.outcome = Some(outcome.clone());
    crate::chip_rank::learn_from_row(&mut mem.ranker, &shown, Some(&outcome));
    told
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chips::{ChipHit, ChipKind};

    fn chip(id: &str, value: &str) -> QuickChip {
        QuickChip {
            id: id.into(),
            label: id.into(),
            value: value.into(),
            kind: ChipKind::Chat,
            score: 1.0,
            hint: String::new(),
            primary: false,
        }
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "grokhub-chip-store-{name}-{}-{}",
            std::process::id(),
            crate::now_ms()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("dir");
        dir
    }

    fn hit(key: &str, label: &str, value: &str, kind: ChipKind) -> ChipHit {
        ChipHit {
            key: key.into(),
            label: label.into(),
            value: value.into(),
            kind,
            uses: 3,
            typed_uses: 0,
            picks: 3,
            last_used_at: 1,
            hour_hits: vec![],
            successes: 0,
            failures: 0,
            context_tags: vec![],
            dismisses: 0,
            dismiss_hours: vec![],
            last_dismissed_at: 0,
        }
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = temp_dir("round");
        let mut mem = ChipMemory {
            total_events: 7,
            ..ChipMemory::default()
        };
        mem.hits.push(hit(
            "id:brief",
            "Cabin brief",
            "Give me a brief.",
            ChipKind::Nav,
        ));
        log_chips_shown(&mut mem, "composer", &[chip("pred-brief", "x")], 5);
        save_chip_memory(&dir, &mem).expect("save");
        let back = load_chip_memory(&dir);
        assert_eq!(back, mem);
        assert_eq!(back.version, CHIP_MEMORY_VERSION);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_v2_file_with_mode_and_shell_habits_migrates() {
        let dir = temp_dir("v2");
        let old = r#"{
          "version": 2,
          "hits": [
            {"key":"id:route-think","label":"Think Harder","value":"__mode:think","kind":"mode","uses":4},
            {"key":"id:pred-shell","label":"git status","value":"$ git status","kind":"shell","uses":2},
            {"key":"id:brief","label":"Cabin brief","value":"__nav:imagine","kind":"nav","uses":2}
          ],
          "lastChipKey": "id:route-think"
        }"#;
        fs::write(chip_store_path(&dir), old).expect("write");
        let mem = load_chip_memory(&dir);
        assert_eq!(mem.version, CHIP_MEMORY_VERSION);
        assert!(mem.hits.iter().all(|h| !h.value.starts_with("__mode:")));
        assert!(mem.hits.iter().all(|h| h.key != "id:route-think"));
        assert_eq!(mem.last_chip_key, None);
        let shell = mem
            .hits
            .iter()
            .find(|h| h.kind == ChipKind::Shell)
            .expect("shell");
        assert!(
            shell.label.is_empty() && shell.value.is_empty(),
            "{shell:?}"
        );
        assert!(mem.hits.iter().any(|h| h.key == "id:brief"));
        // The migration was written back.
        let disk = fs::read_to_string(chip_store_path(&dir)).expect("read");
        assert!(
            !disk.contains("__mode:") && !disk.contains("git status"),
            "{disk}"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_unreadable_file_is_set_aside_not_overwritten() {
        let dir = temp_dir("bad");
        fs::write(chip_store_path(&dir), "{ not json").expect("write");
        let mem = load_chip_memory(&dir);
        assert_eq!(mem, ChipMemory::default());
        assert!(
            !chip_store_path(&dir).exists(),
            "the bad file was moved aside"
        );
        let aside = fs::read_dir(&dir)
            .expect("list")
            .filter_map(Result::ok)
            .any(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("chips.json.corrupt-")
            });
        assert!(aside);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_and_empty_files_are_a_fresh_memory() {
        let dir = temp_dir("empty");
        assert_eq!(load_chip_memory(&dir), ChipMemory::default());
        fs::write(chip_store_path(&dir), "\u{feff}  \n").expect("write");
        assert_eq!(load_chip_memory(&dir), ChipMemory::default());
        assert!(
            chip_store_path(&dir).exists(),
            "an empty file is not corrupt"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_log_holds_no_chip_text() {
        let mut mem = ChipMemory::default();
        let secret = chip("resume:continue:Secret Project", "Pick up Secret Project");
        log_chips_shown(&mut mem, "composer", std::slice::from_ref(&secret), 10);
        let json = serde_json::to_string(&mem.events).expect("json");
        assert!(!json.to_ascii_lowercase().contains("secret"), "{json}");
        assert_eq!(mem.events[0].shown[0].source, "resume");
        assert_eq!(mem.events[0].shown[0].key.len(), LOG_KEY_HEX);
        let llm = chip("llm-0-Fix the blender", "Fix the blender");
        log_chips_shown(&mut mem, "composer", &[llm], 11);
        let json = serde_json::to_string(&mem.events).expect("json");
        assert!(!json.contains("blender"), "{json}");
    }

    #[test]
    fn a_repaint_is_one_showing_and_an_outcome_closes_it() {
        let mut mem = ChipMemory::default();
        let row = [chip("pred-a", "a"), chip("learn-b", "b")];
        log_chips_shown(&mut mem, "composer", &row, 1);
        log_chips_shown(&mut mem, "composer", &row, 2);
        assert_eq!(mem.events.len(), 1, "same row again is the same showing");
        log_chips_shown(&mut mem, "home", &row, 3);
        assert_eq!(mem.events.len(), 2, "another surface is another showing");

        log_chip_outcome(&mut mem, "composer", &row[1], true, 4);
        assert_eq!(
            mem.events[0].outcome,
            Some(ChipOutcome::Picked {
                key: chip_log_key(&row[1]),
                slot: 1,
                at: 4
            })
        );
        assert_eq!(mem.events[1].outcome, None);
        log_chips_shown(&mut mem, "composer", &row, 5);
        assert_eq!(
            mem.events.len(),
            3,
            "after an outcome the next showing is new"
        );

        log_chip_outcome(&mut mem, "home", &row[0], false, 6);
        assert!(matches!(
            mem.events[1].outcome,
            Some(ChipOutcome::Dismissed { slot: 0, .. })
        ));
        log_chip_outcome(&mut mem, "composer", &chip("never-shown", "z"), true, 7);
        assert_eq!(mem.events[2].outcome, None);
    }

    #[test]
    fn the_log_is_capped() {
        let mut mem = ChipMemory::default();
        for i in 0..(CHIP_LOG_MAX + 25) {
            let c = chip(&format!("pred-{i}"), "x");
            log_chips_shown(&mut mem, "composer", &[c], i as u64);
        }
        assert_eq!(mem.events.len(), CHIP_LOG_MAX);
        assert_eq!(mem.events[0].at, 25);
    }

    #[test]
    fn two_writers_leave_a_whole_file() {
        let dir = temp_dir("writers");
        let handles: Vec<_> = (0..4)
            .map(|n| {
                let dir = dir.clone();
                std::thread::spawn(move || {
                    for i in 0..10 {
                        let mem = ChipMemory {
                            total_events: n * 100 + i,
                            ..ChipMemory::default()
                        };
                        save_chip_memory(&dir, &mem).expect("save");
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().expect("writer");
        }
        let raw = fs::read_to_string(chip_store_path(&dir)).expect("read");
        serde_json::from_str::<ChipMemory>(&raw).expect("one whole file");
        let _ = fs::remove_dir_all(&dir);
    }
}
