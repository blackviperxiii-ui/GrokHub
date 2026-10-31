//! The small per-feature JSON ledgers under the config folder (nudge
//! watchers, PC health, print alerts, the day brief, the week recap, ask
//! and screenshot offers) share one loader and one saver here, so every one
//! of them gets `config::load_json`'s size cap, BOM strip and quarantine,
//! and [`TickGate`] keeps their cadence in memory so a frame before the
//! next tick never opens the file.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::config;

/// A ledger past this is quarantined, not parsed. The largest of them
/// (print alerts, with saved printers) stays under a few kilobytes.
pub(super) const LEDGER_CAP: usize = 4 * 1024 * 1024;

/// Where `file` lives: straight under the config folder.
pub(super) fn ledger_path(file: &str) -> PathBuf {
    config::config_dir().join(file)
}

/// The ledger in `file`, or its default when missing or empty. An
/// unreadable one is quarantined (`config::load_json`).
pub(super) fn load_ledger<T: Default + serde::de::DeserializeOwned>(file: &str) -> T {
    #[cfg(test)]
    READS.with(|n| n.set(n.get() + 1));
    config::load_json(&ledger_path(file), LEDGER_CAP)
}

/// Write the ledger to `file`, creating the config folder first. A write
/// that fails is dropped: the next tick rebuilds the same state.
pub(super) fn save_ledger<T: serde::Serialize>(file: &str, value: &T) {
    let Ok(text) = serde_json::to_string_pretty(value) else {
        return;
    };
    let path = ledger_path(file);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = config::atomic_write(&path, text.as_bytes());
}

#[cfg(test)]
thread_local! {
    static READS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// How many ledgers this thread loaded so far, for cadence tests.
#[cfg(test)]
pub(super) fn reads() -> usize {
    READS.with(|n| n.get())
}

/// When each ticker last ran this session, so the cadence check costs a
/// map lookup instead of a file read. The ledger's own `last_tick_ms`
/// still guards the first tick after a relaunch.
#[derive(Debug, Default)]
pub(super) struct TickGate {
    last: HashMap<&'static str, u64>,
}

impl TickGate {
    /// True when `key` never passed this session or passed at least `every`
    /// ms before `now`; a pass is marked at `now`.
    pub(super) fn due(&mut self, key: &'static str, now: u64, every: u64) -> bool {
        let due = self
            .last
            .get(key)
            .is_none_or(|&last| now.saturating_sub(last) >= every);
        if due {
            self.last.insert(key, now);
        }
        due
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_gate_passes_once_per_cadence_and_keys_apart() {
        let mut gate = TickGate::default();
        assert!(gate.due("a", 1_000, 500));
        assert!(!gate.due("a", 1_499, 500), "499 ms on: not yet");
        assert!(gate.due("a", 1_500, 500), "500 ms on: due");
        assert!(!gate.due("a", 1_999, 500));
        assert!(gate.due("b", 1_999, 500), "another key has its own clock");
        assert!(!gate.due("a", 900, 500), "a clock step back is not due");
    }

    #[test]
    fn a_ledger_round_trips_and_a_missing_one_is_default() {
        let root = config::test_config_root("ledger-store");
        let _pin = config::TestConfigDir::set(root.clone());
        let before = reads();
        let got: HashMap<String, u32> = load_ledger("none.json");
        assert!(got.is_empty());
        assert_eq!(reads(), before + 1, "the load helper counts each read");
        let mut want = HashMap::new();
        want.insert("ticks".to_string(), 7u32);
        save_ledger("some.json", &want);
        assert_eq!(ledger_path("some.json"), root.join("some.json"));
        let back: HashMap<String, u32> = load_ledger("some.json");
        assert_eq!(back, want);
        // A BOM (PowerShell's Set-Content) still parses; junk is quarantined.
        std::fs::write(root.join("bom.json"), "\u{feff}{\"ticks\":3}").unwrap();
        let bom: HashMap<String, u32> = load_ledger("bom.json");
        assert_eq!(bom.get("ticks"), Some(&3));
        std::fs::write(root.join("bad.json"), "{not json").unwrap();
        let bad: HashMap<String, u32> = load_ledger("bad.json");
        assert!(bad.is_empty());
        assert!(
            !root.join("bad.json").exists(),
            "quarantined, not left to be overwritten"
        );
        let _ = std::fs::remove_dir_all(root);
    }
}
