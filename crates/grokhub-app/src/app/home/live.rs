//! Slow facts for Home tiles, read off the UI thread at most once a minute and
//! only while Home is painted (nothing reads them while the window is hidden or
//! another page is up).

use eframe::egui;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use grokhub_agent::harness::{self as hx, EgressRead};
use grokhub_agent::indexers::system_state::{self, DiskUse};
use grokhub_core::home::text::SendFacts;

const REFRESH: Duration = Duration::from_secs(60);

/// The window Home last painted in, so a reader that finishes off the UI
/// thread can ask for a frame instead of waiting for the next input.
static WINDOW: Mutex<Option<egui::Context>> = Mutex::new(None);

/// Remember the window Home paints in. Cheap: a context is a handle.
pub(super) fn paint_in(ctx: &egui::Context) {
    if let Ok(mut w) = WINDOW.lock() {
        if w.as_ref() != Some(ctx) {
            *w = Some(ctx.clone());
        }
    }
}

/// Ask Home's window for a frame: a background read just landed.
pub(super) fn wake() {
    if let Some(ctx) = WINDOW.lock().ok().and_then(|w| w.clone()) {
        ctx.request_repaint();
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct LiveFacts {
    /// Sends out of this PC in the last 24 h. `None` until the first read, or
    /// while the log is locked.
    pub sends_24h: Option<usize>,
    /// The destination most of those went to.
    pub top_dest: Option<String>,
    /// The local model is on, installed, and the governor lets it run.
    pub local_ready: bool,
    /// An on-device runtime and model are on disk.
    pub local_installed: bool,
    /// The main GPU's temperature in °C, when the OS reports one.
    pub gpu_temp_c: Option<i32>,
    /// Each local drive's free and total bytes.
    pub drives: Vec<DiskUse>,
    /// The newest clips and captures, newest first (names and times only).
    pub clips: Vec<grokhub_core::clips::Clip>,
    /// The mic and speakers as the OS lists them.
    pub audio: grokhub_core::audio_check::AudioFacts,
    /// The streaming app open, by its process name.
    pub stream_app: Option<&'static str>,
    /// Steam friends saved on this PC; `None` with no Steam account here.
    pub steam_friends: Option<usize>,
    /// Steam is installed on this PC.
    pub steam_found: bool,
}

impl LiveFacts {
    /// What the Connection tile says about sends and the local model.
    pub(super) fn sends(&self) -> SendFacts {
        SendFacts {
            sends_24h: self.sends_24h,
            top_dest: self.top_dest.clone(),
            local_ready: self.local_ready,
        }
    }
}

struct Slot {
    facts: LiveFacts,
    at: Option<Instant>,
    reading: bool,
}

static SLOT: Mutex<Slot> = Mutex::new(Slot {
    facts: LiveFacts {
        sends_24h: None,
        top_dest: None,
        local_ready: false,
        local_installed: false,
        gpu_temp_c: None,
        drives: Vec::new(),
        clips: Vec::new(),
        audio: grokhub_core::audio_check::AudioFacts {
            outputs: Vec::new(),
            inputs: Vec::new(),
            devices: Vec::new(),
        },
        stream_app: None,
        steam_friends: None,
        steam_found: false,
    },
    at: None,
    reading: false,
});

/// The last facts; starts one background read when they are a minute old.
pub(super) fn live_facts() -> LiveFacts {
    let Ok(mut slot) = SLOT.lock() else {
        return LiveFacts::default();
    };
    // Tests never read this machine's drives, mic or Steam: what a test
    // paints must not leak into another test's chips.
    if cfg!(test) {
        return slot.facts.clone();
    }
    if !slot.reading && slot.at.is_none_or(|t| t.elapsed() >= REFRESH) {
        slot.reading = true;
        let dir = crate::config::config_dir();
        let spawned = std::thread::Builder::new()
            .name("home-live".into())
            .spawn(move || {
                let read = hx::read_egress_report(&dir);
                let mut facts = summarize_sends(&read, grokhub_core::now_ms());
                facts.local_ready = grokhub_agent::route::local::available();
                facts.local_installed = grokhub_agent::route::local::installed();
                facts.gpu_temp_c = crate::app::local_probe::gpu_temp_c();
                facts.drives = system_state::local_drives(
                    &system_state::OsProbe,
                    &grokhub_agent::indexers::paths::PlatformDirs::from_env(),
                );
                let steam = grokhub_core::steam::detect_root();
                facts.clips =
                    grokhub_core::clips::recent(&grokhub_core::clips::roots(steam.as_deref()), 8);
                facts.steam_friends = steam.as_deref().and_then(grokhub_core::steam::friend_count);
                facts.steam_found = steam.is_some();
                facts.audio = grokhub_core::audio_check::read_facts();
                facts.stream_app = grokhub_core::home::tiles::stream_app(
                    &grokhub_desktop::game_guard::running_names(),
                );
                if let Ok(mut slot) = SLOT.lock() {
                    slot.facts = facts;
                    slot.at = Some(Instant::now());
                    slot.reading = false;
                }
                wake();
            });
        if spawned.is_err() {
            slot.reading = false;
            slot.at = Some(Instant::now());
        }
    }
    slot.facts.clone()
}

/// Each drive's space as last read for Home. Never starts a read, so it is
/// empty until Home has painted once.
pub(in crate::app) fn cached_drives() -> Vec<DiskUse> {
    SLOT.lock()
        .map(|s| s.facts.drives.clone())
        .unwrap_or_default()
}

/// Count the last day's sends and the busiest destination.
pub(super) fn summarize_sends(read: &EgressRead, now_ms: u64) -> LiveFacts {
    if read.locked.is_some() {
        return LiveFacts::default();
    }
    let (total, top_dest) = grokhub_core::home::text::count_sends(
        read.lines.iter().map(|l| (l.ts_ms, l.dest.as_str())),
        now_ms,
    );
    LiveFacts {
        sends_24h: Some(total),
        top_dest,
        ..LiveFacts::default()
    }
}
