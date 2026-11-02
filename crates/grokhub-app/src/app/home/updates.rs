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
/// A "not allowed" answer is asked again this soon, so a grant in Settings
/// shows within a minute without a new read on every repaint.
const NOT_ALLOWED_EVERY: Duration = Duration::from_secs(60);

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

/// Which parts (Steam, OS) are due a read at `now`.
fn due(slot: &Slot, now: Instant) -> (bool, bool) {
    let past = |at: Option<Instant>, every: Duration| {
        at.is_none_or(|t| now.saturating_duration_since(t) >= every)
    };
    let os_every = if slot.facts.os == OsUpdates::NotAllowed {
        NOT_ALLOWED_EVERY
    } else {
        OS_EVERY
    };
    (past(slot.steam_at, STEAM_EVERY), past(slot.os_at, os_every))
}

/// Puts a finished background read in the slot.
fn record(slot: &mut Slot, steam: Option<Vec<String>>, os: Option<OsUpdates>, now: Instant) {
    if let Some(s) = steam {
        slot.facts.steam = s;
        slot.steam_at = Some(now);
    }
    if let Some(o) = os {
        slot.facts.os = o;
        slot.os_at = Some(now);
    }
    slot.reading = false;
}

/// The last facts; starts one background read when a part is due.
pub(super) fn update_facts() -> UpdateFacts {
    let Ok(mut slot) = SLOT.lock() else {
        return UpdateFacts::default();
    };
    let (steam_due, os_due) = due(&slot, Instant::now());
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
                    record(&mut slot, steam, os, Instant::now());
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

#[cfg(test)]
mod tests {
    use super::*;

    fn empty() -> Slot {
        Slot {
            facts: UpdateFacts::default(),
            steam_at: None,
            os_at: None,
            reading: true,
        }
    }

    #[test]
    fn a_not_allowed_answer_is_asked_again_in_a_minute_not_on_the_next_repaint() {
        let t = Instant::now();
        let mut slot = empty();
        record(&mut slot, Some(Vec::new()), Some(OsUpdates::NotAllowed), t);
        assert!(!slot.reading);
        assert_eq!(due(&slot, t + Duration::from_millis(16)), (false, false));
        assert_eq!(due(&slot, t + Duration::from_secs(59)), (false, false));
        assert_eq!(due(&slot, t + Duration::from_secs(60)), (false, true));
    }

    #[test]
    fn a_package_count_holds_six_hours_and_steam_ten_minutes() {
        let t = Instant::now();
        let mut slot = empty();
        record(
            &mut slot,
            Some(vec!["Portal 2".into()]),
            Some(OsUpdates::Pending(3)),
            t,
        );
        assert_eq!(slot.facts.os, OsUpdates::Pending(3));
        assert_eq!(slot.facts.steam, vec!["Portal 2".to_string()]);
        assert_eq!(due(&slot, t + Duration::from_secs(60)), (false, false));
        assert_eq!(due(&slot, t + Duration::from_secs(10 * 60)), (true, false));
        assert_eq!(
            due(&slot, t + Duration::from_secs(6 * 60 * 60)),
            (true, true)
        );
    }

    #[test]
    fn nothing_read_yet_is_due() {
        assert_eq!(due(&empty(), Instant::now()), (true, true));
    }
}
