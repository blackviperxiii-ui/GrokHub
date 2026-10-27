//! Command Centre card 42: opt-in local watchers that post one Pulse nudge when
//! something worth acting on happens. Pure logic and fixture-driven tests;
//! the cabin collects a [`WatcherSnapshot`] and calls [`tick`].

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::know_me::{tags_mention, KnowMeTag};
use crate::pulse::PulseMeta;
use crate::update_feed::{suggestion_card, UpdateAction, UpdateCard};

pub const HOUR_MS: u64 = 60 * 60 * 1000;
pub const DAY_MS: u64 = 24 * HOUR_MS;
/// No repeat nag for the same dedupe key within 24 hours.
pub const COOLDOWN_MS: u64 = DAY_MS;
/// Dismiss snoozes the same dedupe key for 7 days.
pub const DISMISS_SNOOZE_MS: u64 = 7 * DAY_MS;
/// Default cap on new nudges per local day.
pub const DEFAULT_DAILY_MAX: u32 = 3;
/// How often the cabin should call [`tick`] at most.
pub const TICK_EVERY_MS: u64 = 15 * 60 * 1000;

pub const SOURCE_PREFIX: &str = "nudgew:";
pub const LEDGER_FILE: &str = "nudge-watchers.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WatcherId {
    SteamPatch,
    GpuDriver,
    DiskLow,
    CrashSpike,
    OsUpdates,
    Peripheral,
}

impl WatcherId {
    pub const ALL: [WatcherId; 6] = [
        Self::SteamPatch,
        Self::GpuDriver,
        Self::DiskLow,
        Self::CrashSpike,
        Self::OsUpdates,
        Self::Peripheral,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Self::SteamPatch => "steam_patch",
            Self::GpuDriver => "gpu_driver",
            Self::DiskLow => "disk_low",
            Self::CrashSpike => "crash_spike",
            Self::OsUpdates => "os_updates",
            Self::Peripheral => "peripheral",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::SteamPatch => "Game updates",
            Self::GpuDriver => "GPU driver",
            Self::DiskLow => "Disk space",
            Self::CrashSpike => "App crashes",
            Self::OsUpdates => "OS updates",
            Self::Peripheral => "Devices",
        }
    }
}

fn default_off() -> bool {
    false
}

/// Settings in `app.json`. Each watcher is opt-in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NudgeWatcherSettings {
    #[serde(default = "default_off")]
    pub steam_patch: bool,
    #[serde(default = "default_off")]
    pub gpu_driver: bool,
    #[serde(default = "default_off")]
    pub disk_low: bool,
    #[serde(default = "default_off")]
    pub crash_spike: bool,
    #[serde(default = "default_off")]
    pub os_updates: bool,
    #[serde(default = "default_off")]
    pub peripheral: bool,
    /// Max new nudges per local day across all watchers.
    #[serde(default = "default_daily_max")]
    pub daily_max: u32,
    /// Free bytes below this triggers disk_low (default 15 GiB).
    #[serde(default = "default_disk_threshold")]
    pub disk_threshold_bytes: u64,
}

fn default_daily_max() -> u32 {
    DEFAULT_DAILY_MAX
}

fn default_disk_threshold() -> u64 {
    15 * 1024 * 1024 * 1024
}

impl Default for NudgeWatcherSettings {
    fn default() -> Self {
        Self {
            steam_patch: false,
            gpu_driver: false,
            disk_low: false,
            crash_spike: false,
            os_updates: false,
            peripheral: false,
            daily_max: DEFAULT_DAILY_MAX,
            disk_threshold_bytes: default_disk_threshold(),
        }
    }
}

impl NudgeWatcherSettings {
    pub fn on(&self, id: WatcherId) -> bool {
        match id {
            WatcherId::SteamPatch => self.steam_patch,
            WatcherId::GpuDriver => self.gpu_driver,
            WatcherId::DiskLow => self.disk_low,
            WatcherId::CrashSpike => self.crash_spike,
            WatcherId::OsUpdates => self.os_updates,
            WatcherId::Peripheral => self.peripheral,
        }
    }

    pub fn set_on(&mut self, id: WatcherId, on: bool) {
        match id {
            WatcherId::SteamPatch => self.steam_patch = on,
            WatcherId::GpuDriver => self.gpu_driver = on,
            WatcherId::DiskLow => self.disk_low = on,
            WatcherId::CrashSpike => self.crash_spike = on,
            WatcherId::OsUpdates => self.os_updates = on,
            WatcherId::Peripheral => self.peripheral = on,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NudgeLedger {
    #[serde(default)]
    pub last_tick_ms: u64,
    #[serde(default)]
    pub day: String,
    #[serde(default)]
    pub posted_today: u32,
    #[serde(default)]
    pub last_posted: BTreeMap<String, u64>,
    #[serde(default)]
    pub snoozed_until: BTreeMap<String, u64>,
    #[serde(default)]
    pub steam_news_gid: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NudgeWatcherMeta {
    pub watcher: String,
    pub dedupe_key: String,
    #[serde(default)]
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatcherResult {
    pub id: WatcherId,
    pub dedupe_key: String,
    pub title: String,
    pub body: String,
    pub evidence: Vec<String>,
    pub action: Option<UpdateAction>,
    pub prompt: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SteamNewsHit {
    pub app_id: u32,
    pub game: String,
    pub headline: String,
    pub gid: String,
    pub url: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GpuDriverSnap {
    pub gpu_name: String,
    pub installed: String,
    pub offered: String,
    pub download_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheFolder {
    pub name: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiskSnap {
    pub free_bytes: u64,
    pub cache_folders: Vec<CacheFolder>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrashCount {
    pub app: String,
    pub count: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OsUpdateSnap {
    pub pending: Vec<String>,
    pub failed: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PeripheralEvent {
    Controller {
        name: String,
        last_game: Option<String>,
    },
    Printer {
        name: String,
    },
    Printer3d {
        name: String,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WatcherSnapshot {
    pub steam_news: Vec<SteamNewsHit>,
    pub gpu: Option<GpuDriverSnap>,
    pub disk: Option<DiskSnap>,
    pub crashes: Vec<CrashCount>,
    pub os_updates: Option<OsUpdateSnap>,
    pub peripherals: Vec<PeripheralEvent>,
}

pub struct TickInput<'a> {
    pub settings: &'a NudgeWatcherSettings,
    pub ledger: &'a mut NudgeLedger,
    pub snapshot: &'a WatcherSnapshot,
    pub know_me: &'a [KnowMeTag],
    pub today: &'a str,
    pub now_ms: u64,
}

pub struct TickOutput {
    pub cards: Vec<UpdateCard>,
    pub posted: u32,
}

pub fn is_nudge_watcher_card(card: &UpdateCard) -> bool {
    card.source_id.starts_with(SOURCE_PREFIX)
}

pub fn nudge_source_id(watcher: WatcherId, dedupe_key: &str) -> String {
    format!("{SOURCE_PREFIX}{}:{}", watcher.key(), dedupe_key.trim())
}

pub fn nudge_watcher_card(result: &WatcherResult, now_ms: u64) -> UpdateCard {
    let source = nudge_source_id(result.id, &result.dedupe_key);
    let mut card = suggestion_card(&source, &result.title, &result.body, now_ms);
    card.action = result.action.clone();
    card.prompt = result.prompt.clone();
    card.citations = result
        .evidence
        .iter()
        .filter(|e| e.starts_with("http://") || e.starts_with("https://"))
        .cloned()
        .collect();
    let mut why = format!("Watcher: {}", result.id.label());
    if !result.evidence.is_empty() {
        why.push('\n');
        why.push_str(&result.evidence.join("\n"));
    }
    card.why = Some(why);
    card.pulse = PulseMeta {
        because: Some(result.id.label().into()),
        nudge_watcher: Some(NudgeWatcherMeta {
            watcher: result.id.key().into(),
            dedupe_key: result.dedupe_key.clone(),
            evidence: result.evidence.clone(),
        }),
        ..Default::default()
    };
    card
}

fn ledger_day_reset(ledger: &mut NudgeLedger, today: &str) {
    if ledger.day != today {
        ledger.day = today.to_string();
        ledger.posted_today = 0;
    }
}

fn on_cooldown(ledger: &NudgeLedger, key: &str, now_ms: u64) -> bool {
    ledger
        .last_posted
        .get(key)
        .is_some_and(|t| now_ms.saturating_sub(*t) < COOLDOWN_MS)
}

fn snoozed(ledger: &NudgeLedger, key: &str, now_ms: u64) -> bool {
    ledger
        .snoozed_until
        .get(key)
        .is_some_and(|t| now_ms < *t)
}

fn may_post(ledger: &NudgeLedger, _settings: &NudgeWatcherSettings, key: &str, now_ms: u64) -> bool {
    !on_cooldown(ledger, key, now_ms) && !snoozed(ledger, key, now_ms)
}

fn record_post(ledger: &mut NudgeLedger, key: &str, now_ms: u64) {
    ledger.last_posted.insert(key.to_string(), now_ms);
    ledger.posted_today += 1;
}

/// User dismissed a nudge card: snooze its dedupe key for 7 days.
pub fn dismiss_snooze(ledger: &mut NudgeLedger, meta: &NudgeWatcherMeta, now_ms: u64) {
    ledger
        .snoozed_until
        .insert(meta.dedupe_key.clone(), now_ms + DISMISS_SNOOZE_MS);
}

pub fn detect_steam_patch(
    snap: &WatcherSnapshot,
    ledger: &mut NudgeLedger,
    know_me: &[KnowMeTag],
) -> Option<WatcherResult> {
    let gaming_ok = tags_mention(know_me, &["game", "steam", "gaming"]) || know_me.is_empty();
    if !gaming_ok {
        return None;
    }
    for hit in &snap.steam_news {
        let key = format!("steam:{}", hit.app_id);
        let prev = ledger.steam_news_gid.get(&key).cloned();
        if prev.as_deref() == Some(&hit.gid) || hit.gid.is_empty() {
            continue;
        }
        ledger.steam_news_gid.insert(key.clone(), hit.gid.clone());
        if prev.is_none() {
            // First sighting: seed gid, no nudge.
            continue;
        }
        let evidence = vec![
            format!("app_id={}", hit.app_id),
            format!("news_gid={}", hit.gid),
            format!("game={}", hit.game.trim()),
        ];
        return Some(WatcherResult {
            id: WatcherId::SteamPatch,
            dedupe_key: key,
            title: format!("{} has an update", hit.game.trim()),
            body: hit.headline.trim().to_string(),
            evidence,
            action: (!hit.url.is_empty()).then_some(UpdateAction::DeepLink {
                href: hit.url.clone(),
            }),
            prompt: None,
        });
    }
    None
}

pub fn detect_gpu_driver(snap: &WatcherSnapshot) -> Option<WatcherResult> {
    let g = snap.gpu.as_ref()?;
    if g.offered.is_empty() || g.installed.is_empty() || g.offered == g.installed {
        return None;
    }
    let key = format!("gpu:{}", g.gpu_name.trim().to_ascii_lowercase());
    let evidence = vec![
        format!("gpu={}", g.gpu_name.trim()),
        format!("installed={}", g.installed.trim()),
        format!("offered={}", g.offered.trim()),
    ];
    Some(WatcherResult {
        id: WatcherId::GpuDriver,
        dedupe_key: key,
        title: format!("GPU driver {} is available", g.offered.trim()),
        body: format!(
            "{} is on {}. Open the vendor download page when you are ready — Loadout will not install it.",
            g.gpu_name.trim(),
            g.installed.trim()
        ),
        evidence,
        action: (!g.download_url.is_empty()).then_some(UpdateAction::DeepLink {
            href: g.download_url.clone(),
        }),
        prompt: None,
    })
}

pub fn detect_disk_low(snap: &WatcherSnapshot, threshold: u64) -> Option<WatcherResult> {
    let d = snap.disk.as_ref()?;
    if d.free_bytes >= threshold {
        return None;
    }
    let free_gib = d.free_bytes / (1024 * 1024 * 1024);
    let mut evidence = vec![format!("free_bytes={}", d.free_bytes)];
    for c in &d.cache_folders {
        evidence.push(format!("cache {}={} bytes", c.name, c.bytes));
    }
    let body = if d.cache_folders.is_empty() {
        format!("About {} GiB free on this machine.", free_gib)
    } else {
        let lines: Vec<String> = d
            .cache_folders
            .iter()
            .map(|c| format!("{} — {} MiB", c.name, c.bytes / (1024 * 1024)))
            .collect();
        format!(
            "About {} GiB free. Largest cache folders:\n{}",
            free_gib,
            lines.join("\n")
        )
    };
    Some(WatcherResult {
        id: WatcherId::DiskLow,
        dedupe_key: "disk:home".into(),
        title: "Disk space is getting low".into(),
        body,
        evidence,
        action: None,
        prompt: Some("Show me what's big in my cache folders (counts and sizes only).".into()),
    })
}

pub fn detect_crash_spike(snap: &WatcherSnapshot) -> Option<WatcherResult> {
    const MIN: u32 = 3;
    let hit = snap
        .crashes
        .iter()
        .filter(|c| c.count >= MIN && !c.app.trim().is_empty())
        .max_by_key(|c| c.count)?;
    let evidence = vec![
        format!("app={}", hit.app.trim()),
        format!("crash_reports={}", hit.count),
    ];
    Some(WatcherResult {
        id: WatcherId::CrashSpike,
        dedupe_key: format!("crash:{}", hit.app.trim().to_ascii_lowercase()),
        title: format!("{} crashed several times recently", hit.app.trim()),
        body: format!(
            "{} crash reports for {} in the last day (metadata only, no dump contents).",
            hit.count,
            hit.app.trim()
        ),
        evidence,
        action: None,
        prompt: Some(format!(
            "Help me investigate why {} keeps crashing (read crash logs metadata only).",
            hit.app.trim()
        )),
    })
}

pub fn detect_os_updates(snap: &WatcherSnapshot) -> Option<WatcherResult> {
    let u = snap.os_updates.as_ref()?;
    if !u.failed.is_empty() {
        let names = u.failed.join(", ");
        return Some(WatcherResult {
            id: WatcherId::OsUpdates,
            dedupe_key: format!("os:failed:{}", names),
            title: "Some package updates failed".into(),
            body: format!("Failed packages: {}.", names),
            evidence: u
                .failed
                .iter()
                .map(|p| format!("failed={}", p))
                .collect(),
            action: None,
            prompt: Some("List failed OS package updates and what to try next (package names only).".into()),
        });
    }
    if u.pending.is_empty() {
        return None;
    }
    let names = u.pending.join(", ");
    Some(WatcherResult {
        id: WatcherId::OsUpdates,
        dedupe_key: format!("os:pending:{}", u.pending.len()),
        title: "OS updates are waiting".into(),
        body: format!("Pending packages: {}.", names),
        evidence: u
            .pending
            .iter()
            .map(|p| format!("pending={}", p))
            .collect(),
        action: None,
        prompt: Some("Summarize pending OS updates (package names only).".into()),
    })
}

pub fn detect_peripheral(snap: &WatcherSnapshot, know_me: &[KnowMeTag]) -> Option<WatcherResult> {
    for ev in &snap.peripherals {
        match ev {
            PeripheralEvent::Controller { name, last_game } => {
                let game = last_game.as_deref().filter(|g| !g.is_empty());
                let evidence = vec![
                    format!("device={}", name.trim()),
                    format!(
                        "last_game={}",
                        game.unwrap_or("(none)")
                    ),
                ];
                let (title, body, prompt) = if let Some(g) = game {
                    (
                        format!("{} connected — resume {}?", name.trim(), g),
                        format!("A controller came online. You were playing {} recently.", g),
                        Some(format!("Help me resume {}", g)),
                    )
                } else {
                    (
                        format!("{} connected", name.trim()),
                        "A game controller came online.".into(),
                        None,
                    )
                };
                return Some(WatcherResult {
                    id: WatcherId::Peripheral,
                    dedupe_key: format!("periph:ctrl:{}", name.trim().to_ascii_lowercase()),
                    title,
                    body,
                    evidence,
                    action: None,
                    prompt,
                });
            }
            PeripheralEvent::Printer { name } | PeripheralEvent::Printer3d { name } => {
                if !tags_mention(know_me, &["print", "3d", "maker"]) && !know_me.is_empty() {
                    continue;
                }
                let kind = if matches!(ev, PeripheralEvent::Printer3d { .. }) {
                    "3D printer"
                } else {
                    "Printer"
                };
                return Some(WatcherResult {
                    id: WatcherId::Peripheral,
                    dedupe_key: format!(
                        "periph:pr:{}",
                        name.trim().to_ascii_lowercase()
                    ),
                    title: format!("{} is online", name.trim()),
                    body: format!("{} came online on this machine.", kind),
                    evidence: vec![format!("device={}", name.trim())],
                    action: None,
                    prompt: None,
                });
            }
        }
    }
    None
}

fn try_watcher(
    id: WatcherId,
    settings: &NudgeWatcherSettings,
    ledger: &mut NudgeLedger,
    snap: &WatcherSnapshot,
    know_me: &[KnowMeTag],
    now_ms: u64,
) -> Option<WatcherResult> {
    if !settings.on(id) {
        return None;
    }
    let result = match id {
        WatcherId::SteamPatch => detect_steam_patch(snap, ledger, know_me),
        WatcherId::GpuDriver => detect_gpu_driver(snap),
        WatcherId::DiskLow => detect_disk_low(snap, settings.disk_threshold_bytes),
        WatcherId::CrashSpike => detect_crash_spike(snap),
        WatcherId::OsUpdates => detect_os_updates(snap),
        WatcherId::Peripheral => detect_peripheral(snap, know_me),
    }?;
    if !may_post(ledger, settings, &result.dedupe_key, now_ms) {
        return None;
    }
    Some(result)
}

/// Run enabled watchers once; returns cards to post (at most until daily cap).
pub fn tick(input: TickInput<'_>) -> TickOutput {
    let TickInput {
        settings,
        ledger,
        snapshot,
        know_me,
        today,
        now_ms,
    } = input;
    ledger_day_reset(ledger, today);
    ledger.last_tick_ms = now_ms;
    let mut cards = Vec::new();
    let mut posted = 0u32;
    if ledger.posted_today >= settings.daily_max {
        return TickOutput { cards, posted };
    }
    for id in WatcherId::ALL {
        if ledger.posted_today >= settings.daily_max {
            break;
        }
        if let Some(result) = try_watcher(id, settings, ledger, snapshot, know_me, now_ms) {
            record_post(ledger, &result.dedupe_key, now_ms);
            cards.push(nudge_watcher_card(&result, now_ms));
            posted += 1;
        }
    }
    TickOutput { cards, posted }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ledger() -> NudgeLedger {
        NudgeLedger::default()
    }

    fn settings_all_on() -> NudgeWatcherSettings {
        NudgeWatcherSettings {
            steam_patch: true,
            gpu_driver: true,
            disk_low: true,
            crash_spike: true,
            os_updates: true,
            peripheral: true,
            daily_max: DEFAULT_DAILY_MAX,
            disk_threshold_bytes: 20 * 1024 * 1024 * 1024,
        }
    }

    #[test]
    fn disabled_watcher_never_posts() {
        let mut ledger = ledger();
        let settings = NudgeWatcherSettings::default();
        let snap = WatcherSnapshot {
            gpu: Some(GpuDriverSnap {
                gpu_name: "GPU".into(),
                installed: "1.0".into(),
                offered: "2.0".into(),
                download_url: "https://example.com/driver".into(),
            }),
            ..Default::default()
        };
        let out = tick(TickInput {
            settings: &settings,
            ledger: &mut ledger,
            snapshot: &snap,
            know_me: &[],
            today: "2026-10-27",
            now_ms: 1_000_000,
        });
        assert!(out.cards.is_empty());
    }

    #[test]
    fn steam_patch_needs_second_gid() {
        let mut ledger = ledger();
        let settings = settings_all_on();
        let hit = SteamNewsHit {
            app_id: 570,
            game: "Game".into(),
            headline: "Patch notes".into(),
            gid: "100".into(),
            url: "https://store.steampowered.com/news/".into(),
        };
        let snap = WatcherSnapshot {
            steam_news: vec![hit.clone()],
            ..Default::default()
        };
        let empty = tick(TickInput {
            settings: &settings,
            ledger: &mut ledger,
            snapshot: &snap,
            know_me: &[],
            today: "d",
            now_ms: 1,
        });
        assert!(empty.cards.is_empty());
        let hit2 = SteamNewsHit {
            app_id: hit.app_id,
            game: hit.game,
            headline: hit.headline,
            gid: "101".into(),
            url: hit.url,
        };
        let snap2 = WatcherSnapshot {
            steam_news: vec![hit2],
            ..Default::default()
        };
        let out = tick(TickInput {
            settings: &settings,
            ledger: &mut ledger,
            snapshot: &snap2,
            know_me: &[],
            today: "d",
            now_ms: 2,
        });
        assert_eq!(out.posted, 1);
        assert!(out.cards[0].why.as_ref().is_some_and(|w| w.contains("news_gid=101")));
    }

    #[test]
    fn cooldown_blocks_repeat_within_24h() {
        let mut ledger = ledger();
        let settings = settings_all_on();
        let snap = WatcherSnapshot {
            crashes: vec![CrashCount {
                app: "App".into(),
                count: 5,
            }],
            ..Default::default()
        };
        let first = tick(TickInput {
            settings: &settings,
            ledger: &mut ledger,
            snapshot: &snap,
            know_me: &[],
            today: "d",
            now_ms: 0,
        });
        assert_eq!(first.posted, 1);
        let second = tick(TickInput {
            settings: &settings,
            ledger: &mut ledger,
            snapshot: &snap,
            know_me: &[],
            today: "d",
            now_ms: COOLDOWN_MS - 1,
        });
        assert!(second.cards.is_empty());
        let third = tick(TickInput {
            settings: &settings,
            ledger: &mut ledger,
            snapshot: &snap,
            know_me: &[],
            today: "d",
            now_ms: COOLDOWN_MS + 1,
        });
        assert_eq!(third.posted, 1);
    }

    #[test]
    fn daily_cap_stops_after_n() {
        let mut ledger = ledger();
        let settings = NudgeWatcherSettings {
            daily_max: 1,
            ..settings_all_on()
        };
        let snap = WatcherSnapshot {
            gpu: Some(GpuDriverSnap {
                gpu_name: "A".into(),
                installed: "1".into(),
                offered: "2".into(),
                download_url: "https://example.com/a".into(),
            }),
            crashes: vec![CrashCount {
                app: "B".into(),
                count: 9,
            }],
            ..Default::default()
        };
        let out = tick(TickInput {
            settings: &settings,
            ledger: &mut ledger,
            snapshot: &snap,
            know_me: &[],
            today: "d",
            now_ms: 0,
        });
        assert_eq!(out.posted, 1);
        assert_eq!(out.cards.len(), 1);
    }

    #[test]
    fn dismiss_snooze_holds_seven_days() {
        let mut ledger = ledger();
        let meta = NudgeWatcherMeta {
            watcher: "disk_low".into(),
            dedupe_key: "disk:home".into(),
            evidence: vec!["free_bytes=1".into()],
        };
        dismiss_snooze(&mut ledger, &meta, 100);
        assert!(snoozed(&ledger, "disk:home", 100 + DISMISS_SNOOZE_MS - 1));
        assert!(!snoozed(&ledger, "disk:home", 100 + DISMISS_SNOOZE_MS));
    }

    #[test]
    fn gpu_driver_card_links_out_never_installs_copy() {
        let snap = WatcherSnapshot {
            gpu: Some(GpuDriverSnap {
                gpu_name: "RTX".into(),
                installed: "550".into(),
                offered: "555".into(),
                download_url: "https://vendor.example/driver".into(),
            }),
            ..Default::default()
        };
        let r = detect_gpu_driver(&snap).expect("driver");
        let card = nudge_watcher_card(&r, 1);
        assert!(card.body.as_ref().is_some_and(|b| b.contains("will not install")));
        assert!(matches!(
            card.action,
            Some(UpdateAction::DeepLink { href })
                if href == "https://vendor.example/driver"
        ));
    }

    #[test]
    fn os_updates_failed_beats_pending() {
        let snap = WatcherSnapshot {
            os_updates: Some(OsUpdateSnap {
                pending: vec!["pkg-a".into()],
                failed: vec!["pkg-b".into()],
            }),
            ..Default::default()
        };
        let r = detect_os_updates(&snap).expect("failed");
        assert!(r.title.contains("failed"));
    }

    #[test]
    fn peripheral_controller_resume_prompt() {
        let snap = WatcherSnapshot {
            peripherals: vec![PeripheralEvent::Controller {
                name: "Pad".into(),
                last_game: Some("Hades".into()),
            }],
            ..Default::default()
        };
        let r = detect_peripheral(&snap, &[]).expect("ctrl");
        assert!(r.prompt.as_ref().is_some_and(|p| p.contains("Hades")));
    }
}
