//! Tests for the shell's Pulse digests and offers: a temp config dir and a
//! fixed clock, so each digest lands at its slot exactly once, a fresh
//! cabin lease silences the shell, and an off source stays off.

use super::*;
use std::sync::atomic::{AtomicU32, Ordering};

static SEQ: AtomicU32 = AtomicU32::new(0);

fn temp_config() -> PathBuf {
    let n = SEQ.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!(
        "grokhub-pulse-digests-test-{}-{n}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp config dir");
    dir
}

/// Wednesday 2026-11-11 08:00 UTC.
const WED_8AM: u64 = 1_794_384_000_000;
/// Sunday 2026-11-15 20:00 UTC.
const SUN_8PM: u64 = 1_794_772_800_000;

fn ctx(dir: &Path, now: u64) -> Ctx {
    Ctx {
        config_dir: dir.to_path_buf(),
        now,
        clock: grokhub_core::LocalClock::at_offset(now, 0),
        day: local_day(now, 0),
        quiet: false,
    }
}

fn live_cards(dir: &Path) -> Vec<UpdateCard> {
    crate::pulse::cards(dir).unwrap_or_default()
}

#[test]
fn day_brief_posts_once_in_the_morning() {
    let dir = temp_config();
    let c = ctx(&dir, WED_8AM);
    assert_eq!(day_brief(&c), 1, "the morning slot posts the brief");
    assert_eq!(day_brief(&c), 0, "a second tick in the same slot is quiet");
    let cards = live_cards(&dir);
    assert_eq!(
        cards
            .iter()
            .filter(|k| k.source_id.starts_with(db::CARD_SOURCE_PREFIX))
            .count(),
        1
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn day_brief_stays_quiet_outside_the_fallback_hours() {
    let dir = temp_config();
    // 03:00 UTC — outside the 05:00–14:00 fallback delivery window.
    let early = ctx(&dir, WED_8AM - 5 * 3_600_000);
    assert_eq!(day_brief(&early), 0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn systems_brief_posts_once_on_sunday_night() {
    let dir = temp_config();
    let c = ctx(&dir, SUN_8PM);
    let sw = switches(&dir);
    assert_eq!(systems_brief(&c, &sw), 1, "Sunday night posts the brief");
    assert_eq!(systems_brief(&c, &sw), 0, "and not twice");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn week_recap_posts_once_on_sunday_night() {
    let dir = temp_config();
    let c = ctx(&dir, SUN_8PM);
    assert_eq!(week_recap(&c), 1, "Sunday night posts the recap");
    assert_eq!(week_recap(&c), 0, "and not twice");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn fresh_cabin_lease_silences_the_shell() {
    let dir = temp_config();
    // Another process holds a fresh lease: the shell must stay out.
    let lease = grokhub_core::scheduler_lease::SchedulerLease {
        owner: grokhub_core::scheduler_lease::LeaseOwner::Cabin,
        pid: std::process::id() + 1,
        beat_ms: WED_8AM,
    };
    grokhub_core::scheduler_lease::write_lease(&dir, &lease).expect("write lease");
    assert_eq!(run_due(&dir, WED_8AM), 0);
    assert!(live_cards(&dir).is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn off_sources_stay_off() {
    let dir = temp_config();
    app_json::patch(
        &dir,
        &[
            ("dailyDrop", serde_json::json!({"enabled": false})),
            ("askOffers", serde_json::json!({"enabled": false})),
            (
                "hobbyPaths",
                serde_json::json!({"tips": false, "projects": false}),
            ),
        ],
    )
    .expect("seed settings");
    let c = ctx(&dir, WED_8AM);
    let sw = switches(&dir);
    assert_eq!(daily_drop(&c, &sw), 0);
    assert_eq!(ask_offers(&c, &sw), 0);
    assert_eq!(hobby_paths(&c, &sw), 0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn three_repeated_asks_make_one_offer() {
    let dir = temp_config();
    // The same ask at the same hour on three different days: repeats
    // inside one hour fold into a single log entry by design.
    let ask = "check the greenhouse humidity readings";
    for i in 0..3 {
        note_ask(&dir, ask, WED_8AM + i * 86_400_000);
    }
    // Age the ledger past the tick gate, then tick.
    let mut ledger = load_ask_ledger(&dir);
    ledger.last_tick_ms = 0;
    save_ask_ledger(&dir, &ledger);
    let c = ctx(&dir, WED_8AM + 2 * 86_400_000 + 5 * 60_000);
    let sw = switches(&dir);
    assert_eq!(ask_offers(&c, &sw), 1, "three repeats earn one offer");
    let all = live_cards(&dir);
    let offers: Vec<&UpdateCard> = all
        .iter()
        .filter(|k| k.source_id.starts_with(ao::SOURCE_PREFIX))
        .collect();
    assert_eq!(offers.len(), 1);
    // A later tick offers nothing new for the same ask.
    let mut ledger = load_ask_ledger(&dir);
    ledger.last_tick_ms = 0;
    save_ask_ledger(&dir, &ledger);
    let later = ctx(&dir, WED_8AM + 2 * 86_400_000 + 10 * 60_000);
    assert_eq!(ask_offers(&later, &sw), 0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn sources_view_round_trips_through_app_json() {
    let dir = temp_config();
    let mut view = sources_view(&dir);
    assert!(view.ask_offers_enabled, "cabin default: ask offers on");
    assert!(
        !view.daily_drop.enabled,
        "daily drop is off until turned on"
    );
    assert!(!view.release_radar_on, "radar is off until turned on");
    view.ask_offers_enabled = false;
    view.daily_drop.enabled = true;
    view.daily_drop.hacker_news = true;
    let saved = save_sources(&dir, view).expect("save sources");
    assert!(!saved.ask_offers_enabled);
    assert!(saved.daily_drop.enabled);
    assert!(saved.daily_drop.hacker_news);
    let reread = sources_view(&dir);
    assert!(!reread.ask_offers_enabled);
    assert!(reread.daily_drop.enabled);
    // The radar switch lives in its own file.
    let mut view = reread;
    view.release_radar_on = true;
    save_sources(&dir, view).expect("save radar on");
    assert!(sources_view(&dir).release_radar_on);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn helper_offer_accept_and_dismiss_paths() {
    let dir = temp_config();
    let c = ctx(&dir, WED_8AM);
    let sw = switches(&dir);
    assert_eq!(
        helper_offers(&c, &sw),
        2,
        "both off-by-default helpers offer once"
    );
    assert_eq!(
        helper_offers(&c, &sw),
        0,
        "and not again while the cards live"
    );
    apply_helper_accept(&dir, HelperOffer::Tidy).expect("accept tidy");
    assert!(switches(&dir).downloads_tidy);
    note_helper_dismissed(&dir, HelperOffer::WindDown).expect("dismiss wind down");
    assert!(switches(&dir)
        .helper_dismissed
        .iter()
        .any(|d| d == "wind_down"));
    let _ = std::fs::remove_dir_all(&dir);
}
