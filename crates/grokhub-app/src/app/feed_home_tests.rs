//! Feed, pulse and home tests (rearch stage 5).
//!
//! Mechanical move from `tests.rs`. Wired via `#[path]` so
//! `app/mod.rs` stays untouched.

use super::*;

// ---- Cursor fold: Feed, pulse and home ----

// Folded from PR #275.
#[test]
fn pulse_paints_only_on_an_empty_signed_in_chat() {
    assert!(should_paint_pulse(true, false, true));
    assert!(!should_paint_pulse(false, false, true));
    assert!(!should_paint_pulse(true, true, true));
    assert!(!should_paint_pulse(true, false, false));
}

// Folded from PR #330.
#[test]
fn roll_today_stays_put_on_same_day() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("roll-today-same-day");
    let _ = std::fs::remove_dir_all(&root);
    unsafe { std::env::set_var("GROKHUB_CONFIG", &root) };
    let mut cabin = Cabin::quiet_for_test();
    cabin.status = "Harbor".into();
    let today = Cabin::local_day();
    cabin.usage.day = today.clone();
    cabin.roll_today();
    assert_eq!(
        cabin.usage.day, today,
        "same-day roll must leave usage.day alone"
    );
    assert_eq!(cabin.status, "Harbor");
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
    drop(cabin);
    let _ = std::fs::remove_dir_all(&root);
    unsafe { std::env::remove_var("GROKHUB_CONFIG") };
}

// Folded from PR #375.
#[test]
fn tick_home_surface_remembers_nav() {
    let mut cabin = Cabin::quiet_for_test();
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
    assert!(cabin.chip_memory.last_surface.is_none());
    cabin.set_nav_id("imagine");
    cabin.tick_home_surface();
    assert_eq!(cabin.chip_memory.last_surface.as_deref(), Some("imagine"));
    let stamped = cabin.chip_memory.updated_at;
    assert!(stamped > 0);
    // Same surface again is a no-op — updated_at stays put.
    cabin.tick_home_surface();
    assert_eq!(cabin.chip_memory.last_surface.as_deref(), Some("imagine"));
    assert_eq!(cabin.chip_memory.updated_at, stamped);
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
}

// Folded from PR #376.
#[test]
fn remember_last_frame_keeps_short_url() {
    let mut cabin = Cabin::quiet_for_test();
    assert!(cabin.chat_job_thread.is_none());
    assert!(cabin.last_frame_url.is_none());
    // Short URL sticks; over FRAME_CAP is rejected. No store_hub_frame / live_room / network.
    cabin.remember_last_frame("data:image/jpeg;base64,abc");
    assert_eq!(
        cabin.last_frame_url.as_deref(),
        Some("data:image/jpeg;base64,abc")
    );
    cabin.last_frame_url = None;
    let huge = "x".repeat(FRAME_CAP + 1);
    cabin.remember_last_frame(&huge);
    assert!(cabin.last_frame_url.is_none());
    assert!(cabin.chat_job_thread.is_none());
}

// Folded from PR #378.
#[test]
fn push_presence_keeps_short_url() {
    let mut cabin = Cabin::quiet_for_test();
    assert!(cabin.chat_job_thread.is_none());
    assert!(cabin.presence_ring.is_empty());
    assert!(cabin.last_frame_url.is_none());
    assert!(cabin.rx.is_none());
    assert!(cabin.reflect_rx.is_none());

    let short = "data:image/jpeg;base64,abc".to_string();
    cabin.push_presence(short.clone());
    assert_eq!(cabin.presence_ring.len(), 1);
    assert_eq!(cabin.presence_ring[0].1, short);
    assert!(
        cabin.last_frame_url.is_none(),
        "must not call store_hub_frame/remember_last_frame"
    );

    let oversized = "x".repeat(FRAME_CAP + 1);
    cabin.push_presence(oversized);
    assert_eq!(cabin.presence_ring.len(), 1);
    assert_eq!(cabin.presence_ring[0].1, short);

    assert!(cabin.chat_job_thread.is_none());
    assert!(!cabin.running);
    assert!(cabin.rx.is_none());
    assert!(cabin.reflect_rx.is_none());
}

// Folded from PR #379.
#[test]
fn tool_header_color_failed_vs_ok() {
    assert_eq!(
        super::tool_header_color(true),
        crate::cards::chip_tone_color(crate::cards::ChipTone::Offline),
        "failed tool header uses Offline chip tone"
    );
    assert_eq!(
        super::tool_header_color(false),
        crate::theme::muted(),
        "ok tool header uses muted"
    );
}

// Folded from PR #406.
#[test]
fn device_glance_none_when_hub_off() {
    // Free fn: hub_off + no thumb → None; hub_on → Some("Sharing"). Stay off network/spawn.
    assert!(device_glance(false, None).is_none());
    let share = device_glance(true, None).expect("sharing");
    assert_eq!(share.share_line, "Sharing");
    assert!(!share.has_thumb);
}

// Folded from PR #409. Empty main chat no longer counts a Home deck; ranking stays for Pulse floors.
#[test]
fn home_event_cards_empty_and_one_event() {
    let pulse = grokhub_core::FeedPulse::default();
    assert!(grokhub_core::home_event_cards(&[], &pulse, 0).is_empty());

    let cards = vec![feed_card("event-1", UpdateKind::AutomationDone, false)];
    assert_eq!(grokhub_core::home_event_cards(&cards, &pulse, 0).len(), 1);
}

/// The card menu is gone (2.10.86). A run hidden from Home by an older build
/// still stays off Home, still merges, and Automations still undoes it.
#[test]
fn legacy_hidden_run_stays_off_home_until_undo() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("card-menu-hide");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    unsafe { std::env::set_var("GROKHUB_CONFIG", &root) };
    let _pin = crate::config::TestConfigDir::set(root.clone());

    let mut cabin = Cabin::quiet_for_test();
    cabin.cfg.quiet_start = "00:00".into();
    cabin.cfg.quiet_end = "00:00".into();
    cabin.note_automation_done(
        "zephyr-src-9f3a",
        "ZephyrHideTitle9f3a",
        "ZephyrHideBody9f3a",
    );
    let id = cabin
        .updates
        .iter()
        .find(|card| card.source_id == "zephyr-src-9f3a")
        .map(|card| card.id.clone())
        .expect("run card");
    assert_eq!(
        grokhub_core::home_event_cards(&cabin.updates, &cabin.cfg.feed_pulse, now_ms()).len(),
        1
    );

    // Hidden by an older build's card menu: the source sits in muted_sources.
    assert!(!id.is_empty());
    grokhub_core::hide_home_source(&mut cabin.cfg.feed_pulse, "zephyr-src-9f3a");
    cabin.persist_cfg();
    assert!(
        grokhub_core::home_event_cards(&cabin.updates, &cabin.cfg.feed_pulse, now_ms()).is_empty(),
        "a hidden run stays off Home"
    );
    assert!(
        cabin
            .updates
            .iter()
            .any(|card| card.source_id == "zephyr-src-9f3a"),
        "the card still updates in the store"
    );
    cabin.note_automation_done(
        "zephyr-src-9f3a",
        "ZephyrHideTitle9f3a",
        "ZephyrHideBody9f3a again",
    );
    let stored = cabin
        .updates
        .iter()
        .find(|card| card.source_id == "zephyr-src-9f3a")
        .expect("merged");
    assert!(
        stored.runs >= 2,
        "hidden runs still merge, runs={}",
        stored.runs
    );
    assert!(
        grokhub_core::home_event_cards(&cabin.updates, &cabin.cfg.feed_pulse, now_ms()).is_empty()
    );
    assert_eq!(
        grokhub_core::automation_home_note(&cabin.cfg.feed_pulse, "zephyr-src-9f3a"),
        Some(grokhub_core::HOME_HIDDEN_NOTE)
    );
    let night = include_str!("night.rs");
    assert!(
        night.contains("HOME_HIDDEN_NOTE") && night.contains("undo_hide_automation_from_home"),
        "Automations shows Hidden from Home · Undo"
    );
    assert!(grokhub_core::HOME_HIDDEN_NOTE.contains("Undo"));
    let feed = include_str!("feed_ui.rs");
    assert!(
        feed.contains("fn close_home_card("),
        "close_home_card stays"
    );
    assert!(!feed.contains("Popup"), "feed module has no card menu");
    assert!(!feed.contains("secondary_clicked"), "no right-click menu");
    assert!(!feed.contains("\"⋯\""), "no ⋯ button");
    assert!(
        !feed.contains("fn paint_feed_card("),
        "Home deck paint is gone"
    );

    let app_json = root.join("app.json");
    let mut saw_hide = false;
    for _ in 0..100 {
        if std::fs::read_to_string(&app_json)
            .unwrap_or_default()
            .contains("zephyr-src-9f3a")
        {
            saw_hide = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(saw_hide, "muted source persisted in app.json");

    cabin.undo_hide_automation_from_home("zephyr-src-9f3a");
    assert_eq!(
        grokhub_core::home_event_cards(&cabin.updates, &cabin.cfg.feed_pulse, now_ms()).len(),
        1
    );
    let mut saw_undo = false;
    for _ in 0..100 {
        let body = std::fs::read_to_string(&app_json).unwrap_or_default();
        if !body.contains("zephyr-src-9f3a") {
            saw_undo = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(saw_undo, "Undo drops the source from app.json");

    let signals = std::fs::read_to_string(root.join("card_signals.jsonl")).unwrap_or_default();
    assert!(
        !signals.contains("\"event\":\"hidden\""),
        "nothing hides from a card now: {signals}"
    );
    assert!(signals.contains("\"event\":\"unhidden\""), "{signals}");
    assert!(!signals.contains("ZephyrHideTitle9f3a"), "{signals}");
    assert!(!signals.contains("ZephyrHideBody9f3a"), "{signals}");

    let _ = std::fs::remove_dir_all(&root);
    unsafe { std::env::remove_var("GROKHUB_CONFIG") };
}

#[test]
fn reset_all_clears_what_home_learned() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("home-learned-reset");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    unsafe { std::env::set_var("GROKHUB_CONFIG", &root) };
    let _pin = crate::config::TestConfigDir::set(root.clone());

    let mut cabin = Cabin::quiet_for_test();
    cabin.cfg.quiet_start = "00:00".into();
    cabin.cfg.quiet_end = "00:00".into();
    cabin.note_automation_done(
        "learn-src",
        "ZephyrLedger snapshot",
        "ZephyrBodyShouldStayOut",
    );
    let id = cabin
        .updates
        .iter()
        .find(|card| card.source_id == "learn-src")
        .map(|card| card.id.clone())
        .expect("run card");
    cabin.open_feed_card(&id);
    cabin.note_automation_done(
        "learn-src-b",
        "Quarterly vendor audit",
        "ZephyrBodyShouldStayOut",
    );
    let other = cabin
        .updates
        .iter()
        .find(|card| card.source_id == "learn-src-b")
        .map(|card| card.id.clone())
        .expect("second run card");
    cabin.dismiss_feed_card(&other);

    let prefs_path = root.join("card_prefs.json");
    let body = std::fs::read_to_string(&prefs_path).unwrap_or_default();
    assert!(
        body.contains("zephyrledger") && body.contains("snapshot"),
        "title keywords landed in card_prefs.json: {body}"
    );
    assert!(!body.contains("ZephyrBodyShouldStayOut"), "{body}");
    let signals_before =
        std::fs::read_to_string(root.join("card_signals.jsonl")).unwrap_or_default();
    assert!(
        signals_before.contains("\"event\":\"opened\""),
        "{signals_before}"
    );
    assert!(
        signals_before.contains("\"event\":\"rejected\""),
        "{signals_before}"
    );
    assert!(!signals_before.contains("ZephyrLedger"), "{signals_before}");

    cabin.reset_home_learned();
    let cleared = std::fs::read_to_string(&prefs_path).unwrap_or_default();
    let parsed: grokhub_core::CardPrefs =
        serde_json::from_str(&cleared).expect("empty prefs still parse");
    assert!(parsed.kinds.is_empty(), "{cleared}");
    assert!(parsed.groups.is_empty(), "{cleared}");
    assert!(parsed.topics.is_empty(), "{cleared}");
    let loaded = grokhub_core::load_card_prefs(&root);
    assert!(loaded.kinds.is_empty() && loaded.groups.is_empty() && loaded.topics.is_empty());
    let signals_after =
        std::fs::read_to_string(root.join("card_signals.jsonl")).unwrap_or_default();
    assert_eq!(signals_before, signals_after);

    let _ = std::fs::remove_dir_all(&root);
    unsafe { std::env::remove_var("GROKHUB_CONFIG") };
}

// Folded from PR #410.
#[test]
fn drop_extra_note_none_for_zero_extras() {
    // Free fn: 0 → None; 1 → singular wording; n>1 → mentions n. Stay off spawn/network.
    assert_eq!(drop_extra_note(0), None);
    let one = drop_extra_note(1).expect("singular");
    assert!(
        one.contains("1 more file"),
        "one extra uses singular wording: {one}"
    );
    let many = drop_extra_note(4).expect("plural");
    assert!(many.contains("4 more file"), "n>1 extras mention n: {many}");
}

// Folded from PR #423.
#[test]
fn composer_hint_ink_blends_subtle_and_muted() {
    let ink = super::composer_hint_ink();
    assert_ne!(ink.a(), 0, "hint ink is not fully transparent: {ink:?}");
    assert_eq!(
        ink,
        crate::theme::blend_color(crate::theme::subtle(), crate::theme::muted(), 0.5),
        "hint ink is the subtle+muted mid blend"
    );
}

#[test]
fn a_wide_empty_composer_names_file_drop_and_the_shortcut_list() {
    let _paint = crate::theme::hold_paint_test();
    crate::theme::set_paint_palette(crate::theme::PaintPalette::OpsDark);
    assert_eq!(
        super::composer_ghost_hint(600.0),
        "Say something...  ·  drop files here  ·  Ctrl+/ for shortcuts"
    );
    assert_eq!(super::composer_ghost_hint(300.0), "Say something...");
    crate::theme::set_paint_palette(crate::theme::PaintPalette::ClassicDark);
    assert_eq!(
        super::composer_ghost_hint(600.0),
        "Ask anything  ·  drop files here  ·  Ctrl+/ for shortcuts"
    );
    assert_eq!(super::composer_ghost_hint(300.0), "Ask anything");
}
