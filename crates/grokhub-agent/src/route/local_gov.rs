//! Applies the live governor's level (`grokhub_core::local_governor`) to
//! GrokHub's own sidecar and nothing else. RampUp, Normal and Throttle set the
//! auto-config state for the next launch and mark a running sidecar for a
//! restart, which [`local_sidecar`](super::local_sidecar) does only while no
//! request is in flight. Unload pauses the local route and stops the sidecar
//! between requests. A crash-loop breaker refuses starts after 3 failures in
//! 10 minutes, for 30 minutes.
//!
//! Each level change is one line in `{config}/local/governor.jsonl`, on this
//! device only: lines older than 7 days drop on the next write, and the file
//! can be deleted at any time. Heavy background steps wait for RampUp in
//! [`wait_for_ramp_up`].

use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

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

/// The governor log, next to `governor.json`.
pub const LOG_FILE: &str = "governor.jsonl";
/// Log lines older than this drop on the next write.
pub const LOG_KEEP_MS: u64 = 7 * 24 * 60 * 60 * 1_000;

struct Gov {
    level: Level,
    reason: &'static str,
    restart_due: bool,
    breaker: CrashBreaker,
    log_dir: Option<PathBuf>,
    /// Heavy background steps waiting for RampUp.
    queued: usize,
}

/// Wakes [`wait_for_ramp_up`] on every level change.
fn changed() -> &'static Condvar {
    static C: OnceLock<Condvar> = OnceLock::new();
    C.get_or_init(Condvar::new)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn gov() -> &'static Mutex<Gov> {
    static G: OnceLock<Mutex<Gov>> = OnceLock::new();
    G.get_or_init(|| {
        Mutex::new(Gov {
            level: Level::Normal,
            reason: "normal use",
            restart_due: false,
            breaker: CrashBreaker::default(),
            log_dir: None,
            queued: 0,
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
    if let Some(dir) = &g.log_dir {
        let _ = append_log(dir, now_ms(), g.level, level, reason);
    }
    g.level = level;
    changed().notify_all();
}

/// Where [`apply`] logs level changes (`{config}/local`).
pub fn set_log_dir(dir: PathBuf) {
    if let Ok(mut g) = gov().lock() {
        g.log_dir = Some(dir);
    }
}

/// One change as a `governor.jsonl` line.
pub fn log_line(at_ms: u64, from: Level, to: Level, reason: &str) -> String {
    serde_json::json!({
        "at": at_ms,
        "from": from.label(),
        "to": to.label(),
        "reason": reason,
    })
    .to_string()
}

/// Append one change, dropping lines older than [`LOG_KEEP_MS`].
pub fn append_log(
    dir: &Path,
    now_ms: u64,
    from: Level,
    to: Level,
    reason: &str,
) -> std::io::Result<()> {
    let path = dir.join(LOG_FILE);
    let since = now_ms.saturating_sub(LOG_KEEP_MS);
    let old = std::fs::read_to_string(&path).unwrap_or_default();
    let mut out: String = old
        .lines()
        .filter(|l| {
            serde_json::from_str::<serde_json::Value>(l)
                .ok()
                .and_then(|v| v.get("at").and_then(|a| a.as_u64()))
                .is_some_and(|at| at >= since)
        })
        .map(|l| format!("{l}\n"))
        .collect();
    out.push_str(&log_line(now_ms, from, to, reason));
    out.push('\n');
    std::fs::create_dir_all(dir)?;
    std::fs::write(path, out)
}

/// Heavy background steps waiting for RampUp now.
pub fn queued() -> usize {
    gov().lock().map(|g| g.queued).unwrap_or(0)
}

/// How often a waiting step looks at its cancel flag.
const CANCEL_POLL: Duration = Duration::from_millis(250);

/// Hold a heavy background step until the machine is idle on AC (RampUp),
/// for at most `limit`. Returns the level it left at: RampUp, Unload (the
/// step can't run), or whatever held when `limit` ran out; `None` once
/// `cancelled` says Stop was pressed.
pub fn wait_for_ramp_up(limit: Duration, cancelled: &dyn Fn() -> bool) -> Option<Level> {
    let Ok(mut g) = gov().lock() else {
        return Some(Level::Normal);
    };
    let until = Instant::now() + limit;
    g.queued += 1;
    let mut stopped = false;
    while !matches!(g.level, Level::RampUp | Level::Unload) {
        if cancelled() {
            stopped = true;
            break;
        }
        let left = until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            break;
        }
        g = match changed().wait_timeout(g, left.min(CANCEL_POLL)) {
            Ok((g, _)) => g,
            Err(e) => e.into_inner().0,
        };
    }
    g.queued -= 1;
    (!stopped).then_some(g.level)
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
        return Err(format!(
            "the local model server kept stopping, so {product} paused it for 30 minutes",
            product = grokhub_core::brand::product_name()
        ));
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
        g.log_dir = None;
    }
    autoconfig::set_state(GovState::Normal);
}

#[cfg(test)]
#[path = "local_gov_tests.rs"]
mod tests;
