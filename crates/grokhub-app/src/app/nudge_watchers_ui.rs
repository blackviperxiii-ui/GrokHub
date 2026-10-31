//! Command Centre card 42: local nudge watchers in the cabin.

use grokhub_core::know_me::load_tags;
use grokhub_core::nudge_watchers::{
    self as nw, CacheFolder, CrashCount, DiskSnap, GpuDriverSnap, NudgeLedger, OsUpdateSnap,
    PeripheralEvent, SteamNewsHit, TickInput, WatcherSnapshot, TICK_EVERY_MS,
};

use super::*;
use crate::config;

pub(super) fn load_ledger() -> NudgeLedger {
    super::ledger_store::load_ledger(nw::LEDGER_FILE)
}

pub(super) fn save_ledger(ledger: &NudgeLedger) {
    super::ledger_store::save_ledger(nw::LEDGER_FILE, ledger);
}

/// The half of a watcher snapshot that walks folders and devices, read on
/// a worker thread so the frame never waits on the disk.
pub(super) struct NudgeFsSnap {
    disk: Option<DiskSnap>,
    crashes: Vec<CrashCount>,
    os_updates: OsUpdateSnap,
    peripherals: Vec<PeripheralEvent>,
}

/// Read the disk, crash, OS update and peripheral signals. `last_game`
/// names the game a found controller is offered for.
fn fs_snapshot(last_game: Option<String>) -> NudgeFsSnap {
    let home =
        grokhub_core::paths::user_home().unwrap_or_else(|| std::path::PathBuf::from("/home/user"));
    let free = super::setup_wizard::free_disk_mb(&home).map(|mb| mb * 1024 * 1024);
    let disk = free.map(|free_bytes| DiskSnap {
        free_bytes,
        cache_folders: cache_folders(&home),
    });
    NudgeFsSnap {
        disk,
        crashes: crash_counts_linux(),
        os_updates: os_updates_linux(),
        peripherals: peripheral_events(last_game),
    }
}

fn peripheral_events(last_game: Option<String>) -> Vec<PeripheralEvent> {
    #[cfg(target_os = "linux")]
    {
        let mut out = Vec::new();
        if let Ok(entries) = std::fs::read_dir("/dev/input") {
            for ent in entries.flatten() {
                let name = ent.file_name().to_string_lossy().into_owned();
                if name.starts_with("js") {
                    out.push(PeripheralEvent::Controller {
                        name: "Game controller".into(),
                        last_game,
                    });
                    break;
                }
            }
        }
        if std::path::Path::new("/dev/usb/lp0").exists()
            || std::path::Path::new("/dev/usb/lp1").exists()
        {
            out.push(PeripheralEvent::Printer {
                name: "USB printer".into(),
            });
        }
        out
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = last_game;
        Vec::new()
    }
}

fn dir_size_cap(path: &std::path::Path, cap_entries: usize) -> u64 {
    let mut total = 0u64;
    let mut n = 0usize;
    if let Ok(read) = std::fs::read_dir(path) {
        for ent in read.flatten() {
            if n >= cap_entries {
                break;
            }
            n += 1;
            let meta = ent.metadata().ok();
            total += meta.map(|m| m.len()).unwrap_or(0);
            if ent.path().is_dir() {
                if let Ok(sub) = std::fs::read_dir(ent.path()) {
                    for f in sub.flatten().take(32) {
                        total += f.metadata().map(|m| m.len()).unwrap_or(0);
                    }
                }
            }
        }
    }
    total
}

fn cache_folders(home: &std::path::Path) -> Vec<CacheFolder> {
    let candidates = [
        (".cache", home.join(".cache")),
        (".steam", home.join(".steam")),
        ("Steam", home.join(".local/share/Steam")),
    ];
    let mut out = Vec::new();
    for (name, path) in candidates {
        if path.is_dir() {
            out.push(CacheFolder {
                name: name.into(),
                bytes: dir_size_cap(&path, 48),
            });
        }
    }
    out.sort_by_key(|c| std::cmp::Reverse(c.bytes));
    out.truncate(5);
    out
}

#[cfg(target_os = "linux")]
fn os_updates_linux() -> OsUpdateSnap {
    let pending = super::setup_wizard::run("apt", &["list", "--upgradable"])
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
    let failed = super::setup_wizard::run("apt", &["list", "--upgradable", "--broken"])
        .map(|_| Vec::new())
        .unwrap_or_default();
    OsUpdateSnap { pending, failed }
}

#[cfg(not(target_os = "linux"))]
fn os_updates_linux() -> OsUpdateSnap {
    OsUpdateSnap::default()
}

fn crash_counts_linux() -> Vec<CrashCount> {
    let out = super::setup_wizard::run(
        "journalctl",
        &[
            "--since",
            "24 hours ago",
            "-p",
            "err",
            "-o",
            "json",
            "-q",
            "--no-pager",
        ],
    );
    let mut counts: std::collections::BTreeMap<String, u32> = std::collections::BTreeMap::new();
    if let Some(text) = out {
        for line in text.lines().take(500) {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
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
                if grokhub_core::nudge_watchers::is_log_noise(app, message) {
                    continue;
                }
                counts
                    .entry(app.to_string())
                    .and_modify(|c| *c += 1)
                    .or_insert(1);
            }
        }
    }
    counts
        .into_iter()
        .map(|(app, count)| CrashCount { app, count })
        .collect()
}

fn gpu_driver_snap(_cabin: &Cabin) -> Option<GpuDriverSnap> {
    let home =
        grokhub_core::paths::user_home().unwrap_or_else(|| std::path::PathBuf::from("/home/user"));
    let profile = super::local_probe::read_profile(&home);
    let gpu = profile
        .gpus
        .iter()
        .find(|g| !g.integrated)
        .or_else(|| profile.gpus.first())?;
    let installed = gpu.driver.trim();
    if installed.is_empty() {
        return None;
    }
    let cache_path = config::config_dir().join("nudge-gpu-offer.json");
    let offered = std::fs::read_to_string(&cache_path)
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| {
            let ver = v.get("version")?.as_str()?.trim().to_string();
            let url = v.get("url")?.as_str()?.trim().to_string();
            if ver.is_empty() || url.is_empty() {
                None
            } else {
                Some((ver, url))
            }
        });
    offered.map(|(offered, url)| GpuDriverSnap {
        gpu_name: gpu.name.clone(),
        installed: installed.to_string(),
        offered,
        download_url: url,
    })
}

impl Cabin {
    /// The snapshot from the worker's folder and device reads plus the
    /// Steam and GPU signals the cabin already holds.
    pub(super) fn collect_nudge_snapshot(&mut self, now: u64, fs: NudgeFsSnap) -> WatcherSnapshot {
        self.steam_refresh_library(now);
        let steam_news = self.steam_news_hits(now);
        WatcherSnapshot {
            steam_news,
            gpu: gpu_driver_snap(self),
            disk: fs.disk,
            crashes: fs.crashes,
            os_updates: Some(fs.os_updates),
            peripherals: fs.peripherals,
        }
    }

    fn steam_news_hits(&self, _now: u64) -> Vec<SteamNewsHit> {
        // News items are parsed on the Pulse source check; reuse the newest
        // cached Steam posts when present (metadata only).
        let cache =
            grokhub_core::pulse_sources::load_cache(&super::pulse_sources_ui::sources_cache_path());
        let mut out = Vec::new();
        for entry in cache.entries.values() {
            for item in &entry.items {
                if !grokhub_core::steam::is_news_feed(&item.url) {
                    continue;
                }
                let gid = format!("{}:{}", item.published_ms, item.title);
                let app_id = grokhub_core::steam::app_id_from_news_feed(&item.url).unwrap_or(0);
                out.push(SteamNewsHit {
                    app_id,
                    game: item.source_name.clone(),
                    headline: item.title.clone(),
                    gid,
                    url: item.url.clone(),
                });
            }
        }
        out.truncate(8);
        out
    }

    /// The game a found controller is offered for: the most played Steam
    /// title, when the Steam source is on.
    fn last_played_game_hint(&self) -> Option<String> {
        self.sources.steam.top_game_name()
    }

    /// Each frame: take the worker's reads when they land; otherwise, once
    /// per `TICK_EVERY_MS`, read the ledger and start the worker.
    pub(super) fn tick_nudge_watchers(&mut self) {
        let now = now_ms();
        if let Some(rx) = self.nudge_probe.take() {
            match rx.try_recv() {
                Ok(fs) => self.finish_nudge_tick(fs, now),
                Err(mpsc::TryRecvError::Empty) => self.nudge_probe = Some(rx),
                Err(mpsc::TryRecvError::Disconnected) => {}
            }
            return;
        }
        if !self.ticks.due(nw::LEDGER_FILE, now, TICK_EVERY_MS) {
            return;
        }
        let mut ledger = load_ledger();
        if now.saturating_sub(ledger.last_tick_ms) < TICK_EVERY_MS {
            return;
        }
        if !self.nudge_watchers_enabled() {
            ledger.last_tick_ms = now;
            save_ledger(&ledger);
            return;
        }
        let last_game = self.last_played_game_hint();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(fs_snapshot(last_game));
        });
        self.nudge_probe = Some(rx);
    }

    /// The worker's reads landed: run the watchers and post their cards.
    fn finish_nudge_tick(&mut self, fs: NudgeFsSnap, now: u64) {
        let mut ledger = load_ledger();
        let snap = self.collect_nudge_snapshot(now, fs);
        let know_me = load_tags(&config::config_dir());
        let today = Self::local_day();
        let out = nw::tick(TickInput {
            settings: &self.cfg.nudge_watchers,
            ledger: &mut ledger,
            snapshot: &snap,
            know_me: &know_me,
            today: &today,
            now_ms: now,
        });
        save_ledger(&ledger);
        for card in out.cards {
            self.post_feed_card(card);
        }
    }

    fn nudge_watchers_enabled(&self) -> bool {
        grokhub_core::nudge_watchers::WatcherId::ALL
            .iter()
            .any(|id| self.cfg.nudge_watchers.on(*id))
    }

    pub(super) fn nudge_watcher_dismissed(&mut self, card: &grokhub_core::UpdateCard) {
        if let Some(meta) = &card.pulse.nudge_watcher {
            let mut ledger = load_ledger();
            nw::dismiss_snooze(&mut ledger, meta, now_ms());
            save_ledger(&ledger);
        }
    }

    pub(super) fn ui_nudge_watchers(&mut self, ui: &mut egui::Ui) {
        crate::cards::section_label(ui, "Proactive nudges");
        crate::cards::help_text(
            ui,
            "Small local watchers post one Pulse suggestion when something worth acting on happens. Metadata only; nothing installs or deletes without the normal approval card.",
        );
        ui.add_space(8.0);
        let mut daily = self.cfg.nudge_watchers.daily_max;
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("Max nudges per day")
                    .size(crate::theme::FONT_BODY)
                    .color(crate::theme::fg()),
            );
            if ui
                .add(egui::DragValue::new(&mut daily).range(1..=12).speed(0.2))
                .changed()
            {
                self.cfg.nudge_watchers.daily_max = daily;
                self.persist_cfg();
            }
        });
        ui.add_space(8.0);
        for id in nw::WatcherId::ALL {
            let mut on = self.cfg.nudge_watchers.on(id);
            if ui
                .checkbox(
                    &mut on,
                    egui::RichText::new(id.label()).size(crate::theme::FONT_BODY),
                )
                .changed()
            {
                self.cfg.nudge_watchers.set_on(id, on);
                self.persist_cfg();
            }
        }
    }
}
