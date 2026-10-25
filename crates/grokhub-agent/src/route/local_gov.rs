//! Applies the live governor's level (`grokhub_core::local_governor`) to
//! GrokHub's own sidecar and nothing else. RampUp, Normal and Throttle set the
//! auto-config state for the next launch and mark a running sidecar for a
//! restart, which [`local_sidecar`](super::local_sidecar) does only while no
//! request is in flight. Unload pauses the local route and stops the sidecar
//! between requests. A crash-loop breaker refuses starts after 3 failures in
//! 10 minutes, for 30 minutes.

use std::sync::{Mutex, OnceLock};

use grokhub_core::local_governor::{CrashBreaker, Level};

use super::local_autoconfig::{self as autoconfig, GovState};

/// The auto-config state for a level; `Unload` never launches.
pub fn gov_state(level: Level) -> Option<GovState> {
    match level {
        Level::Unload => None,
        Level::Throttle => Some(GovState::Throttle),
        Level::Normal => Some(GovState::Normal),
        Level::RampUp => Some(GovState::RampUp),
    }
}

struct Gov {
    level: Level,
    reason: &'static str,
    restart_due: bool,
    breaker: CrashBreaker,
}

fn gov() -> &'static Mutex<Gov> {
    static G: OnceLock<Mutex<Gov>> = OnceLock::new();
    G.get_or_init(|| {
        Mutex::new(Gov {
            level: Level::Normal,
            reason: "normal use",
            restart_due: false,
            breaker: CrashBreaker::default(),
        })
    })
}

/// The sampler's new level and why.
pub fn apply(level: Level, reason: &'static str) {
    let Ok(mut g) = gov().lock() else {
        return;
    };
    g.reason = reason;
    if g.level == level {
        return;
    }
    if let Some(s) = gov_state(level) {
        autoconfig::set_state(s);
    }
    // Into Unload stops the sidecar; out of it starts fresh. Between two
    // running levels the launch args change, so the running one restarts.
    g.restart_due = g.level != Level::Unload && level != Level::Unload;
    g.level = level;
}

pub fn level() -> Level {
    gov().lock().map(|g| g.level).unwrap_or_default()
}

/// The local route is paused (Unload).
pub fn paused() -> bool {
    level() == Level::Unload
}

/// Why the governor is where it is, for the pause message.
pub fn reason() -> &'static str {
    gov().lock().map(|g| g.reason).unwrap_or("normal use")
}

pub fn restart_due() -> bool {
    gov().lock().map(|g| g.restart_due).unwrap_or(false)
}

pub fn clear_restart() {
    if let Ok(mut g) = gov().lock() {
        g.restart_due = false;
    }
}

/// `Err` while the crash-loop breaker is open.
pub fn may_start(now_ms: u64) -> Result<(), String> {
    let open = gov()
        .lock()
        .map(|g| !g.breaker.allows(now_ms))
        .unwrap_or(false);
    if open {
        return Err(
            "the local model server kept stopping, so GrokHub paused it for 30 minutes".into(),
        );
    }
    Ok(())
}

/// A start failed or a running sidecar was found dead.
pub fn start_failed(now_ms: u64) {
    if let Ok(mut g) = gov().lock() {
        g.breaker.failed(now_ms);
    }
}

#[cfg(test)]
pub(crate) fn reset_for_test() {
    if let Ok(mut g) = gov().lock() {
        g.level = Level::Normal;
        g.reason = "normal use";
        g.restart_due = false;
        g.breaker = CrashBreaker::default();
    }
    autoconfig::set_state(GovState::Normal);
}

#[cfg(test)]
#[path = "local_gov_tests.rs"]
mod tests;
