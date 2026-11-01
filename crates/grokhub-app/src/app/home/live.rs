//! Slow facts for Home tiles, read off the UI thread at most once a minute and
//! only while Home is painted (nothing reads them while the window is hidden or
//! another page is up).

use std::sync::Mutex;
use std::time::{Duration, Instant};

use grokhub_agent::harness::{self as hx, EgressRead};
use grokhub_agent::indexers::system_state::{self, DiskUse};

const REFRESH: Duration = Duration::from_secs(60);
const DAY_MS: u64 = 86_400_000;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct LiveFacts {
    /// Sends out of this PC in the last 24 h. `None` until the first read, or
    /// while the log is locked.
    pub sends_24h: Option<usize>,
    /// The destination most of those went to.
    pub top_dest: Option<String>,
    /// The local model is on, installed, and the governor lets it run.
    pub local_ready: bool,
    /// The main GPU's temperature in °C, when the OS reports one.
    pub gpu_temp_c: Option<i32>,
    /// Each local drive's free and total bytes.
    pub drives: Vec<DiskUse>,
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
        gpu_temp_c: None,
        drives: Vec::new(),
    },
    at: None,
    reading: false,
});

/// The last facts; starts one background read when they are a minute old.
pub(super) fn live_facts() -> LiveFacts {
    let Ok(mut slot) = SLOT.lock() else {
        return LiveFacts::default();
    };
    if !slot.reading && slot.at.is_none_or(|t| t.elapsed() >= REFRESH) {
        slot.reading = true;
        let dir = crate::config::config_dir();
        let spawned = std::thread::Builder::new()
            .name("home-live".into())
            .spawn(move || {
                let read = hx::read_egress_report(&dir);
                let mut facts = summarize_sends(&read, grokhub_core::now_ms());
                facts.local_ready = grokhub_agent::route::local::available();
                facts.gpu_temp_c = crate::app::local_probe::gpu_temp_c();
                facts.drives = system_state::local_drives(
                    &system_state::OsProbe,
                    &grokhub_agent::indexers::paths::PlatformDirs::from_env(),
                );
                if let Ok(mut slot) = SLOT.lock() {
                    slot.facts = facts;
                    slot.at = Some(Instant::now());
                    slot.reading = false;
                }
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
    let since = now_ms.saturating_sub(DAY_MS);
    let mut counts: Vec<(&str, usize)> = Vec::new();
    let mut total = 0;
    for line in read.lines.iter().filter(|l| l.ts_ms >= since) {
        total += 1;
        match counts.iter_mut().find(|(d, _)| *d == line.dest) {
            Some((_, n)) => *n += 1,
            None => counts.push((&line.dest, 1)),
        }
    }
    let top_dest = counts
        .iter()
        .max_by_key(|(_, n)| *n)
        .map(|(d, _)| (*d).to_string());
    LiveFacts {
        sends_24h: Some(total),
        top_dest,
        ..LiveFacts::default()
    }
}
