//! The shell's local nudge watchers (parity with the cabin's
//! `nudge_watchers_ui.rs`). Small watchers post one Pulse suggestion when
//! something worth acting on happens: a game patch, a newer GPU driver, low
//! disk, a crash spike, pending OS updates, a new controller or printer.
//! Metadata only; nothing installs or deletes without the normal approval.
//!
//! The rules and the ledger are the cabin's (`grokhub_core::nudge_watchers`,
//! `nudge-watchers.json`), so a nudge fires once across both apps. Which
//! watchers are on lives in the cabin's `app.json` (`nudgeWatchers`); every
//! one is off until you turn it on, so this reads it and never writes it.
//! Like the scheduler, it posts only while the cabin's beat is stale.

use std::path::{Path, PathBuf};

use grokhub_core::know_me::load_tags;
use grokhub_core::nudge_watchers::{
    self as nw, CacheFolder, CrashCount, DiskSnap, GpuDriverSnap, NudgeLedger,
    NudgeWatcherSettings, OsUpdateSnap, PeripheralEvent, SteamNewsHit, TickInput, WatcherSnapshot,
    TICK_EVERY_MS,
};
use grokhub_core::pulse_sources as ps;
use grokhub_core::scheduler_lease::{read_lease, shell_may_fire};
use grokhub_core::UpdateCard;

/// A settings file is small JSON; anything bigger is not one.
const SETTINGS_CAP: u64 = 4 * 1024 * 1024;
const LEDGER_CAP: u64 = 256 * 1024;

fn ledger_path(config_dir: &Path) -> PathBuf {
    config_dir.join(nw::LEDGER_FILE)
}

pub fn load_ledger(config_dir: &Path) -> NudgeLedger {
    let path = ledger_path(config_dir);
    if std::fs::metadata(&path).map_or(true, |m| m.len() > LEDGER_CAP) {
        return NudgeLedger::default();
    }
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn save_ledger(config_dir: &Path, ledger: &NudgeLedger) -> Result<(), String> {
    std::fs::create_dir_all(config_dir).map_err(|e| e.to_string())?;
    let text = serde_json::to_string_pretty(ledger).map_err(|e| e.to_string())?;
    let tmp = config_dir.join(format!("{}.{}.tmp", nw::LEDGER_FILE, std::process::id()));
    std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, ledger_path(config_dir)).map_err(|e| e.to_string())
}

/// Which watchers are on, from the cabin's `app.json`. A missing or broken
/// file is all off.
pub fn settings(config_dir: &Path) -> NudgeWatcherSettings {
    let path = config_dir.join("app.json");
    if std::fs::metadata(&path).map_or(true, |m| m.len() > SETTINGS_CAP) {
        return NudgeWatcherSettings::default();
    }
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| v.get("nudgeWatchers").cloned())
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default()
}

fn any_on(settings: &NudgeWatcherSettings) -> bool {
    nw::WatcherId::ALL.iter().any(|id| settings.on(*id))
}

/// `YYYY-MM-DD` on the window's clock, the day string the cabin writes, so the
/// daily cap carries across both apps.
pub fn local_day(now_ms: u64, offset_min: i32) -> String {
    let minutes = (now_ms / 60_000) as i64 + i64::from(offset_min);
    let z = minutes.div_euclid(1440) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

/// A watcher's command may run this long. One that hangs is stopped, so the
/// proactive thread it runs on keeps looking.
#[cfg(target_os = "linux")]
const PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
/// The most a watcher keeps of what its command printed.
#[cfg(target_os = "linux")]
const PROBE_CAP: u64 = 4 * 1024 * 1024;

/// What `cmd` printed, or `None` when it is missing, failed or ran past
/// `timeout`. The shared bounded runner does the waiting and the stop.
#[cfg(target_os = "linux")]
fn probe(cmd: &str, args: &[&str], timeout: std::time::Duration) -> Option<String> {
    let mut command = std::process::Command::new(cmd);
    command.args(args);
    grokhub_core::proc_util::output_within(&mut command, timeout, PROBE_CAP)
}

fn dir_size_cap(path: &Path, cap_entries: usize) -> u64 {
    let mut total = 0u64;
    let Ok(read) = std::fs::read_dir(path) else {
        return 0;
    };
    for ent in read.flatten().take(cap_entries) {
        total += ent.metadata().map(|m| m.len()).unwrap_or(0);
        if ent.path().is_dir() {
            if let Ok(sub) = std::fs::read_dir(ent.path()) {
                for f in sub.flatten().take(32) {
                    total += f.metadata().map(|m| m.len()).unwrap_or(0);
                }
            }
        }
    }
    total
}

fn cache_folders(home: &Path) -> Vec<CacheFolder> {
    let candidates = [
        (".cache", home.join(".cache")),
        (".steam", home.join(".steam")),
        ("Steam", home.join(".local/share/Steam")),
    ];
    let mut out: Vec<CacheFolder> = candidates
        .into_iter()
        .filter(|(_, path)| path.is_dir())
        .map(|(name, path)| CacheFolder {
            name: name.into(),
            bytes: dir_size_cap(&path, 48),
        })
        .collect();
    out.sort_by_key(|c| std::cmp::Reverse(c.bytes));
    out.truncate(5);
    out
}

#[cfg(target_os = "linux")]
fn os_updates() -> OsUpdateSnap {
    let pending = probe("apt", &["list", "--upgradable"], PROBE_TIMEOUT)
        .map(|s| {
            s.lines()
                .skip(1)
                .filter_map(|l| l.split('/').next())
                .map(str::trim)
                .filter(|p| !p.is_empty())
                .take(12)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    OsUpdateSnap {
        pending,
        failed: Vec::new(),
    }
}

#[cfg(not(target_os = "linux"))]
fn os_updates() -> OsUpdateSnap {
    OsUpdateSnap::default()
}

#[cfg(target_os = "linux")]
fn crash_counts() -> Vec<CrashCount> {
    // The newest 500 error lines of the day: only that many are counted.
    let out = probe(
        "journalctl",
        &[
            "--since",
            "24 hours ago",
            "-p",
            "err",
            "-n",
            "500",
            "-o",
            "json",
            "-q",
            "--no-pager",
        ],
        PROBE_TIMEOUT,
    );
    let mut counts: std::collections::BTreeMap<String, u32> = std::collections::BTreeMap::new();
    for line in out.iter().flat_map(|t| t.lines().take(500)) {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let app = v
            .get("SYSLOG_IDENTIFIER")
            .or_else(|| v.get("_COMM"))
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .trim();
        if app.is_empty() || app.eq_ignore_ascii_case("grokhub") {
            continue;
        }
        let message = v.get("MESSAGE").and_then(|x| x.as_str()).unwrap_or("");
        if nw::is_log_noise(app, message) {
            continue;
        }
        *counts.entry(app.to_string()).or_insert(0) += 1;
    }
    counts
        .into_iter()
        .map(|(app, count)| CrashCount { app, count })
        .collect()
}

#[cfg(not(target_os = "linux"))]
fn crash_counts() -> Vec<CrashCount> {
    Vec::new()
}

#[cfg(target_os = "linux")]
fn peripherals() -> Vec<PeripheralEvent> {
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir("/dev/input") {
        let pad = entries
            .flatten()
            .any(|e| e.file_name().to_string_lossy().starts_with("js"));
        if pad {
            out.push(PeripheralEvent::Controller {
                name: "Game controller".into(),
                last_game: None,
            });
        }
    }
    if Path::new("/dev/usb/lp0").exists() || Path::new("/dev/usb/lp1").exists() {
        out.push(PeripheralEvent::Printer {
            name: "USB printer".into(),
        });
    }
    out
}

#[cfg(not(target_os = "linux"))]
fn peripherals() -> Vec<PeripheralEvent> {
    Vec::new()
}

/// The newest cached Steam posts (metadata only), from the Pulse source check.
fn steam_news(config_dir: &Path) -> Vec<SteamNewsHit> {
    let cache = ps::load_cache(&config_dir.join(ps::CACHE_FILE));
    let mut out = Vec::new();
    for entry in cache.entries.values() {
        for item in &entry.items {
            if !grokhub_core::steam::is_news_feed(&item.url) {
                continue;
            }
            out.push(SteamNewsHit {
                app_id: grokhub_core::steam::app_id_from_news_feed(&item.url).unwrap_or(0),
                game: item.source_name.clone(),
                headline: item.title.clone(),
                gid: format!("{}:{}", item.published_ms, item.title),
                url: item.url.clone(),
            });
        }
    }
    out.truncate(8);
    out
}

/// The driver a feed fetch offered, from `nudge-gpu-offer.json`: its version
/// and where to get it. Missing, broken or incomplete is no offer.
fn gpu_offer(config_dir: &Path) -> Option<(String, String)> {
    let path = config_dir.join("nudge-gpu-offer.json");
    if std::fs::metadata(&path).map_or(true, |m| m.len() > LEDGER_CAP) {
        return None;
    }
    let raw = std::fs::read_to_string(path).ok()?;
    let v = serde_json::from_str::<serde_json::Value>(&raw).ok()?;
    let offered = v.get("version")?.as_str()?.trim().to_string();
    let url = v.get("url")?.as_str()?.trim().to_string();
    (!offered.is_empty() && !url.is_empty()).then_some((offered, url))
}

/// The main GPU's name and installed driver, from the hardware probes
/// (nvidia-smi, vulkaninfo).
fn main_gpu(home: &Path) -> Option<(String, String)> {
    let profile = grokhub_agent::device::read_profile(home);
    let gpu = profile.main_gpu()?;
    let installed = gpu.driver.trim();
    (!installed.is_empty()).then(|| (gpu.name.clone(), installed.to_string()))
}

/// The offer is read first: without one there is no nudge to make, so the
/// hardware is not probed.
fn gpu_driver(
    config_dir: &Path,
    installed: impl FnOnce() -> Option<(String, String)>,
) -> Option<GpuDriverSnap> {
    let (offered, download_url) = gpu_offer(config_dir)?;
    let (gpu_name, installed) = installed()?;
    Some(GpuDriverSnap {
        gpu_name,
        installed,
        offered,
        download_url,
    })
}

/// What the watchers look at, read from this machine. Only the watchers that
/// are on are read, so an off one costs nothing.
pub fn snapshot(config_dir: &Path, settings: &NudgeWatcherSettings) -> WatcherSnapshot {
    let home = grokhub_core::paths::user_home().unwrap_or_default();
    let mut snap = WatcherSnapshot::default();
    if settings.steam_patch {
        snap.steam_news = steam_news(config_dir);
    }
    if settings.gpu_driver {
        snap.gpu = gpu_driver(config_dir, || main_gpu(&home));
    }
    if settings.disk_low {
        snap.disk = crate::local_ai::free_disk_mb(&home).map(|mb| DiskSnap {
            free_bytes: mb * 1024 * 1024,
            cache_folders: cache_folders(&home),
        });
    }
    if settings.crash_spike {
        snap.crashes = crash_counts();
    }
    if settings.os_updates {
        snap.os_updates = Some(os_updates());
    }
    if settings.peripheral {
        snap.peripherals = peripherals();
    }
    snap
}

/// Runs the watchers on `snap` and posts what they find. Split from
/// [`tick`] so a test can hand it a fixed snapshot. The ledger is saved first,
/// so a nudge that was claimed never fires twice even if a post fails.
pub fn tick_with(
    config_dir: &Path,
    settings: &NudgeWatcherSettings,
    snap: &WatcherSnapshot,
    today: &str,
    now_ms: u64,
) -> Result<Vec<UpdateCard>, String> {
    let mut ledger = load_ledger(config_dir);
    let know_me = load_tags(config_dir);
    let out = nw::tick(TickInput {
        settings,
        ledger: &mut ledger,
        snapshot: snap,
        know_me: &know_me,
        today,
        now_ms,
    });
    save_ledger(config_dir, &ledger)?;
    let mut posted = Vec::new();
    for card in out.cards {
        match crate::pulse::add_post(config_dir, card.clone()) {
            Ok(true) => posted.push(card),
            Ok(false) => {}
            // Cards already on the page still go back, so their looks run.
            Err(e) if posted.is_empty() => return Err(e),
            Err(e) => {
                eprintln!("nudges: {e}");
                break;
            }
        }
    }
    Ok(posted)
}

/// One look: nothing while the cabin holds the lease, nothing before the
/// ledger's 15 minutes are up, nothing when every watcher is off.
pub fn tick(
    config_dir: &Path,
    pid: u32,
    now_ms: u64,
    offset_min: i32,
) -> Result<Vec<UpdateCard>, String> {
    if !shell_may_fire(read_lease(config_dir).as_ref(), pid, now_ms) {
        return Ok(Vec::new());
    }
    let ledger = load_ledger(config_dir);
    if now_ms.saturating_sub(ledger.last_tick_ms) < TICK_EVERY_MS {
        return Ok(Vec::new());
    }
    let settings = settings(config_dir);
    let today = local_day(now_ms, offset_min);
    if !any_on(&settings) {
        let mut ledger = ledger;
        ledger.last_tick_ms = now_ms;
        save_ledger(config_dir, &ledger)?;
        return Ok(Vec::new());
    }
    let snap = snapshot(config_dir, &settings);
    tick_with(config_dir, &settings, &snap, &today, now_ms)
}

/// A nudge card was dismissed in Pulse: that watcher and key rest for a week.
pub fn dismissed(config_dir: &Path, card: &UpdateCard) {
    if let Some(meta) = &card.pulse.nudge_watcher {
        let mut ledger = load_ledger(config_dir);
        nw::dismiss_snooze(&mut ledger, meta, grokhub_core::now_ms());
        let _ = save_ledger(config_dir, &ledger);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_core::scheduler_lease::{write_lease, LeaseOwner, SchedulerLease};

    const NOW: u64 = 1_800_000_000_000;
    const MIN: u64 = 60_000;
    const PID: u32 = 4242;
    const GIB: u64 = 1024 * 1024 * 1024;

    /// A test's scratch folder, removed when the test ends, pass or fail.
    struct Scratch(PathBuf);

    impl std::ops::Deref for Scratch {
        type Target = Path;
        fn deref(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn temp_dir(name: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!(
            "grokhub-shell-nudges-{name}-{}-{}",
            std::process::id(),
            grokhub_core::now_ms()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        Scratch(dir)
    }

    fn disk_on() -> NudgeWatcherSettings {
        NudgeWatcherSettings {
            disk_low: true,
            ..NudgeWatcherSettings::default()
        }
    }

    fn low_disk() -> WatcherSnapshot {
        WatcherSnapshot {
            disk: Some(DiskSnap {
                free_bytes: 4 * GIB,
                cache_folders: vec![CacheFolder {
                    name: ".cache".into(),
                    bytes: 3 * GIB,
                }],
            }),
            ..WatcherSnapshot::default()
        }
    }

    #[test]
    fn the_watchers_you_turned_on_in_the_cabin_are_read_from_app_json() {
        let dir = temp_dir("settings");
        assert_eq!(settings(&dir), NudgeWatcherSettings::default());
        std::fs::write(
            dir.join("app.json"),
            r#"{"theme":"dark","nudgeWatchers":{"diskLow":true,"dailyMax":2}}"#,
        )
        .expect("write");
        let s = settings(&dir);
        assert!(s.disk_low && !s.gpu_driver && !s.steam_patch);
        assert_eq!(s.daily_max, 2);
        assert_eq!(s.disk_threshold_bytes, 15 * GIB);
        assert!(any_on(&s));
        std::fs::write(dir.join("app.json"), "{ nope").expect("write");
        assert!(!any_on(&settings(&dir)), "a broken file is all off");
    }

    #[test]
    fn the_day_string_matches_the_cabins_date_format() {
        // 2027-01-15 08:00 UTC.
        assert_eq!(local_day(NOW, 0), "2027-01-15");
        assert_eq!(local_day(NOW, -600), "2027-01-14");
        assert_eq!(local_day(NOW, 960 - 120), "2027-01-15");
        assert_eq!(local_day(NOW + 16 * 60 * MIN, 0), "2027-01-16");
        assert_eq!(local_day(0, 0), "1970-01-01");
        // 2028-02-29 12:00 UTC, a leap day.
        assert_eq!(local_day(1_835_438_400_000, 0), "2028-02-29");
    }

    #[test]
    fn low_disk_posts_one_nudge_and_never_a_second() {
        let dir = temp_dir("once");
        let first = tick_with(&dir, &disk_on(), &low_disk(), "2027-01-15", NOW).expect("one");
        assert_eq!(first.len(), 1);
        assert!(nw::is_nudge_watcher_card(&first[0]));
        let again =
            tick_with(&dir, &disk_on(), &low_disk(), "2027-01-15", NOW + 20 * MIN).expect("two");
        assert!(again.is_empty(), "the cooldown holds it");
        assert_eq!(crate::pulse::cards(&dir).expect("cards").len(), 1);
        let ledger = load_ledger(&dir);
        assert_eq!(ledger.posted_today, 1);
        assert_eq!(ledger.day, "2027-01-15");
    }

    #[test]
    fn a_nudge_the_cabin_posted_is_not_posted_again_here() {
        let dir = temp_dir("shared");
        // The cabin ticked first and wrote the shared ledger and the card.
        let cabin =
            tick_with(&dir, &disk_on(), &low_disk(), "2027-01-15", NOW - 20 * MIN).expect("cabin");
        assert_eq!(cabin.len(), 1);
        let shell = tick(&dir, PID, NOW, 0).expect("shell");
        assert!(shell.is_empty());
        assert_eq!(crate::pulse::cards(&dir).expect("cards").len(), 1);
    }

    #[test]
    fn the_daily_cap_holds_across_ticks() {
        let dir = temp_dir("cap");
        let settings = NudgeWatcherSettings {
            disk_low: true,
            os_updates: true,
            daily_max: 1,
            ..NudgeWatcherSettings::default()
        };
        let snap = WatcherSnapshot {
            os_updates: Some(OsUpdateSnap {
                pending: vec!["linux-image".into()],
                failed: vec!["libfoo".into()],
            }),
            ..low_disk()
        };
        let posted = tick_with(&dir, &settings, &snap, "2027-01-15", NOW).expect("tick");
        assert_eq!(posted.len(), 1, "one a day, whatever fired");
        let next_day =
            tick_with(&dir, &settings, &snap, "2027-01-16", NOW + 24 * 60 * MIN).expect("next day");
        assert_eq!(next_day.len(), 1);
    }

    #[test]
    fn the_cabin_holding_the_lease_means_no_look_at_all() {
        let dir = temp_dir("lease");
        std::fs::write(
            dir.join("app.json"),
            r#"{"nudgeWatchers":{"diskLow":true}}"#,
        )
        .expect("write");
        write_lease(
            &dir,
            &SchedulerLease {
                owner: LeaseOwner::Cabin,
                pid: 9,
                beat_ms: NOW - MIN,
            },
        )
        .expect("lease");
        assert!(tick(&dir, PID, NOW, 0).expect("tick").is_empty());
        assert_eq!(load_ledger(&dir), NudgeLedger::default(), "untouched");
    }

    #[test]
    fn all_off_looks_at_nothing_and_waits_fifteen_minutes() {
        let dir = temp_dir("off");
        assert!(tick(&dir, PID, NOW, 0).expect("tick").is_empty());
        assert_eq!(load_ledger(&dir).last_tick_ms, NOW);
        // Turned on a minute later: the ledger's 15 minutes are not up yet.
        std::fs::write(
            dir.join("app.json"),
            r#"{"nudgeWatchers":{"diskLow":true}}"#,
        )
        .expect("write");
        assert!(tick(&dir, PID, NOW + MIN, 0).expect("tick").is_empty());
        assert_eq!(load_ledger(&dir).last_tick_ms, NOW);
    }

    #[test]
    fn dismissing_a_nudge_snoozes_that_watcher_for_a_week() {
        let dir = temp_dir("dismiss");
        let card = tick_with(&dir, &disk_on(), &low_disk(), "2027-01-15", NOW)
            .expect("tick")
            .remove(0);
        let meta = card.pulse.nudge_watcher.clone().expect("meta");
        dismissed(&dir, &card);
        let after = load_ledger(&dir);
        let until = after.snoozed_until.get(&meta.dedupe_key).copied();
        assert!(
            until.is_some_and(|u| u >= nw::DISMISS_SNOOZE_MS),
            "snoozed a week from now: {until:?}"
        );
        // A card that is not a nudge changes nothing.
        let plain = grokhub_core::idea_card("idea-src", "A post", "Some detail", NOW);
        dismissed(&dir, &plain);
        assert_eq!(load_ledger(&dir), after);
        assert_eq!(meta.watcher, nw::WatcherId::DiskLow.key());
    }

    #[test]
    fn every_way_of_removing_a_nudge_in_pulse_rests_its_watcher_for_a_week() {
        use crate::pulse::Act;
        for act in [Act::Dismiss, Act::Delete, Act::Ignore] {
            let dir = temp_dir("pulse-act");
            let _config = grokhub_agent::perm::ConfigGuard::set(dir.to_path_buf());
            let card = tick_with(&dir, &disk_on(), &low_disk(), "2027-01-15", NOW)
                .expect("tick")
                .remove(0);
            let key = card.pulse.nudge_watcher.clone().expect("meta").dedupe_key;
            let day = crate::pulse::Day {
                date: "2027-01-15".into(),
                hour: 8,
                minute: 1,
            };
            let clicked = grokhub_core::now_ms();
            crate::pulse::act(&dir, &card.id, act, &day, NOW + MIN).expect("act");
            let until = load_ledger(&dir).snoozed_until.get(&key).copied();
            assert!(
                until.is_some_and(|u| u >= clicked + nw::DISMISS_SNOOZE_MS),
                "{act:?}: {until:?}"
            );
        }
    }

    #[test]
    fn without_a_driver_offer_the_gpu_is_not_probed() {
        let dir = temp_dir("gpu");
        let probed = std::cell::Cell::new(0);
        let installed = || {
            probed.set(probed.get() + 1);
            Some(("Test GPU".to_string(), "550.1".to_string()))
        };
        assert_eq!(gpu_offer(&dir), None);
        assert_eq!(gpu_driver(&dir, installed), None);
        std::fs::write(
            dir.join("nudge-gpu-offer.json"),
            r#"{"version":"551.2","url":""}"#,
        )
        .expect("write");
        assert_eq!(gpu_driver(&dir, installed), None, "an offer with no link");
        assert_eq!(probed.get(), 0, "no offer, so no hardware probe");

        std::fs::write(
            dir.join("nudge-gpu-offer.json"),
            r#"{"version":" 551.2 ","url":"https://example.com/driver"}"#,
        )
        .expect("write");
        assert_eq!(
            gpu_driver(&dir, installed),
            Some(GpuDriverSnap {
                gpu_name: "Test GPU".into(),
                installed: "550.1".into(),
                offered: "551.2".into(),
                download_url: "https://example.com/driver".into(),
            })
        );
        assert_eq!(probed.get(), 1);
        assert_eq!(gpu_driver(&dir, || None), None, "no GPU to offer it for");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_watcher_command_that_hangs_is_stopped_at_its_deadline() {
        use std::time::{Duration, Instant};
        let start = Instant::now();
        assert_eq!(probe("sleep", &["5"], Duration::from_millis(200)), None);
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "{:?}",
            start.elapsed()
        );
        assert_eq!(
            probe("echo", &["ok"], Duration::from_secs(10)).as_deref(),
            Some("ok\n")
        );
    }
}
