//! Who fires scheduled automations right now (cross-platform Phase 3b).
//!
//! The egui cabin and the Tauri shell read the same `automations.json`. While
//! the cabin is open it writes a beat to `scheduler-lease.json` every
//! [`LEASE_BEAT_MS`], and the shell fires nothing while that beat is fresh.
//! Once the cabin is closed the beat goes stale and the shell takes over,
//! beating as `shell` so a second shell process defers to the first.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

pub const LEASE_FILE: &str = "scheduler-lease.json";
/// How often the owner rewrites its beat.
pub const LEASE_BEAT_MS: u64 = 30_000;
/// A beat younger than this holds the lease. Three missed beats free it.
pub const LEASE_FRESH_MS: u64 = 90_000;
/// The file holds three small fields; anything bigger is not a lease.
const LEASE_CAP: u64 = 4 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LeaseOwner {
    Cabin,
    Shell,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchedulerLease {
    pub owner: LeaseOwner,
    pub pid: u32,
    pub beat_ms: u64,
}

pub fn lease_path(config_dir: &Path) -> PathBuf {
    config_dir.join(LEASE_FILE)
}

/// Fresh while the beat is under [`LEASE_FRESH_MS`] old. A beat stamped far
/// in the future (a clock that jumped back) is stale, so it cannot hold the
/// lease forever.
pub fn lease_fresh(lease: &SchedulerLease, now_ms: u64) -> bool {
    lease.beat_ms <= now_ms.saturating_add(LEASE_FRESH_MS)
        && now_ms.saturating_sub(lease.beat_ms) < LEASE_FRESH_MS
}

/// The shell fires only when no fresh lease belongs to someone else: the
/// cabin while it is open, or another shell process.
pub fn shell_may_fire(lease: Option<&SchedulerLease>, pid: u32, now_ms: u64) -> bool {
    match lease {
        Some(l) if lease_fresh(l, now_ms) => l.owner == LeaseOwner::Shell && l.pid == pid,
        _ => true,
    }
}

/// A missing, oversized or broken file is no lease.
pub fn read_lease(config_dir: &Path) -> Option<SchedulerLease> {
    let path = lease_path(config_dir);
    if std::fs::metadata(&path).ok()?.len() > LEASE_CAP {
        return None;
    }
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

/// Writes through a temp file named for this process and a rename, so a
/// reader never sees half a lease.
pub fn write_lease(config_dir: &Path, lease: &SchedulerLease) -> Result<(), String> {
    std::fs::create_dir_all(config_dir).map_err(|e| e.to_string())?;
    let raw = serde_json::to_string(lease).map_err(|e| e.to_string())?;
    let tmp = config_dir.join(format!("{LEASE_FILE}.{}.tmp", lease.pid));
    std::fs::write(&tmp, raw).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, lease_path(config_dir)).map_err(|e| e.to_string())
}

/// The cabin's beat, called from its pulse. `last` holds when this process
/// last wrote; the write happens at most once per [`LEASE_BEAT_MS`]. True when
/// it wrote.
pub fn beat_cabin_lease(config_dir: &Path, now_ms: u64, last: &AtomicU64) -> bool {
    let prev = last.load(Ordering::Relaxed);
    if prev != 0 && now_ms.saturating_sub(prev) < LEASE_BEAT_MS {
        return false;
    }
    last.store(now_ms, Ordering::Relaxed);
    let lease = SchedulerLease {
        owner: LeaseOwner::Cabin,
        pid: std::process::id(),
        beat_ms: now_ms,
    };
    write_lease(config_dir, &lease).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_800_000_000_000;

    fn lease(owner: LeaseOwner, pid: u32, beat_ms: u64) -> SchedulerLease {
        SchedulerLease {
            owner,
            pid,
            beat_ms,
        }
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "grokhub-lease-{name}-{}-{}",
            std::process::id(),
            crate::now_ms()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn a_beat_is_fresh_for_ninety_seconds() {
        let cabin = lease(LeaseOwner::Cabin, 7, NOW);
        assert!(lease_fresh(&cabin, NOW));
        assert!(lease_fresh(&cabin, NOW + 89_999));
        assert!(!lease_fresh(&cabin, NOW + 90_000));
        assert!(!lease_fresh(&cabin, NOW + 3_600_000));
        // A clock that moved back a little keeps the beat; a far-future stamp does not.
        assert!(lease_fresh(&cabin, NOW - 60_000));
        assert!(!lease_fresh(&cabin, NOW - 90_001));
    }

    #[test]
    fn the_shell_defers_to_a_fresh_cabin_or_another_shell() {
        let cabin = lease(LeaseOwner::Cabin, 7, NOW);
        assert!(!shell_may_fire(Some(&cabin), 9, NOW + 30_000));
        assert!(shell_may_fire(Some(&cabin), 9, NOW + 90_000));
        let other = lease(LeaseOwner::Shell, 8, NOW);
        assert!(!shell_may_fire(Some(&other), 9, NOW + 30_000));
        let mine = lease(LeaseOwner::Shell, 9, NOW);
        assert!(shell_may_fire(Some(&mine), 9, NOW + 30_000));
        assert!(shell_may_fire(None, 9, NOW));
    }

    #[test]
    fn the_lease_file_round_trips_and_a_broken_one_is_no_lease() {
        let dir = temp_dir("file");
        assert_eq!(read_lease(&dir), None);
        let shell = lease(LeaseOwner::Shell, 42, NOW);
        write_lease(&dir, &shell).expect("write");
        assert_eq!(
            std::fs::read_to_string(lease_path(&dir)).expect("raw"),
            r#"{"owner":"shell","pid":42,"beat_ms":1800000000000}"#
        );
        assert_eq!(read_lease(&dir), Some(shell));
        assert!(!dir.join("scheduler-lease.json.42.tmp").exists());
        std::fs::write(lease_path(&dir), "{not json").expect("broken");
        assert_eq!(read_lease(&dir), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_cabin_beats_at_most_every_thirty_seconds() {
        let dir = temp_dir("beat");
        let last = AtomicU64::new(0);
        assert!(beat_cabin_lease(&dir, NOW, &last));
        let first = read_lease(&dir).expect("first");
        assert_eq!(
            (first.owner, first.pid, first.beat_ms),
            (LeaseOwner::Cabin, std::process::id(), NOW)
        );
        assert!(!beat_cabin_lease(&dir, NOW + 29_999, &last));
        assert_eq!(read_lease(&dir).expect("kept").beat_ms, NOW);
        assert!(beat_cabin_lease(&dir, NOW + 30_000, &last));
        assert_eq!(read_lease(&dir).expect("next").beat_ms, NOW + 30_000);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
