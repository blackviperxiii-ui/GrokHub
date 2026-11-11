use super::*;
use grokhub_core::scheduler_lease::{write_lease, LeaseOwner, SchedulerLease};
use std::cell::Cell;

/// A Friday, 02:00 UTC.
const NOW: u64 = 1_800_000_000_000 - (1_800_000_000_000 % 86_400_000) + 2 * 3_600_000;

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "grokhub-helper-loop-{name}-{}-{}",
        std::process::id(),
        grokhub_core::now_ms()
    ));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn clear(idle_secs: u64) -> Machine {
    Machine {
        idle_secs: Some(idle_secs),
        game: Some(false),
        fullscreen: Some(false),
        external_gpu_pct: Some(3),
    }
}

fn cabin_beat(d: &Path, beat_ms: u64) {
    write_lease(
        d,
        &SchedulerLease {
            owner: LeaseOwner::Cabin,
            pid: 99,
            beat_ms,
        },
    )
    .unwrap();
}

fn wind_on(d: &Path, auto: Option<u32>) {
    let t = wind_down::Tracker {
        on: true,
        auto_sleep_min: auto,
        ..Default::default()
    };
    wind_down::save(d, &t).unwrap();
}

fn rhythm_off(d: &Path) {
    let r = rhythm::Rhythm {
        on: false,
        ..Default::default()
    };
    rhythm::save(&d.join(rhythm::FILE_NAME), &r).unwrap();
}

fn run(d: &Path, now: u64, focused: bool, m: Machine, slept: &Cell<u32>) -> Did {
    tick(d, 7, now, 0, focused, &|| m, &|| {
        slept.set(slept.get() + 1);
        Ok(())
    })
    .unwrap()
}

#[test]
fn a_fresh_cabin_beat_means_the_tick_does_nothing() {
    let d = dir("fresh");
    wind_on(&d, Some(30));
    cabin_beat(&d, NOW - 10_000);
    let never = || -> Machine { panic!("read while the cabin runs") };
    let did = tick(&d, 7, NOW, 0, true, &never, &|| panic!("slept")).unwrap();
    assert_eq!(did, Did::default());
    assert_eq!(wind_down::load(&d).last_sleep_ms, None);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn with_both_helpers_off_nothing_is_read() {
    let d = dir("off");
    rhythm_off(&d);
    cabin_beat(&d, NOW - 200_000);
    let never = || -> Machine { panic!("read with everything off") };
    let did = tick(&d, 7, NOW, 0, true, &never, &|| panic!("slept")).unwrap();
    assert_eq!(did, Did::default());
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_stale_beat_and_an_idle_clear_pc_sleeps_once_then_waits_out_the_retry_gap() {
    let d = dir("sleep");
    wind_on(&d, Some(30));
    rhythm_off(&d);
    cabin_beat(&d, NOW - 200_000);
    let slept = Cell::new(0);
    let did = run(&d, NOW, false, clear(31 * 60), &slept);
    assert!(did.slept);
    assert_eq!(slept.get(), 1);
    assert_eq!(wind_down::load(&d).last_sleep_ms, Some(NOW));
    // The PC woke a minute later, still reading idle: not asked again.
    let did = run(&d, NOW + 60_000, false, clear(32 * 60), &slept);
    assert!(!did.slept);
    assert_eq!(slept.get(), 1);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn it_does_not_sleep_while_the_user_is_here_or_something_is_running() {
    let d = dir("busy");
    wind_on(&d, Some(30));
    rhythm_off(&d);
    let slept = Cell::new(0);
    run(&d, NOW, false, clear(5 * 60), &slept);
    let game = Machine {
        game: Some(true),
        ..clear(40 * 60)
    };
    run(&d, NOW, false, game, &slept);
    let full = Machine {
        fullscreen: Some(true),
        ..clear(40 * 60)
    };
    run(&d, NOW, false, full, &slept);
    let render = Machine {
        external_gpu_pct: Some(80),
        ..clear(40 * 60)
    };
    run(&d, NOW, false, render, &slept);
    assert_eq!(slept.get(), 0);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_pc_that_cannot_see_the_gpu_never_sleeps_and_says_so() {
    let d = dir("blind");
    wind_on(&d, Some(30));
    rhythm_off(&d);
    let slept = Cell::new(0);
    let blind = Machine {
        external_gpu_pct: None,
        ..clear(90 * 60)
    };
    let did = run(&d, NOW, false, blind, &slept);
    assert!(!did.slept);
    assert_eq!(slept.get(), 0);
    assert!(crate::pc_helpers::read(&d).sleep_blind);
    run(&d, NOW + 60_000, false, clear(0), &slept);
    assert!(!crate::pc_helpers::read(&d).sleep_blind);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn the_setup_offer_is_left_to_the_settings_page() {
    let d = dir("offer");
    wind_on(&d, None);
    rhythm_off(&d);
    let slept = Cell::new(0);
    let did = run(&d, NOW, true, clear(30), &slept);
    assert_eq!(did, Did::default());
    let t = wind_down::load(&d);
    assert!(t.on && !t.setup_offered && t.auto_sleep_min.is_none());
    assert_eq!(slept.get(), 0);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn the_rhythm_counts_a_minute_only_while_the_user_is_at_this_window() {
    let d = dir("rhythm");
    let path = d.join(rhythm::FILE_NAME);
    let slept = Cell::new(0);
    // Away from the window, or no input for ten minutes: nothing counts.
    assert!(!run(&d, NOW, false, clear(10), &slept).counted_minute);
    assert!(!run(&d, NOW, true, clear(600), &slept).counted_minute);
    assert_eq!(rhythm::load(&path).total_minutes(), 0);
    // At the window: one minute, in Friday's 2 AM slot.
    assert!(run(&d, NOW, true, clear(10), &slept).counted_minute);
    let r = rhythm::load(&path);
    assert_eq!(r.total_minutes(), 1);
    assert_eq!(r.at(5, 2), 1);
    // The same minute twice counts once; the next minute counts.
    assert!(!run(&d, NOW + 20_000, true, clear(10), &slept).counted_minute);
    assert!(run(&d, NOW + 60_000, true, clear(10), &slept).counted_minute);
    assert_eq!(rhythm::load(&path).total_minutes(), 2);
    let _ = std::fs::remove_dir_all(&d);
}
