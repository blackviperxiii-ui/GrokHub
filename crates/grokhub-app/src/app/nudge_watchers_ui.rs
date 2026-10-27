//! Command Centre card 42: local nudge watchers in the cabin.

use grokhub_core::know_me::load_tags;
use grokhub_core::nudge_watchers::{
    self as nw, CacheFolder, CrashCount, DiskSnap, GpuDriverSnap, NudgeLedger, OsUpdateSnap,
    PeripheralEvent, SteamNewsHit, TickInput, WatcherSnapshot, TICK_EVERY_MS,
};

use super::*;
use crate::config;

fn ledger_path() -> std::path::PathBuf {
    config::config_dir().join(nw::LEDGER_FILE)
}

pub(super) fn load_ledger() -> NudgeLedger {
    std::fs::read_to_string(ledger_path())
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

pub(super) fn save_ledger(ledger: &NudgeLedger) {
    if let Ok(text) = serde_json::to_string_pretty(ledger) {
        let path = ledger_path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(path, text);
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
    pub(super) fn collect_nudge_snapshot(&mut self, now: u64) -> WatcherSnapshot {
        let home = grokhub_core::paths::user_home()
            .unwrap_or_else(|| std::path::PathBuf::from("/home/user"));
        let free = super::setup_wizard::free_disk_mb(&home).map(|mb| mb * 1024 * 1024);
        let disk = free.map(|free_bytes| DiskSnap {
            free_bytes,
            cache_folders: cache_folders(&home),
        });
        self.steam_refresh_library(now);
        let steam_news = self.steam_news_hits(now);
        WatcherSnapshot {
            steam_news,
            gpu: gpu_driver_snap(self),
            disk,
            crashes: crash_counts_linux(),
            os_updates: Some(os_updates_linux()),
            peripherals: self.peripheral_events(),
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

    fn peripheral_events(&self) -> Vec<PeripheralEvent> {
        let mut out = Vec::new();
        #[cfg(target_os = "linux")]
        {
            if let Ok(entries) = std::fs::read_dir("/dev/input") {
                for ent in entries.flatten() {
                    let name = ent.file_name().to_string_lossy().into_owned();
                    if name.starts_with("js") {
                        out.push(PeripheralEvent::Controller {
                            name: "Game controller".into(),
                            last_game: self.last_played_game_hint(),
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
        }
        out
    }

    #[cfg(target_os = "linux")]
    fn last_played_game_hint(&self) -> Option<String> {
        self.sources.steam.top_game_name()
    }

    pub(super) fn tick_nudge_watchers(&mut self) {
        let now = now_ms();
        let mut ledger = load_ledger();
        if now.saturating_sub(ledger.last_tick_ms) < TICK_EVERY_MS {
            return;
        }
        if !self.nudge_watchers_enabled() {
            ledger.last_tick_ms = now;
            save_ledger(&ledger);
            return;
        }
        let snap = self.collect_nudge_snapshot(now);
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
