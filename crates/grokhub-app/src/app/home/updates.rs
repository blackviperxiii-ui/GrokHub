//! Updates & Drivers facts for Home, read off the UI thread and only while
//! Home is painted. Steam's own manifests every 10 minutes; the OS package
//! list every 6 hours, from the lists already on disk (never a refresh), and
//! only once system checks are allowed in Settings.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use grokhub_agent::harness as hx;
use grokhub_agent::repair::{
    pending_update_count, probe_spec_with, read_only_violation, run_spec, Os, ProbeId, ProbeOutput,
    ProbeRun,
};

const STEAM_EVERY: Duration = Duration::from_secs(10 * 60);
const OS_EVERY: Duration = Duration::from_secs(6 * 60 * 60);

/// What the OS package check found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum OsUpdates {
    /// Not read yet.
    Unknown,
    /// System checks are off in Settings, so nothing ran.
    NotAllowed,
    /// No package manager this checks (Windows Update handles itself, or an unknown distro).
    NoChecker,
    Pending(usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct UpdateFacts {
    pub os: OsUpdates,
    /// Installed Steam games with an update waiting, by name.
    pub steam: Vec<String>,
}

impl Default for UpdateFacts {
    fn default() -> Self {
        Self {
            os: OsUpdates::Unknown,
            steam: Vec::new(),
        }
    }
}

struct Slot {
    facts: UpdateFacts,
    steam_at: Option<Instant>,
    os_at: Option<Instant>,
    reading: bool,
}

static SLOT: Mutex<Slot> = Mutex::new(Slot {
    facts: UpdateFacts {
        os: OsUpdates::Unknown,
        steam: Vec::new(),
    },
    steam_at: None,
    os_at: None,
    reading: false,
});

/// The last facts; starts one background read when a part is due.
pub(super) fn update_facts() -> UpdateFacts {
    let Ok(mut slot) = SLOT.lock() else {
        return UpdateFacts::default();
    };
    let steam_due = slot.steam_at.is_none_or(|t| t.elapsed() >= STEAM_EVERY);
    let os_due = slot.os_at.is_none_or(|t| t.elapsed() >= OS_EVERY);
    if !slot.reading && (steam_due || os_due) {
        slot.reading = true;
        let dir = crate::config::config_dir();
        let spawned = std::thread::Builder::new()
            .name("home-updates".into())
            .spawn(move || {
                let steam = steam_due.then(|| {
                    grokhub_core::steam::detect_root()
                        .map(|r| grokhub_core::steam::games_needing_update(&r))
                        .unwrap_or_default()
                });
                let os = os_due.then(|| {
                    let allowed = hx::ConsentLedger::load(&dir)
                        .scope_grant(&hx::Scope::SystemState)
                        .is_some();
                    if allowed {
                        os_pending()
                    } else {
                        OsUpdates::NotAllowed
                    }
                });
                if let Ok(mut slot) = SLOT.lock() {
                    let now = Instant::now();
                    if let Some(s) = steam {
                        slot.facts.steam = s;
                        slot.steam_at = Some(now);
                    }
                    if let Some(o) = os {
                        // A grant later should show soon, not in 6 hours.
                        slot.os_at = (o != OsUpdates::NotAllowed).then_some(now);
                        slot.facts.os = o;
                    }
                    slot.reading = false;
                }
            });
        if spawned.is_err() {
            slot.reading = false;
            slot.steam_at = Some(Instant::now());
            slot.os_at = Some(Instant::now());
        }
    }
    slot.facts.clone()
}

/// The repair probe's read-only package list (`apt list --upgradable`,
/// `pacman -Qu`, `dnf -C check-update`, `zypper --no-refresh list-updates`).
fn os_pending() -> OsUpdates {
    if cfg!(not(target_os = "linux")) {
        return OsUpdates::NoChecker;
    }
    let Some(spec) = probe_spec_with(ProbeId::PendingUpdates, Os::Linux, &|b| {
        grokhub_agent::repair::on_path(b)
    }) else {
        return OsUpdates::NoChecker;
    };
    if read_only_violation(&spec).is_some() || spec.needs_admin {
        return OsUpdates::NoChecker;
    }
    match run_spec(&spec) {
        ProbeRun::Done {
            code,
            stdout,
            stderr,
        } => pending_update_count(&ProbeOutput {
            program: spec.program.clone(),
            code,
            stdout,
            stderr,
        })
        .map_or(OsUpdates::Unknown, OsUpdates::Pending),
        ProbeRun::TimedOut | ProbeRun::Missing(_) => OsUpdates::Unknown,
    }
}
