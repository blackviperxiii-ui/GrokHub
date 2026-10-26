//! Voice, hotkeys, clock and day tests (rearch stage 5).
//!
//! Mechanical move from `tests.rs`. Wired via `#[path]` so
//! `app/mod.rs` stays untouched.

use super::*;

// ---- Cursor fold: Voice and hotkeys ----

// Folded from PR #254.
// Voice is on Ready and the cabin is not running, so leaving voice must not halt a run.
#[test]
fn leave_voice_while_idle_stays_off_a_run() {
    let mut app = Cabin::quiet_for_test();
    app.voice_state = grokhub_core::VoiceState::Ready;
    app.voice_orb = "live".into();
    app.leave_voice();
    assert!(matches!(app.voice_state, grokhub_core::VoiceState::Idle));
    assert_eq!(app.voice_orb, "idle");
    assert_eq!(app.status, "Voice off");
    assert!(app.voice_hold_rx.is_none());
    assert!(app.voice_ready_at.is_none());
    assert!(!app.running);
    assert!(app.chat_job_thread.is_none());
}

// Folded from PR #263.
#[test]
fn voice_hold_while_idle_stays_off_a_run() {
    let mut app = Cabin::quiet_for_test();
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    app.voice_hold_rx = Some(rx);
    app.voice_state = VoiceState::Idle;
    app.poll_voice_hold();
    assert!(app.voice_hold_rx.is_some());
    assert!(matches!(app.voice_state, VoiceState::Idle));
    assert!(!app.running);
    assert!(app.chat_job_thread.is_none());
    drop(tx);
    app.poll_voice_hold();
    assert!(app.voice_hold_rx.is_none());
    assert!(matches!(app.voice_state, VoiceState::Idle));
    assert!(!app.running);
    assert!(app.chat_job_thread.is_none());
}

// Folded from PR #290.
#[test]
fn voice_mode_is_on_when_ready() {
    let mut app = Cabin::quiet_for_test();
    app.voice_state = VoiceState::Idle;
    assert!(!app.voice_is_on());
    app.voice_state = VoiceState::Ready;
    assert!(app.voice_is_on());
    app.voice_state = VoiceState::Idle;
    assert!(!app.voice_is_on());
    assert!(!app.running);
    assert!(app.chat_job_thread.is_none());
}

// Folded from PR #307.
#[test]
fn global_hotkeys_poll_stays_off_without_manager() {
    let mut cabin = Cabin::quiet_for_test();
    cabin.status = "Harbor".into();
    // quiet_for_test leaves hotkeys None — early return before listen_voice / halt_work.
    assert!(cabin.hotkeys.is_none());
    cabin.poll_global_hotkeys();
    assert_eq!(cabin.status, "Harbor");
    assert!(!cabin.running);
    assert!(matches!(cabin.voice_state, VoiceState::Idle));
    assert!(!cabin.voice_is_on());
    assert!(cabin.chat_job_thread.is_none());
}

// ---- Cursor fold: Clock, day and host shells ----

// Folded from PR #387.
#[test]
fn day_now_returns_yyyy_mm_dd() {
    // Associated fn: shells `date +%F`, or falls back to 1970-01-01. No network/xAI.
    let day = Cabin::day_now();
    let ok = day.len() == 10
        && day.as_bytes()[4] == b'-'
        && day.as_bytes()[7] == b'-'
        && day
            .bytes()
            .enumerate()
            .all(|(i, b)| i == 4 || i == 7 || b.is_ascii_digit());
    assert!(
        ok,
        "day_now must be YYYY-MM-DD (or the 1970-01-01 fallback), got {day:?}"
    );
}

// Folded from PR #390.
#[test]
fn clock_now_returns_local_clock() {
    // Associated fn: shells `date` (or noon Monday fallback). No network/xAI.
    let clock = Cabin::clock_now();
    assert!(
        clock.weekday <= 6 && clock.hour <= 23 && clock.minute <= 59,
        "LocalClock out of range (or expect weekday=1 hour=12 minute=0 fallback): weekday={} hour={} minute={}",
        clock.weekday,
        clock.hour,
        clock.minute
    );
}

// Folded from PR #393.
#[test]
fn local_clock_returns_cached_or_fresh() {
    // Associated fn: cache hit or `clock_now` via `date` (or noon Monday fallback).
    // No network/xAI. Call twice so the second path is cached-or-fresh.
    let a = Cabin::local_clock();
    let b = Cabin::local_clock();
    for (label, clock) in [("first", a), ("second", b)] {
        assert!(
            clock.weekday <= 6 && clock.hour <= 23 && clock.minute <= 59,
            "{label} LocalClock out of range (or expect weekday=1 hour=12 minute=0 fallback): weekday={} hour={} minute={}",
            clock.weekday,
            clock.hour,
            clock.minute
        );
    }
}

// Folded from PR #395.
#[test]
fn date_out_formats_via_date() {
    // Associated fn: shells `date` with fmt; empty on failure. No network/xAI.
    let out = Cabin::date_out("+%F");
    let ok = out.is_empty()
        || (out.len() == 10
            && out.as_bytes()[4] == b'-'
            && out.as_bytes()[7] == b'-'
            && out
                .bytes()
                .enumerate()
                .all(|(i, b)| i == 4 || i == 7 || b.is_ascii_digit()));
    assert!(
        ok,
        "date_out(+%F) must be YYYY-MM-DD shaped or empty on failure, got {out:?}"
    );
}

// Folded from PR #396.
#[test]
fn kick_local_clock_stays_off_network() {
    // Associated fn: background `date` refresh into LAST_CLOCK. No grok/xAI/network.
    let cabin = Cabin::quiet_for_test();
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());

    if let Ok(mut g) = super::LAST_CLOCK.lock() {
        *g = None;
    }
    Cabin::kick_local_clock();

    let clock = (0..80)
        .find_map(|_| {
            if let Ok(g) = super::LAST_CLOCK.lock() {
                if let Some((_, clock, inflight)) = g.as_ref() {
                    if !*inflight {
                        return Some(*clock);
                    }
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
            None
        })
        .expect("kick_local_clock should cache a LocalClock (date shell OK)");

    assert!(
        clock.weekday <= 6 && clock.hour <= 23 && clock.minute <= 59,
        "cached LocalClock out of range: weekday={} hour={} minute={}",
        clock.weekday,
        clock.hour,
        clock.minute
    );
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
}

// Folded from PR #397.
#[test]
fn kick_local_day_stays_off_network() {
    let cabin = Cabin::quiet_for_test();
    // Seed LAST_DAY so kick marks inflight and refreshes via `day_now` / `date +%F`.
    let _ = Cabin::local_day();
    Cabin::kick_local_day();
    // Date shell OK; wait for the worker to rewrite the cache. No grok/xAI.
    let mut day = String::new();
    for _ in 0..100 {
        std::thread::sleep(std::time::Duration::from_millis(10));
        day = Cabin::local_day();
        let shaped = day.len() == 10
            && day.as_bytes()[4] == b'-'
            && day.as_bytes()[7] == b'-'
            && day
                .bytes()
                .enumerate()
                .all(|(i, b)| i == 4 || i == 7 || b.is_ascii_digit());
        if shaped {
            break;
        }
    }
    let ok = day.len() == 10
        && day.as_bytes()[4] == b'-'
        && day.as_bytes()[7] == b'-'
        && day
            .bytes()
            .enumerate()
            .all(|(i, b)| i == 4 || i == 7 || b.is_ascii_digit());
    assert!(
        ok,
        "kick_local_day must refresh/cache YYYY-MM-DD (date shell OK), got {day:?}"
    );
    assert!(!cabin.running, "kick_local_day must stay off grok/xAI");
    assert!(cabin.chat_job_thread.is_none());
    assert!(cabin.rx.is_none());
}

// Folded from PR #401.
#[test]
fn hostname_i_now_returns_string() {
    // `hostname -I` on Linux, the adapter list on Windows, or the routed address: every
    // word is an IP address (IPv6 included on Linux). Stays off the network.
    let out = hostname_i_now();
    for word in out.split_whitespace() {
        assert!(
            word.parse::<std::net::IpAddr>().is_ok(),
            "{word:?} in {out:?}"
        );
    }
}

// Folded from PR #402.
#[test]
fn chip_hour_returns_0_through_23() {
    // Associated fn: hour from local_clock (cached date shell or noon fallback). No network/xAI.
    let hour = Cabin::chip_hour();
    assert!(
        (0..=23).contains(&hour),
        "chip_hour must be 0..=23 (or noon fallback hour=12), got {hour}"
    );
}

#[test]
fn halt_shortcut_is_ctrl_alt_h_not_the_task_manager_key() {
    let src = include_str!("mod.rs");
    // The hotkey halts the live turn and every background run.
    let halt = src
        .split("self.halt_everything(\"Stopped\");")
        .next()
        .and_then(|s| s.rsplit("if ctx.input(").next())
        .expect("in-app halt shortcut");
    assert!(
        halt.contains("i.modifiers.command")
            && halt.contains("i.modifiers.alt")
            && halt.contains("!i.modifiers.shift")
            && halt.contains("egui::Key::H"),
        "Ctrl+Alt+H halts: {halt}"
    );
    assert!(
        !src.contains("i.modifiers.shift && i.key_pressed(egui::Key::Escape)"),
        "Ctrl+Shift+Esc opens Task Manager on Windows and never reaches the app"
    );
}
