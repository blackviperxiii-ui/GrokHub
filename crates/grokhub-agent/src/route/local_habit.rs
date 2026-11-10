//! When chats usually start on this device, so the background warm-up
//! (`local_prewarm`) reads the models into RAM a few minutes before. RAM
//! only: nothing loads to the GPU, and no habit warm runs while a game does.
//!
//! A chat start is a local request after [`QUIET_SECS`] without one (or the
//! first since the app opened). Starts are kept as UTC seconds and matched by
//! time of day and time of week, so no time zone is needed; a clock change
//! moves the match by an hour until a couple of weeks of new starts catch up.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

const DAY: u64 = 24 * 60 * 60;
const WEEK: u64 = 7 * DAY;
/// A request after this long without one starts a chat.
pub const QUIET_SECS: u64 = 30 * 60;
/// Warm this far ahead of a usual start.
pub const LEAD_SECS: u64 = 15 * 60;
/// One habit warm per usual start.
pub const AGAIN_SECS: u64 = 45 * 60;
/// A daily habit: a start in this window on this many of the last 14 days.
const DAILY_DAYS: usize = 3;
const DAILY_LOOKBACK: u64 = 14 * DAY;
/// A weekly habit: a start in this window on this many of the last 4 weeks.
const WEEKLY_WEEKS: usize = 2;
const KEEP_SECS: u64 = 4 * WEEK;
const MAX_STARTS: usize = 400;

static LAST_REQUEST: AtomicU64 = AtomicU64::new(0);
static LAST_WARM: AtomicU64 = AtomicU64::new(0);
/// Starts not yet saved.
static NEW: Mutex<Vec<u64>> = Mutex::new(Vec::new());
/// The saved starts, read once.
static STARTS: Mutex<Option<Vec<u64>>> = Mutex::new(None);

/// A request at `now` after one at `last` (0: none yet) starts a chat.
pub fn is_start(last: u64, now: u64) -> bool {
    last == 0 || now.saturating_sub(last) >= QUIET_SECS
}

/// A local request began at `now` (unix seconds).
pub fn note_request(now: u64) {
    if is_start(LAST_REQUEST.swap(now, Ordering::SeqCst), now) {
        NEW.lock().unwrap_or_else(|e| e.into_inner()).push(now);
    }
}

/// `start` falls between `now` and [`LEAD_SECS`] later, as a time of day
/// (`period` [`DAY`]) or of week ([`WEEK`]).
fn ahead(start: u64, now: u64, period: u64) -> bool {
    (start % period + period - now % period) % period <= LEAD_SECS
}

/// A usual start is due within [`LEAD_SECS`] of `now`.
pub fn chat_soon(starts: &[u64], now: u64) -> bool {
    let distinct = |lookback: u64, period: u64| {
        let mut seen: Vec<u64> = starts
            .iter()
            .filter(|s| **s < now && now - **s <= lookback && ahead(**s, now, period))
            .map(|s| *s / period)
            .collect();
        seen.sort_unstable();
        seen.dedup();
        seen.len()
    };
    distinct(DAILY_LOOKBACK, DAY) >= DAILY_DAYS || distinct(KEEP_SECS, WEEK) >= WEEKLY_WEEKS
}

/// Drop starts older than [`KEEP_SECS`] and keep the newest [`MAX_STARTS`].
pub fn prune(starts: &mut Vec<u64>, now: u64) {
    starts.retain(|s| now.saturating_sub(*s) <= KEEP_SECS);
    starts.sort_unstable();
    if starts.len() > MAX_STARTS {
        starts.drain(..starts.len() - MAX_STARTS);
    }
}

fn file() -> Option<PathBuf> {
    super::local_autoconfig::tune_dir().map(|d| d.join("chat_starts.json"))
}

fn load(path: &Path) -> Vec<u64> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// Save new starts, then say whether to warm now: a usual start is near and
/// no habit warm ran for it yet. The caller skips this while a game runs.
pub fn tick(now: u64) -> bool {
    let Some(path) = file() else {
        return false;
    };
    let fresh = std::mem::take(&mut *NEW.lock().unwrap_or_else(|e| e.into_inner()));
    let mut guard = STARTS.lock().unwrap_or_else(|e| e.into_inner());
    let starts = guard.get_or_insert_with(|| load(&path));
    if !fresh.is_empty() {
        starts.extend(fresh);
        prune(starts, now);
        if let Ok(text) = serde_json::to_string(starts) {
            let _ = grokhub_core::pulse_writer::write_atomic(&path, &text);
        }
    }
    if !chat_soon(starts, now) || now.saturating_sub(LAST_WARM.load(Ordering::SeqCst)) < AGAIN_SECS
    {
        return false;
    }
    LAST_WARM.store(now, Ordering::SeqCst);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Monday 2026-11-09 00:00 UTC.
    const MON: u64 = 1_794_182_400;
    const H: u64 = 60 * 60;

    #[test]
    fn three_days_at_the_same_time_make_a_daily_habit() {
        let now = MON + 7 * DAY + 19 * H;
        // 19:10 on three earlier days.
        let starts = [1, 2, 4].map(|d| now - d * DAY + 10 * 60);
        assert!(chat_soon(&starts, now));
        assert!(!chat_soon(&starts[..2], now), "two days is not a habit yet");
        // Twenty minutes ahead is past the lead.
        assert!(!chat_soon(&starts, now - 10 * 60));
        // Started already today, an hour ago: nothing ahead from that.
        assert!(!chat_soon(
            &[now - H, now - DAY - H, now - 2 * DAY - H],
            now
        ));
        // Older than two weeks doesn't count for the daily habit.
        let old = [15, 16, 17].map(|d| now - d * DAY + 10 * 60);
        assert!(!chat_soon(&old, now));
    }

    #[test]
    fn two_weeks_on_the_same_weekday_make_a_weekly_habit() {
        let now = MON + 2 * WEEK + 20 * H;
        let starts = [now - WEEK + 5 * 60, now - 2 * WEEK + 5 * 60];
        assert!(chat_soon(&starts, now));
        assert!(!chat_soon(&starts[..1], now));
        // The same time on another weekday is no weekly match.
        assert!(!chat_soon(
            &[now - WEEK - DAY + 5 * 60, now - 2 * WEEK - DAY + 5 * 60],
            now
        ));
    }

    #[test]
    fn only_a_request_after_a_quiet_spell_starts_a_chat() {
        let base = MON + 3 * WEEK;
        assert!(is_start(0, base), "the first request since the app opened");
        assert!(!is_start(base, base + 60));
        assert!(!is_start(base, base + QUIET_SECS - 1));
        assert!(is_start(base, base + QUIET_SECS));
    }

    #[test]
    fn prune_keeps_four_weeks_and_a_cap() {
        let now = MON + 10 * WEEK;
        let mut starts = vec![now - 5 * WEEK, now - DAY, now - 3 * WEEK];
        prune(&mut starts, now);
        assert_eq!(starts, vec![now - 3 * WEEK, now - DAY]);
        let mut many: Vec<u64> = (0..MAX_STARTS as u64 + 10).map(|i| now - i).collect();
        prune(&mut many, now);
        assert_eq!(many.len(), MAX_STARTS);
        assert_eq!(*many.last().unwrap(), now);
    }
}
