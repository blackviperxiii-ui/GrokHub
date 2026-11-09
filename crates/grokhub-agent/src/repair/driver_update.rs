//! A driver update from start to finish, for the Updates page: both checks
//! from [`super::drivers`], the way back, the root script that takes the
//! snapshot and updates, the re-check after, and the way back's own script.
//! Probes run through an injected runner so the whole flow is tested without
//! a package manager.

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::drivers::{
    apt_versions, candidate_spec, driver_updated, driver_updates, installed_spec, parse_candidate,
    parse_installed, rollback_for, verify_driver, DriverBlock, DriverDoubt, DriverUpdate, Rollback,
    VerifiedDriver,
};
use super::probes::{PackageManager, ProbeSpec, ProgramName};
use super::restore::SnapshotBackend;

/// What both checks found for the driver packages the update list names.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DriverCheck {
    /// Both checks agree on these.
    pub verified: Vec<VerifiedDriver>,
    /// Listed, but the second check didn't agree: left alone.
    pub doubts: Vec<(DriverUpdate, DriverDoubt)>,
    /// The way back for `verified`, if there is one.
    pub rollback: Option<Rollback>,
}

/// Runs one read-only probe and returns its stdout, or `None` when it failed.
pub type Runner<'a> = &'a dyn Fn(&ProbeSpec) -> Option<String>;

/// Both checks for every driver package in `pending` (a `PendingUpdates`
/// probe's stdout), then the way back. Only snapper and timeshift count as a
/// snapshot here: a bare btrfs snapshot needs a layout this can't check.
pub fn check(
    pm: PackageManager,
    pending: &str,
    run: Runner<'_>,
    snapshots: &[SnapshotBackend],
    cached: &dyn Fn(&Path) -> bool,
) -> DriverCheck {
    let mut out = DriverCheck::default();
    let mut apt_tables = Vec::new();
    for u in driver_updates(pm, pending) {
        let Some(name) = ProgramName::parse(&u.pkg) else {
            out.doubts.push((u, DriverDoubt::NotADriver));
            continue;
        };
        let installed = installed_spec(pm, &name)
            .and_then(|s| run(&s))
            .and_then(|o| parse_installed(pm, &o));
        let candidate_out = candidate_spec(pm, &name).and_then(|s| run(&s));
        let candidate = candidate_out
            .as_deref()
            .and_then(|o| parse_candidate(pm, o));
        match verify_driver(&u, installed.as_deref(), candidate.as_deref()) {
            Ok(v) => {
                apt_tables.push((
                    v.pkg.clone(),
                    candidate_out.map(|o| apt_versions(&o)).unwrap_or_default(),
                ));
                out.verified.push(v);
            }
            Err(doubt) => out.doubts.push((u, doubt)),
        }
    }
    let snaps: Vec<SnapshotBackend> = snapshots
        .iter()
        .copied()
        .filter(|s| matches!(s, SnapshotBackend::Snapper | SnapshotBackend::Timeshift))
        .collect();
    let apt_old = |d: &VerifiedDriver| {
        apt_tables
            .iter()
            .any(|(pkg, versions)| *pkg == d.pkg && versions.contains(&d.from))
    };
    out.rollback = rollback_for(pm, &out.verified, &snaps, cached, &apt_old);
    out
}

/// The snapshot step for the way back, run as root before the update.
fn snapshot_cmd(backend: SnapshotBackend) -> Option<&'static str> {
    match backend {
        SnapshotBackend::Snapper => Some("snapper create --description before-driver-update"),
        SnapshotBackend::Timeshift => {
            Some("timeshift --create --scripted --comments before-driver-update")
        }
        _ => None,
    }
}

/// The root script: the snapshot first (a failed snapshot stops everything,
/// since it is the way back), then the update. `game_running` and a missing
/// way back refuse it, as the driver card does.
pub fn update_script(
    pm: PackageManager,
    c: &DriverCheck,
    game_running: bool,
) -> Result<String, DriverBlock> {
    if game_running {
        return Err(DriverBlock::GameRunning);
    }
    if c.verified.is_empty() {
        return Err(DriverBlock::Nothing);
    }
    let rollback = c.rollback.as_ref().ok_or(DriverBlock::NoWayBack)?;
    let update = super::drivers::update_cmd(pm, &c.verified).ok_or(DriverBlock::Unsupported)?;
    Ok(match rollback {
        Rollback::Snapshot(b) => {
            let snap = snapshot_cmd(*b).ok_or(DriverBlock::NoWayBack)?;
            format!("{snap}\n{update}")
        }
        Rollback::Cached(_) | Rollback::AptVersions => update,
    })
}

/// The update the page made, kept so it can be checked and put back later.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DriverRecord {
    pub drivers: Vec<VerifiedDriver>,
    pub rollback: Rollback,
}

/// The re-check after the update: every driver is at its new version.
pub fn all_updated(pm: PackageManager, drivers: &[VerifiedDriver], run: Runner<'_>) -> bool {
    drivers.iter().all(|d| {
        let now = ProgramName::parse(&d.pkg)
            .and_then(|n| installed_spec(pm, &n))
            .and_then(|s| run(&s))
            .and_then(|o| parse_installed(pm, &o));
        driver_updated(d, now.as_deref())
    })
}

/// The root script that puts the old drivers back, when the way back is the
/// old packages. A snapshot goes back with its own tool, so it has none.
pub fn rollback_script(r: &DriverRecord, game_running: bool) -> Result<String, DriverBlock> {
    if game_running {
        return Err(DriverBlock::GameRunning);
    }
    super::drivers::rollback_cmd(&r.rollback, &r.drivers).ok_or(DriverBlock::NoWayBack)
}

/// Where the record lives.
pub fn record_path(config_dir: &Path) -> std::path::PathBuf {
    config_dir.join("driver-update.json")
}

pub fn save_record(config_dir: &Path, r: &DriverRecord) -> std::io::Result<()> {
    let text = serde_json::to_string_pretty(r).map_err(std::io::Error::other)?;
    std::fs::write(record_path(config_dir), text)
}

pub fn load_record(config_dir: &Path) -> Option<DriverRecord> {
    let text = std::fs::read_to_string(record_path(config_dir)).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn clear_record(config_dir: &Path) {
    let _ = std::fs::remove_file(record_path(config_dir));
}

#[cfg(test)]
#[path = "driver_update_tests.rs"]
mod tests;
