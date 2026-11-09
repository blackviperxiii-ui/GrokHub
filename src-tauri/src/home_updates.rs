//! Home's Updates & Drivers page on the shell: reads what waits to update on
//! this PC (read-only, from the lists already on disk, never a refresh) and
//! runs the page's offers. Nothing here runs on its own: every offer is a tap,
//! the system update is behind the OS's own password prompt, and nothing heavy
//! starts while a game runs. The tile reads the last page so it can say how
//! many wait without reading again.

use std::path::Path;
use std::sync::Mutex;

use grokhub_agent::elevate::{self, Elevated};
use grokhub_agent::repair::driver_update::{self as du, DriverCheck, DriverRecord};
use grokhub_agent::repair::{
    self, DriverBlock, Os, PackageManager, ProbeId, ProbeRun, Rollback, SnapshotBackend,
    VerifiedDriver,
};
use grokhub_core::home::updates::{
    self as up, Drivers, Found, Offer, Sources, UpdateItem, UpdatesPage, WayBack,
};

/// The last read, for the tile: Steam games, system packages, apps, Windows
/// Update entries waiting.
static LAST: Mutex<Option<Sources>> = Mutex::new(None);

/// What the last page read found, if the page was opened this run.
pub fn last() -> Option<Sources> {
    LAST.lock().ok().and_then(|s| s.clone())
}

/// Reads every source this PC has. `loadout_newer` comes from the app's own
/// updater check, which the caller runs first (it is async).
pub fn read(config_dir: &Path, loadout_newer: Option<String>, updatable: bool) -> Sources {
    let os = if cfg!(windows) {
        Os::Windows
    } else {
        Os::Linux
    };
    let has = |b: &str| repair::on_path(b);
    let linux = cfg!(target_os = "linux");
    let gaming = gaming();
    let (system, drivers) = if linux {
        let (found, pending) = system(os, &has);
        (
            Some(found),
            Some(linux_drivers(config_dir, pending.as_deref(), &has)),
        )
    } else if cfg!(windows) {
        (None, Some(windows_drivers()))
    } else {
        (None, None)
    };
    let sources = Sources {
        version: crate::shell_update::version(),
        loadout_newer,
        loadout_updatable: updatable,
        steam: grokhub_core::steam::detect_root()
            .map(|r| grokhub_core::steam::games_needing_update(&r)),
        system,
        apps: cfg!(windows).then(|| apps(os, &has)),
        windows: Some(if cfg!(windows) {
            windows_update()
        } else {
            Found::NotHere
        }),
        gpu_driver: crate::home_tiles::gpu_driver(config_dir),
        gaming,
        drivers,
    };
    if let Ok(mut slot) = LAST.lock() {
        *slot = Some(sources.clone());
    }
    sources
}

/// The page for what [`read`] found.
pub fn page(sources: &Sources) -> UpdatesPage {
    up::page(sources)
}

fn run(spec: &repair::ProbeSpec) -> Result<(Option<i32>, String), String> {
    if let Some(why) = repair::read_only_violation(spec) {
        return Err(why);
    }
    match repair::run_spec(spec) {
        ProbeRun::Done { code, stdout, .. } => Ok((code, stdout)),
        ProbeRun::TimedOut => Err("it took too long to answer".into()),
        ProbeRun::Missing(_) => Err("its tool isn't installed".into()),
    }
}

/// The system list, and the probe's output when it read one (the driver
/// check reads its driver packages from the same list).
fn system(os: Os, has: &dyn Fn(&str) -> bool) -> (Found, Option<String>) {
    let Some(spec) = repair::probe_spec_with(ProbeId::PendingUpdates, os, has) else {
        return (Found::NotHere, None);
    };
    match run(&spec) {
        Ok((code, out)) => {
            let found = listed(code, up::parse_linux(&spec.program, &out), &spec.program);
            let read = matches!(found, Found::Items { .. }).then_some(out);
            (found, read)
        }
        Err(why) => (Found::CantCheck { why }, None),
    }
}

/// One read-only probe for the driver checks: its output when it exited 0.
fn probe_out(spec: &repair::ProbeSpec) -> Option<String> {
    match run(spec) {
        Ok((Some(0), out)) => Some(out),
        _ => None,
    }
}

/// Both driver checks and the way back, for `pending` (the system list).
fn driver_check(pm: PackageManager, pending: &str, has: &dyn Fn(&str) -> bool) -> DriverCheck {
    let snapshots = repair::detect_backends(Os::Linux, has);
    du::check(pm, pending, &probe_out, &snapshots, &|p: &Path| p.exists())
}

fn way_back(r: &Rollback) -> WayBack {
    match r {
        Rollback::Snapshot(b) => WayBack::Snapshot(snapshot_name(*b).into()),
        Rollback::Cached(_) | Rollback::AptVersions => WayBack::OldPackages,
    }
}

fn snapshot_name(b: SnapshotBackend) -> &'static str {
    match b {
        SnapshotBackend::Snapper => "Snapper",
        SnapshotBackend::Timeshift => "Timeshift",
        SnapshotBackend::Btrfs => "btrfs",
        SnapshotBackend::SystemRestore => "System Restore",
    }
}

fn driver_items(drivers: &[VerifiedDriver]) -> Vec<UpdateItem> {
    drivers
        .iter()
        .map(|d| UpdateItem {
            name: d.pkg.clone(),
            from: Some(d.from.clone()),
            to: Some(d.to.clone()),
        })
        .collect()
}

/// Why a checked driver update isn't offered, in the page's words.
pub fn held_why(block: DriverBlock) -> &'static str {
    match block {
        DriverBlock::NoWayBack => {
            "Not updated: there's no way back if it misbehaves (no Snapper or Timeshift snapshot, and the old version isn't kept). Setting up Timeshift or Snapper gives one."
        }
        DriverBlock::Unsupported | DriverBlock::Windows => {
            "Not updated here: this package manager's driver versions can't be checked twice yet."
        }
        DriverBlock::GameRunning => "Waits until your game closes",
        DriverBlock::Nothing => "Up to date",
    }
}

/// What the Drivers group shows, from the check and the record of the last
/// update the page made.
pub fn drivers_state(
    pm: PackageManager,
    c: &DriverCheck,
    record: Option<&DriverRecord>,
    run: du::Runner<'_>,
) -> Drivers {
    if !c.verified.is_empty() {
        let items = driver_items(&c.verified);
        return match (du::update_script(pm, c, false), &c.rollback) {
            (Ok(_), Some(r)) => Drivers::Ready {
                items,
                way_back: way_back(r),
            },
            (Err(block), _) => Drivers::Held {
                items,
                why: held_why(block).into(),
            },
            (Ok(_), None) => Drivers::Held {
                items,
                why: held_why(DriverBlock::NoWayBack).into(),
            },
        };
    }
    if let Some(r) = record {
        if du::all_updated(pm, &r.drivers, run) {
            return Drivers::Updated {
                items: driver_items(&r.drivers),
                way_back: way_back(&r.rollback),
            };
        }
    }
    if c.doubts.is_empty() {
        return Drivers::UpToDate;
    }
    Drivers::Held {
        items: c
            .doubts
            .iter()
            .map(|(u, _)| UpdateItem {
                name: u.pkg.clone(),
                from: u.from.clone(),
                to: Some(u.to.clone()),
            })
            .collect(),
        why: "Listed, but the second check didn't match, so it's left alone.".into(),
    }
}

fn linux_drivers(config_dir: &Path, pending: Option<&str>, has: &dyn Fn(&str) -> bool) -> Drivers {
    let Some(pm) = PackageManager::detect(has) else {
        return Drivers::UpToDate;
    };
    let Some(pending) = pending else {
        return Drivers::Held {
            items: Vec::new(),
            why: "Couldn't check: the system list couldn't be read".into(),
        };
    };
    let c = driver_check(pm, pending, has);
    let record = du::load_record(config_dir);
    // The way back is kept only while the new drivers are installed.
    if let Some(r) = &record {
        if c.verified.is_empty() && !du::all_updated(pm, &r.drivers, &probe_out) {
            du::clear_record(config_dir);
            return drivers_state(pm, &c, None, &probe_out);
        }
    }
    drivers_state(pm, &c, record.as_ref(), &probe_out)
}

fn windows_drivers() -> Drivers {
    match run(&repair::windows_driver_titles_spec()) {
        Ok((Some(0), out)) => Drivers::Windows {
            items: up::parse_windows_update(&out),
        },
        Ok(_) | Err(_) => Drivers::Held {
            items: Vec::new(),
            why: "Couldn't check: Windows Update didn't answer".into(),
        },
    }
}

/// A list counts only when its tool says it read one: an error exit with
/// nothing listed is "couldn't check", never "up to date". dnf exits 100 when
/// updates wait; pacman -Qu exits 1 when nothing does.
pub fn listed(code: Option<i32>, items: Vec<up::UpdateItem>, program: &str) -> Found {
    let ok = match (program, code) {
        (_, Some(0)) => true,
        ("dnf", Some(100)) | ("pacman", Some(1)) => true,
        _ => !items.is_empty(),
    };
    if ok {
        Found::items(items)
    } else {
        Found::CantCheck {
            why: "its list couldn't be read".into(),
        }
    }
}

fn apps(os: Os, has: &dyn Fn(&str) -> bool) -> Found {
    if !has("winget") {
        return Found::NotHere;
    }
    let Some(spec) = repair::probe_spec_with(ProbeId::WingetUpgrades, os, has) else {
        return Found::NotHere;
    };
    match run(&spec) {
        Ok((code, out)) => listed(code, up::parse_winget(&out), "winget"),
        Err(why) => Found::CantCheck { why },
    }
}

fn windows_update() -> Found {
    match run(&repair::windows_update_titles_spec()) {
        Ok((Some(0), out)) => Found::items(up::parse_windows_update(&out)),
        Ok(_) => Found::CantCheck {
            why: "Windows Update didn't answer".into(),
        },
        Err(why) => Found::CantCheck { why },
    }
}

#[cfg(desktop)]
fn gaming() -> bool {
    let g = grokhub_desktop::game_guard::peek();
    g.gaming || g.anticheat.is_some()
}

#[cfg(mobile)]
fn gaming() -> bool {
    false
}

/// The fixed page an offer opens, if it opens one.
pub fn offer_page(offer: Offer) -> Option<&'static str> {
    match offer {
        Offer::OpenSteamDownloads => Some("steam://open/downloads"),
        Offer::OpenWindowsUpdate => Some("ms-settings:windowsupdate"),
        Offer::OpenOptionalUpdates => Some("ms-settings:windowsupdate-optionalupdates"),
        _ => None,
    }
}

/// Runs a heavy offer (system packages, winget). Blocks until it is done;
/// returns one plain sentence. Refused while a game runs.
pub fn run_offer(offer: Offer, config_dir: &Path) -> Result<String, String> {
    let heavy = matches!(
        offer,
        Offer::UpdateSystem | Offer::UpdateApps | Offer::UpdateDrivers | Offer::RollBackDrivers
    );
    if heavy && gaming() {
        return Err("A game is running, so this waits until it closes.".into());
    }
    match offer {
        Offer::UpdateSystem => update_system(),
        Offer::UpdateApps => update_apps(),
        Offer::UpdateDrivers => update_drivers(config_dir),
        Offer::RollBackDrivers => roll_back_drivers(config_dir),
        _ => Err("That one opens its own page.".into()),
    }
}

fn update_system() -> Result<String, String> {
    if !cfg!(target_os = "linux") {
        return Err("System packages update this way on Linux only.".into());
    }
    let spec = repair::probe_spec_with(ProbeId::PendingUpdates, Os::Linux, &|b| repair::on_path(b))
        .ok_or("No package manager this knows is on this PC.")?;
    let script = up::system_upgrade_script(&spec.program)
        .ok_or("No package manager this knows is on this PC.")?;
    system_sentence(elevate::root_sh(&script))
}

/// Checks the drivers twice again (the list may have changed since the page
/// was read), takes the snapshot if that is the way back, updates to the
/// exact versions, then checks the new versions are installed before
/// keeping the record the way back uses.
fn update_drivers(config_dir: &Path) -> Result<String, String> {
    if !cfg!(target_os = "linux") {
        return Err("On Windows, driver updates install from Windows Update's own page.".into());
    }
    let has = |b: &str| repair::on_path(b);
    let pm = PackageManager::detect(&has).ok_or("No package manager this knows is on this PC.")?;
    let (_, pending) = system(Os::Linux, &has);
    let pending = pending.ok_or("The update list couldn't be read, so nothing changed.")?;
    let c = driver_check(pm, &pending, &has);
    let script = du::update_script(pm, &c, gaming()).map_err(|b| held_why(b).to_string())?;
    let record = DriverRecord {
        drivers: c.verified.clone(),
        rollback: c.rollback.clone().ok_or(held_why(DriverBlock::NoWayBack))?,
    };
    match elevate::root_sh(&script) {
        Elevated::Done => {}
        other => return not_run(other, "The driver update"),
    }
    driver_landed(du::all_updated(pm, &record.drivers, &probe_out), || {
        du::save_record(config_dir, &record)
    })
}

/// The result after the update ran: the record is kept only when every
/// driver is at its new version.
pub fn driver_landed(
    landed: bool,
    save: impl FnOnce() -> std::io::Result<()>,
) -> Result<String, String> {
    if !landed {
        return Err(
            "The update ran, but the new version isn't installed. Nothing else changed.".into(),
        );
    }
    match save() {
        Ok(()) => Ok("Driver updated. Restart to load it.".into()),
        Err(_) => Ok(
            "Driver updated. Restart to load it. The way back couldn't be saved, so going back is by hand."
                .into(),
        ),
    }
}

/// Puts the old drivers back from the record, then checks they are.
fn roll_back_drivers(config_dir: &Path) -> Result<String, String> {
    let has = |b: &str| repair::on_path(b);
    let pm = PackageManager::detect(&has).ok_or("No package manager this knows is on this PC.")?;
    let record = du::load_record(config_dir).ok_or("There's no driver update here to undo.")?;
    let script = du::rollback_script(&record, gaming()).map_err(|b| match b {
        DriverBlock::NoWayBack => {
            "This update goes back by restoring its snapshot, in its own app.".to_string()
        }
        other => held_why(other).to_string(),
    })?;
    match elevate::root_sh(&script) {
        Elevated::Done => {}
        other => return not_run(other, "Putting the old driver back"),
    }
    let old: Vec<VerifiedDriver> = record
        .drivers
        .iter()
        .map(|d| VerifiedDriver {
            pkg: d.pkg.clone(),
            from: d.to.clone(),
            to: d.from.clone(),
        })
        .collect();
    if du::all_updated(pm, &old, &probe_out) {
        du::clear_record(config_dir);
        Ok("The old driver is back. Restart to load it.".into())
    } else {
        Err("Putting it back ran, but the old version isn't installed.".into())
    }
}

/// A driver step whose prompt or run didn't finish, in plain words.
pub fn not_run(ran: Elevated, what: &str) -> Result<String, String> {
    match ran {
        Elevated::Done => Ok(String::new()),
        Elevated::Dismissed => Ok("The password prompt was closed, so nothing changed.".into()),
        Elevated::NoPrompt => {
            Err("This desktop can't show a password prompt, so nothing changed.".into())
        }
        Elevated::Failed(why) => Err(format!("{what} stopped: {why}")),
    }
}

/// What the system update's prompt and run came to, in plain words.
pub fn system_sentence(ran: Elevated) -> Result<String, String> {
    match ran {
        Elevated::Done => Ok("System updated. A restart may be needed for some of it.".into()),
        Elevated::Dismissed => Ok("The password prompt was closed, so nothing changed.".into()),
        Elevated::NoPrompt => Err(
            "This desktop can't show a password prompt. Your system's Software app can update it."
                .into(),
        ),
        Elevated::Failed(why) => Err(format!("The update stopped: {why}")),
    }
}

#[cfg(windows)]
fn update_apps() -> Result<String, String> {
    // A visible window: winget asks for agreements or admin itself.
    std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-Command", up::WINGET_UPGRADE_ALL])
        .spawn()
        .map(|_| "winget is updating your apps in its own window.".to_string())
        .map_err(|e| format!("winget didn't start ({e})"))
}

#[cfg(not(windows))]
fn update_apps() -> Result<String, String> {
    Err("Apps update this way on Windows only.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offers_open_only_their_own_fixed_pages() {
        assert_eq!(
            offer_page(Offer::OpenSteamDownloads),
            Some("steam://open/downloads")
        );
        assert_eq!(
            offer_page(Offer::OpenWindowsUpdate),
            Some("ms-settings:windowsupdate")
        );
        assert_eq!(offer_page(Offer::UpdateSystem), None);
        assert_eq!(offer_page(Offer::UpdateThisApp), None);
    }

    #[test]
    fn an_error_exit_with_nothing_listed_is_never_up_to_date() {
        let one = vec![up::UpdateItem::named("vim")];
        assert_eq!(listed(Some(0), vec![], "apt"), Found::items(vec![]));
        assert_eq!(listed(Some(1), vec![], "pacman"), Found::items(vec![]));
        assert_eq!(
            listed(Some(100), one.clone(), "dnf"),
            Found::items(one.clone())
        );
        assert_eq!(
            listed(Some(100), vec![], "apt"),
            Found::CantCheck {
                why: "its list couldn't be read".into()
            }
        );
        assert_eq!(
            listed(None, vec![], "winget"),
            Found::CantCheck {
                why: "its list couldn't be read".into()
            }
        );
    }

    fn nvidia() -> VerifiedDriver {
        VerifiedDriver {
            pkg: "nvidia-utils".into(),
            from: "560.35.03-1".into(),
            to: "565.57.01-1".into(),
        }
    }

    #[test]
    fn checked_drivers_with_a_way_back_are_ready_and_without_one_are_held() {
        let none = |_: &repair::ProbeSpec| -> Option<String> { None };
        let mut c = DriverCheck {
            verified: vec![nvidia()],
            doubts: vec![],
            rollback: Some(Rollback::Snapshot(SnapshotBackend::Timeshift)),
        };
        assert_eq!(
            drivers_state(PackageManager::Pacman, &c, None, &none),
            Drivers::Ready {
                items: vec![UpdateItem {
                    name: "nvidia-utils".into(),
                    from: Some("560.35.03-1".into()),
                    to: Some("565.57.01-1".into()),
                }],
                way_back: WayBack::Snapshot("Timeshift".into()),
            }
        );
        c.rollback = None;
        let Drivers::Held { why, .. } = drivers_state(PackageManager::Pacman, &c, None, &none)
        else {
            panic!("no way back must hold the update");
        };
        assert!(why.starts_with("Not updated: there's no way back"));
    }

    #[test]
    fn the_last_update_shows_while_its_drivers_are_installed() {
        let c = DriverCheck::default();
        let record = DriverRecord {
            drivers: vec![nvidia()],
            rollback: Rollback::AptVersions,
        };
        let new = |_: &repair::ProbeSpec| Some("nvidia-utils 565.57.01-1\n".to_string());
        assert!(matches!(
            drivers_state(PackageManager::Pacman, &c, Some(&record), &new),
            Drivers::Updated {
                way_back: WayBack::OldPackages,
                ..
            }
        ));
        let old = |_: &repair::ProbeSpec| Some("nvidia-utils 560.35.03-1\n".to_string());
        assert_eq!(
            drivers_state(PackageManager::Pacman, &c, Some(&record), &old),
            Drivers::UpToDate
        );
    }

    #[test]
    fn the_way_back_is_kept_only_when_the_new_driver_landed() {
        let mut saved = false;
        assert!(driver_landed(false, || {
            saved = true;
            Ok(())
        })
        .is_err());
        assert!(!saved);
        assert_eq!(
            driver_landed(true, || Ok(())),
            Ok("Driver updated. Restart to load it.".into())
        );
        assert_eq!(
            not_run(Elevated::Dismissed, "The driver update"),
            Ok("The password prompt was closed, so nothing changed.".into())
        );
        assert_eq!(
            not_run(
                Elevated::Failed("snapper failed".into()),
                "The driver update"
            ),
            Err("The driver update stopped: snapper failed".into())
        );
        assert_eq!(
            offer_page(Offer::OpenOptionalUpdates),
            Some("ms-settings:windowsupdate-optionalupdates")
        );
    }

    #[test]
    fn the_system_update_reports_each_prompt_answer_plainly() {
        assert_eq!(
            system_sentence(Elevated::Dismissed),
            Ok("The password prompt was closed, so nothing changed.".into())
        );
        assert_eq!(
            system_sentence(Elevated::Failed("dpkg was interrupted".into())),
            Err("The update stopped: dpkg was interrupted".into())
        );
        assert!(system_sentence(Elevated::Done)
            .unwrap()
            .starts_with("System updated."));
    }
}
