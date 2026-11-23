//! Rolls a self-update of the AppImage back when the new version can't stay
//! up. Before the updater replaces the AppImage, [`prepare`] keeps a copy of
//! the running one beside it and writes `update-pending.json`. Each start of
//! the new version counts a launch ([`on_launch`]) until it has run for
//! [`HEALTHY_AFTER`] ([`mark_healthy`]). When two starts in a row never got
//! that far, the next start puts the old AppImage back and runs it.
//!
//! Windows installs through its own installer, which keeps no copy to go
//! back to, so this covers the AppImage only.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// The record of an update that hasn't proved itself yet.
pub const PENDING_FILE: &str = "update-pending.json";
/// How long a start must stay up to count as working.
pub const HEALTHY_AFTER: Duration = Duration::from_secs(90);
/// Starts of the new version that may end early before it is rolled back.
pub const FAILED_STARTS: u32 = 2;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pending {
    /// The version that was running when the update was installed.
    pub from: String,
    /// The version the update installs.
    pub to: String,
    /// The AppImage the updater replaces.
    pub target: PathBuf,
    /// The copy of the old AppImage.
    pub backup: PathBuf,
    /// Starts of `to` that haven't reached [`HEALTHY_AFTER`] yet.
    pub launches: u32,
}

/// What a start does with the pending record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// No update is waiting to prove itself.
    Nothing,
    /// The record no longer applies (the old version is running, or it names
    /// another version): drop it and its copy.
    Drop,
    /// The new version started again: save the higher count.
    Count(Pending),
    /// The new version failed too often: put the old AppImage back.
    Restore(Pending),
}

/// The decision for a start of `running`.
pub fn step(pending: Option<Pending>, running: &str) -> Step {
    let Some(mut p) = pending else {
        return Step::Nothing;
    };
    if p.to != running {
        return Step::Drop;
    }
    if p.launches >= FAILED_STARTS {
        return Step::Restore(p);
    }
    p.launches += 1;
    Step::Count(p)
}

fn pending_path(config_dir: &Path) -> PathBuf {
    config_dir.join(PENDING_FILE)
}

fn load(config_dir: &Path) -> Option<Pending> {
    let text = std::fs::read_to_string(pending_path(config_dir)).ok()?;
    serde_json::from_str(&text).ok()
}

fn save(config_dir: &Path, p: &Pending) -> Result<(), String> {
    let text = serde_json::to_string_pretty(p).map_err(|e| e.to_string())?;
    grokhub_core::pulse_writer::write_atomic(&pending_path(config_dir), &text)
}

fn forget(config_dir: &Path, p: Option<&Pending>) {
    if let Some(p) = p {
        let _ = std::fs::remove_file(&p.backup);
    }
    let _ = std::fs::remove_file(pending_path(config_dir));
}

/// The copy's path, beside the AppImage so restoring it is a rename.
pub fn backup_path(target: &Path) -> PathBuf {
    let mut name = target.file_name().unwrap_or_default().to_os_string();
    name.push(".previous");
    target.with_file_name(name)
}

/// Before an update replaces `target`: copy it and record the update.
pub fn prepare(config_dir: &Path, target: &Path, from: &str, to: &str) -> Result<(), String> {
    let backup = backup_path(target);
    let _ = std::fs::remove_file(config_dir.join(ROLLED_BACK_FILE));
    std::fs::copy(target, &backup).map_err(|e| format!("couldn't keep the old version: {e}"))?;
    save(
        config_dir,
        &Pending {
            from: from.to_string(),
            to: to.to_string(),
            target: target.to_path_buf(),
            backup,
            launches: 0,
        },
    )
}

/// An update that didn't install: the old version keeps running, so drop
/// the record and the copy.
pub fn abandon(config_dir: &Path) {
    forget(config_dir, load(config_dir).as_ref());
}

/// At start, before anything else. `Some(path)` when the old AppImage was
/// put back: run it instead of this one.
pub fn on_launch(config_dir: &Path, running: &str) -> Option<PathBuf> {
    let pending = load(config_dir);
    match step(pending.clone(), running) {
        Step::Nothing => None,
        Step::Drop => {
            forget(config_dir, pending.as_ref());
            None
        }
        Step::Count(p) => {
            let _ = save(config_dir, &p);
            None
        }
        Step::Restore(p) => restore(config_dir, &p),
    }
}

fn restore(config_dir: &Path, p: &Pending) -> Option<PathBuf> {
    // Without the copy there is nothing to go back to; stop counting.
    let restored = p.backup.is_file() && std::fs::rename(&p.backup, &p.target).is_ok();
    let _ = std::fs::remove_file(pending_path(config_dir));
    if !restored {
        return None;
    }
    let note = format!(
        "{} didn't stay running after the update, so {} was put back.\n",
        p.to, p.from
    );
    let _ = std::fs::write(config_dir.join(ROLLED_BACK_FILE), note);
    Some(p.target.clone())
}

/// What the last rollback said, for the Updates panel. The next update
/// clears it.
pub const ROLLED_BACK_FILE: &str = "update-rolled-back.txt";

/// The last rollback's note, if one happened since the last update.
pub fn rolled_back_note(config_dir: &Path) -> Option<String> {
    std::fs::read_to_string(config_dir.join(ROLLED_BACK_FILE))
        .ok()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
}

/// The new version has stayed up: it keeps the update.
pub fn mark_healthy(config_dir: &Path, running: &str) {
    if let Some(p) = load(config_dir).filter(|p| p.to == running) {
        forget(config_dir, Some(&p));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pending(launches: u32) -> Pending {
        Pending {
            from: "3.5.0-beta.1116".into(),
            to: "3.5.0-beta.1117".into(),
            target: PathBuf::from("/home/user/.local/bin/grokhub-shell"),
            backup: PathBuf::from("/home/user/.local/bin/grokhub-shell.previous"),
            launches,
        }
    }

    #[test]
    fn two_short_starts_of_the_new_version_roll_it_back_on_the_third() {
        assert_eq!(step(None, "3.5.0-beta.1117"), Step::Nothing);
        assert_eq!(
            step(Some(pending(0)), "3.5.0-beta.1117"),
            Step::Count(pending(1))
        );
        assert_eq!(
            step(Some(pending(1)), "3.5.0-beta.1117"),
            Step::Count(pending(2))
        );
        assert_eq!(
            step(Some(pending(2)), "3.5.0-beta.1117"),
            Step::Restore(pending(2))
        );
        // The old version running again means the update never took.
        assert_eq!(step(Some(pending(1)), "3.5.0-beta.1116"), Step::Drop);
    }

    #[test]
    fn the_backup_sits_beside_the_appimage() {
        assert_eq!(
            backup_path(Path::new("/home/user/Apps/App.AppImage")),
            PathBuf::from("/home/user/Apps/App.AppImage.previous")
        );
    }

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("grokhub-rollback-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        dir
    }

    #[test]
    fn a_new_version_that_keeps_ending_early_gets_the_old_one_back() {
        let dir = scratch("restore");
        let app = dir.join("bin/grokhub-shell");
        std::fs::write(&app, "old build").unwrap();
        prepare(&dir, &app, "3.5.0-beta.1116", "3.5.0-beta.1117").unwrap();
        // The updater writes the new AppImage over the old one.
        std::fs::write(&app, "new build").unwrap();
        assert_eq!(on_launch(&dir, "3.5.0-beta.1117"), None);
        assert_eq!(on_launch(&dir, "3.5.0-beta.1117"), None);
        assert_eq!(load(&dir).map(|p| p.launches), Some(2));
        assert_eq!(on_launch(&dir, "3.5.0-beta.1117"), Some(app.clone()));
        assert_eq!(std::fs::read_to_string(&app).unwrap(), "old build");
        assert!(!backup_path(&app).exists());
        assert!(!dir.join(PENDING_FILE).exists());
        assert_eq!(
            rolled_back_note(&dir).as_deref(),
            Some("3.5.0-beta.1117 didn't stay running after the update, so 3.5.0-beta.1116 was put back.")
        );
        // The next update attempt clears the note.
        prepare(&dir, &app, "3.5.0-beta.1116", "3.5.0-beta.1118").unwrap();
        assert_eq!(rolled_back_note(&dir), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_new_version_that_stays_up_keeps_the_update() {
        let dir = scratch("healthy");
        let app = dir.join("bin/grokhub-shell");
        std::fs::write(&app, "old build").unwrap();
        prepare(&dir, &app, "3.5.0-beta.1116", "3.5.0-beta.1117").unwrap();
        std::fs::write(&app, "new build").unwrap();
        assert_eq!(on_launch(&dir, "3.5.0-beta.1117"), None);
        // A healthy mark from another version changes nothing.
        mark_healthy(&dir, "3.5.0-beta.1116");
        assert!(dir.join(PENDING_FILE).exists());
        mark_healthy(&dir, "3.5.0-beta.1117");
        assert!(!dir.join(PENDING_FILE).exists());
        assert!(!backup_path(&app).exists());
        assert_eq!(on_launch(&dir, "3.5.0-beta.1117"), None);
        assert_eq!(std::fs::read_to_string(&app).unwrap(), "new build");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_update_that_did_not_install_drops_its_copy() {
        let dir = scratch("abandon");
        let app = dir.join("bin/grokhub-shell");
        std::fs::write(&app, "old build").unwrap();
        prepare(&dir, &app, "3.5.0-beta.1116", "3.5.0-beta.1117").unwrap();
        assert_eq!(
            std::fs::read_to_string(backup_path(&app)).unwrap(),
            "old build"
        );
        abandon(&dir);
        assert!(!backup_path(&app).exists());
        assert!(!dir.join(PENDING_FILE).exists());
        // The old version starting with a stale record drops it too.
        prepare(&dir, &app, "3.5.0-beta.1116", "3.5.0-beta.1117").unwrap();
        assert_eq!(on_launch(&dir, "3.5.0-beta.1116"), None);
        assert!(!backup_path(&app).exists());
        assert!(!dir.join(PENDING_FILE).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
