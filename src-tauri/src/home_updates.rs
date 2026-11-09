//! Home's Updates & Drivers page on the shell: reads what waits to update on
//! this PC (read-only, from the lists already on disk, never a refresh) and
//! runs the page's offers. Nothing here runs on its own: every offer is a tap,
//! the system update is behind the OS's own password prompt, and nothing heavy
//! starts while a game runs. The tile reads the last page so it can say how
//! many wait without reading again.

use std::path::Path;
use std::sync::Mutex;

use grokhub_agent::elevate::{self, Elevated};
use grokhub_agent::repair::{self, Os, ProbeId, ProbeRun};
use grokhub_core::home::updates::{self as up, Found, Offer, Sources, UpdatesPage};

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
    let sources = Sources {
        version: crate::shell_update::version(),
        loadout_newer,
        loadout_updatable: updatable,
        steam: grokhub_core::steam::detect_root()
            .map(|r| grokhub_core::steam::games_needing_update(&r)),
        system: linux.then(|| system(os, &has)),
        apps: cfg!(windows).then(|| apps(os, &has)),
        windows: Some(if cfg!(windows) {
            windows_update()
        } else {
            Found::NotHere
        }),
        gpu_driver: crate::home_tiles::gpu_driver(config_dir),
        gaming: gaming(),
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

fn system(os: Os, has: &dyn Fn(&str) -> bool) -> Found {
    let Some(spec) = repair::probe_spec_with(ProbeId::PendingUpdates, os, has) else {
        return Found::NotHere;
    };
    match run(&spec) {
        Ok((code, out)) => listed(code, up::parse_linux(&spec.program, &out), &spec.program),
        Err(why) => Found::CantCheck { why },
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
        _ => None,
    }
}

/// Runs a heavy offer (system packages, winget). Blocks until it is done;
/// returns one plain sentence. Refused while a game runs.
pub fn run_offer(offer: Offer) -> Result<String, String> {
    if matches!(offer, Offer::UpdateSystem | Offer::UpdateApps) && gaming() {
        return Err("A game is running, so this waits until it closes.".into());
    }
    match offer {
        Offer::UpdateSystem => update_system(),
        Offer::UpdateApps => update_apps(),
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
        assert_eq!(offer_page(Offer::UpdateLoadout), None);
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
