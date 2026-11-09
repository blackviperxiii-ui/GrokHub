//! Home tiles say what the app really knows.

use grokhub_agent::route::connectivity::NetState;
use grokhub_core::game_guard::GameGuard;

use super::live::{summarize_sends, LiveFacts};
use super::paint::{frame_rate_text, speeds_text};
use crate::app::budget_ui::WeekSpend;
use grokhub_agent::indexers::system_state::DiskUse;
use grokhub_core::home::text::{
    game_mode_text, gpu_temp_text, storage_rows, storage_text, updates_text,
};
use grokhub_core::home::{TileText, Tone};

/// The usage tile as the desktop app calls it, with the real cache floor.
fn usage_text(
    used: u64,
    cap: u64,
    week: Option<WeekSpend>,
    week_cap_usd: u32,
    cache_pct: Option<u8>,
) -> TileText {
    grokhub_core::home::text::usage_text(
        used,
        cap,
        week,
        week_cap_usd,
        cache_pct,
        grokhub_agent::cache_watch::FLOOR_PCT,
    )
}

/// The Connection tile as the desktop app calls it.
fn connection_text(net: NetState, local_on: bool, facts: &LiveFacts) -> TileText {
    grokhub_core::home::text::connection_text(net.link(), local_on, &facts.sends())
}

#[test]
fn usage_tile_shows_tokens_against_the_cap_and_week_spend() {
    let t = usage_text(
        812_000,
        1_000_000,
        Some(WeekSpend {
            usd: 3.5,
            pct: Some(35),
        }),
        10,
        None,
    );
    assert_eq!(t.big, "812k");
    assert_eq!(t.caption, "Tokens today · 81% of 1.0M");
    assert_eq!(t.detail.as_deref(), Some("$3.50 of $10 this week"));
    assert_eq!(t.tone, Tone::Warn);

    let over = usage_text(1_200_000, 1_000_000, None, 0, None);
    assert_eq!(over.tone, Tone::Alert);
    assert_eq!(over.detail, None);

    let none = usage_text(0, 0, None, 0, None);
    assert_eq!(none.big, "0");
    assert_eq!(none.caption, "Tokens today · no daily cap");
    assert_eq!(none.tone, Tone::Quiet);

    let uncapped = usage_text(
        4_200,
        0,
        Some(WeekSpend {
            usd: 0.42,
            pct: None,
        }),
        0,
        None,
    );
    assert_eq!(uncapped.detail.as_deref(), Some("$0.42 this week"));
    assert_eq!(uncapped.tone, Tone::Normal);
}

#[test]
fn usage_tile_adds_the_cache_share_and_flags_a_low_one() {
    let week = Some(WeekSpend {
        usd: 3.5,
        pct: None,
    });
    let good = usage_text(1_000, 0, week, 0, Some(91));
    assert_eq!(good.detail.as_deref(), Some("$3.50 this week · cache 91%"));
    let low = usage_text(1_000, 0, week, 0, Some(42));
    assert_eq!(
        low.detail.as_deref(),
        Some("$3.50 this week · cache 42% (low)")
    );
    let alone = usage_text(1_000, 0, None, 0, Some(88));
    assert_eq!(alone.detail.as_deref(), Some("cache 88%"));
}

#[test]
fn game_mode_tile_follows_the_guard() {
    let idle = game_mode_text(GameGuard::default(), None, false);
    assert_eq!(idle.big, "No game running");
    assert_eq!(idle.tone, Tone::Quiet);

    let playing = GameGuard {
        gaming: true,
        ..GameGuard::default()
    };
    let t = game_mode_text(playing, Some("Deep Rock Galactic"), true);
    assert_eq!(t.big, "Deep Rock Galactic");
    assert_eq!(t.caption, "Desktop control paused while you play");
    assert_eq!(
        t.detail.as_deref(),
        Some("Local AI paused to keep the GPU free")
    );
    assert_eq!(t.tone, Tone::Warn);

    let ac = GameGuard {
        anticheat: Some("Vanguard"),
        gaming: true,
        idle_anticheat: None,
    };
    let t = game_mode_text(ac, None, false);
    assert_eq!(t.big, "Game running");
    assert_eq!(t.caption, "Vanguard active · Loadout stays hands-off");

    let idle_ac = GameGuard {
        anticheat: None,
        gaming: false,
        idle_anticheat: Some("Vanguard"),
    };
    assert_eq!(
        game_mode_text(idle_ac, None, false).caption,
        "Vanguard installed · idle"
    );
}

fn line(ts_ms: u64, dest: &str) -> grokhub_agent::harness::EgressLine {
    serde_json::from_value(serde_json::json!({
        "ts_ms": ts_ms,
        "dest": dest,
        "data_classes": [],
    }))
    .expect("egress line")
}

#[test]
fn connection_tile_counts_the_last_days_sends() {
    let now = 10 * 86_400_000;
    let read = grokhub_agent::harness::EgressRead {
        lines: vec![
            line(now - 2 * 86_400_000, "old.example"),
            line(now - 3_600_000, "api.x.ai"),
            line(now - 60_000, "api.x.ai"),
            line(now - 1_000, "hub"),
        ],
        locked: None,
        unreadable: 0,
    };
    let facts = summarize_sends(&read, now);
    assert_eq!(facts.sends_24h, Some(3));
    assert_eq!(facts.top_dest.as_deref(), Some("api.x.ai"));

    let t = connection_text(
        NetState::Online,
        true,
        &LiveFacts {
            local_ready: true,
            ..facts
        },
    );
    assert_eq!(t.big, "3 sends");
    assert_eq!(t.caption, "Online · local AI ready");
    assert_eq!(t.detail.as_deref(), Some("Mostly to api.x.ai · last 24 h"));
    assert_eq!(t.tone, Tone::Normal);

    let off = connection_text(NetState::Offline, false, &LiveFacts::default());
    assert_eq!(off.big, "—");
    assert_eq!(off.caption, "Offline · local AI off");
    assert_eq!(off.tone, Tone::Alert);
}

#[test]
fn gpu_temp_tile_reads_degrees_and_falls_back_honestly() {
    use grokhub_core::local_profile::Thermal;
    let t = gpu_temp_text(Some(62), Some(Thermal::Unknown));
    assert_eq!(
        (t.big.as_str(), t.caption.as_str()),
        ("62 °C", "Main GPU · cool")
    );
    assert_eq!(gpu_temp_text(Some(78), None).tone, Tone::Warn);
    assert_eq!(gpu_temp_text(Some(91), None).caption, "Main GPU · hot");
    let t = gpu_temp_text(None, Some(Thermal::Fair));
    assert_eq!(
        (t.big.as_str(), t.caption.as_str()),
        ("Warm", "PC sensors · no GPU sensor")
    );
    let t = gpu_temp_text(None, None);
    assert_eq!((t.big.as_str(), t.tone), ("—", Tone::Quiet));
}

#[test]
fn storage_tile_shows_the_fullest_drive_and_lists_the_rest() {
    let d = |label: &str, total: u64, free: u64| DiskUse {
        label: label.into(),
        total,
        free,
    };
    let drives = [
        d("/", 1_000_000_000_000, 400_000_000_000),
        d("Games", 2_000_000_000_000, 60_000_000_000),
    ];
    let t = storage_text(&drives, Some(1));
    assert_eq!(t.big, "60 GB free");
    assert_eq!(t.caption, "Games · 97% used of 2.0 TB");
    assert_eq!(t.detail.as_deref(), Some("2 drives · fullest shown"));
    assert_eq!(t.tone, Tone::Alert);
    let rows: Vec<_> = storage_rows(&drives)
        .into_iter()
        .map(|r| (r.label, r.value, r.tone))
        .collect();
    assert_eq!(
        rows,
        [
            (
                "Games".to_string(),
                "60 GB free of 2.0 TB".to_string(),
                Tone::Alert
            ),
            (
                "/".to_string(),
                "400 GB free of 1.0 TB".to_string(),
                Tone::Normal
            ),
        ]
    );
    let early = storage_text(&[], Some(102_400));
    assert_eq!(
        (early.big.as_str(), early.caption.as_str()),
        ("107 GB free", "Home drive")
    );
    assert_eq!(storage_text(&[], None).tone, Tone::Quiet);
}

#[test]
fn updates_tile_names_what_is_waiting() {
    use grokhub_core::home::text::{OsUpdates, UpdateFacts};
    let none = UpdateFacts {
        os: OsUpdates::NotAllowed,
        steam: vec![],
    };
    let t = updates_text(false, &none, Some("amdgpu (kernel)"));
    assert_eq!(t.big, "Up to date");
    assert_eq!(t.caption, "Loadout and Steam checked · system checks off");
    assert_eq!(t.detail.as_deref(), Some("GPU driver amdgpu (kernel)"));
    let busy = UpdateFacts {
        os: OsUpdates::Pending(12),
        steam: vec!["Apex Legends".into(), "Counter-Strike 2".into()],
    };
    let t = updates_text(true, &busy, None);
    assert_eq!(t.big, "3 waiting");
    assert_eq!(t.caption, "Loadout · 2 Steam games · 12 system packages");
    assert_eq!(t.tone, Tone::Warn);
    let one = UpdateFacts {
        os: OsUpdates::Pending(0),
        steam: vec!["Apex Legends".into()],
    };
    assert_eq!(updates_text(false, &one, None).caption, "Apex Legends");
    let windows = UpdateFacts {
        os: OsUpdates::NoChecker,
        steam: vec![],
    };
    assert_eq!(
        updates_text(false, &windows, None).caption,
        "Loadout and Steam checked"
    );
}

#[test]
fn speeds_tile_reads_the_internet_log_and_sends_nothing_when_off() {
    use crate::app::net_log_ui::{jitter_ms, NetTileFacts};
    assert_eq!(jitter_ms(&[20, 30, 25, 25]), Some(5));
    assert_eq!(jitter_ms(&[20]), None);
    let off = NetTileFacts {
        on: false,
        down: false,
        ping_ms: None,
        jitter_ms: None,
        last_speed: None,
        testing: false,
        note: String::new(),
    };
    let t = speeds_text(&off, 0);
    assert_eq!((t.big.as_str(), t.tone), ("Off", Tone::Quiet));
    let now = 10 * 60_000;
    let on = NetTileFacts {
        on: true,
        ping_ms: Some(24),
        jitter_ms: Some(3),
        last_speed: Some((412.5, now - 3 * 60_000)),
        ..off.clone()
    };
    let t = speeds_text(&on, now);
    assert_eq!(t.big, "24 ms");
    assert_eq!(t.caption, "Ping · jitter 3 ms");
    assert_eq!(t.detail.as_deref(), Some("Down 412.5 Mbps · 3m ago"));
    let down = NetTileFacts {
        down: true,
        ..on.clone()
    };
    assert_eq!(speeds_text(&down, now).tone, Tone::Alert);
}

#[test]
fn frame_rate_tile_is_honest_about_not_reading_games() {
    use super::fps::FpsState;
    use grokhub_agent::indexers::display_fps::FpsBlock;
    let t = frame_rate_text(None, false, None);
    assert_eq!(t.big, "Not read");
    assert_eq!(t.caption, "Loadout never hooks into games");
    assert_eq!(t.tone, Tone::Quiet);
    let t = frame_rate_text(Some(FpsState::Rate(143.6)), false, None);
    assert_eq!(
        (t.big.as_str(), t.caption.as_str()),
        ("144 FPS", "Frames on screen")
    );
    let t = frame_rate_text(
        Some(FpsState::Blocked(FpsBlock::NeedsPermission)),
        false,
        None,
    );
    assert_eq!(t.caption, "Needs a one-time setup");
    assert_eq!(
        t.detail.as_deref(),
        Some("Set up asks for your password once")
    );
    let t = frame_rate_text(Some(FpsState::Blocked(FpsBlock::NoTracepoint)), false, None);
    assert_eq!(t.caption, "This driver doesn't report screen frames");
}

#[test]
fn frame_rate_tile_on_windows_names_the_app_it_counts() {
    use super::fps::FpsState;
    use grokhub_agent::indexers::display_fps::FpsBlock;
    let t = frame_rate_text(Some(FpsState::Rate(239.7)), true, Some("cs2.exe"));
    assert_eq!(
        (t.big.as_str(), t.caption.as_str()),
        ("240 FPS", "From cs2")
    );
    assert_eq!(
        t.detail.as_deref(),
        Some("Counted by Windows, never inside the game")
    );
    let t = frame_rate_text(Some(FpsState::Rate(0.0)), true, None);
    assert_eq!(t.caption, "Nothing is drawing frames");
    assert_eq!(t.tone, Tone::Quiet);
    let t = frame_rate_text(
        Some(FpsState::Blocked(FpsBlock::NeedsPermission)),
        true,
        None,
    );
    assert_eq!(t.caption, "Needs a one-time admin setup");
    let t = frame_rate_text(Some(FpsState::Blocked(FpsBlock::NoTracepoint)), true, None);
    assert_eq!(t.caption, "Windows didn't start the frame trace");
}

#[test]
fn every_home_module_has_a_painter_or_a_real_tile() {
    use super::paint::LIVE_MODULES;
    use super::views::{
        automations_view, daily_drop_view, decisions_view, journal_view, local_ai_view,
        personality_view, playbooks_view, recent_chats_view,
    };
    use grokhub_core::home::{lookup, tiles, DataStatus, TileState, REGISTRY};
    // Every painter is a real registry module, and none is flagged New.
    for id in LIVE_MODULES {
        let m = lookup(id).unwrap_or_else(|| panic!("{id} painted but not in the registry"));
        assert_ne!(m.data, DataStatus::New, "{id} is painted but flagged New");
    }
    // The rest draw from the shared model; with nothing to show, each says
    // what it will show and gives one way forward. No placeholder is left.
    let now = 1_800_000_000_000;
    let empties = [
        recent_chats_view(&[], now),
        local_ai_view(false, false, false),
        local_ai_view(false, true, false),
        playbooks_view(std::iter::empty(), 0),
        automations_view(&[], now),
        journal_view(&[]),
        daily_drop_view(None),
        tiles::now_playing_view(&[], now, None),
        tiles::game_library_view(&[]),
        tiles::what_to_play_view(&[], now, true, None),
        tiles::clips_view(&[], true, now),
        tiles::session_timer_view(None, None, now),
    ];
    for v in &empties {
        assert!(
            matches!(v.state, TileState::Empty | TileState::Off),
            "{:?}",
            v.text
        );
        assert_eq!(v.buttons.len(), 1, "{:?}", v.text);
        assert!(!v.text.caption.is_empty());
    }
    assert_eq!(decisions_view(&[]).text.big, "All clear");
    assert_eq!(personality_view("").text.caption, "Your companion's voice");
    let painted = REGISTRY
        .iter()
        .filter(|m| !LIVE_MODULES.contains(&m.id))
        .count();
    assert_eq!(painted, 23, "every other module goes through views.rs");
}

#[test]
fn recent_chats_lead_with_the_latest_and_open_it() {
    use super::views::recent_chats_view;
    use grokhub_core::home::TileAction;
    let mk = |id: &str, title: &str, at: u64| {
        let mut t = crate::threads::ChatThread::new(title, false);
        t.id = id.into();
        t.last_active_ms = at;
        t.messages = std::sync::Arc::new(vec![("user".into(), "hi".into())]);
        t
    };
    let now = 1_800_000_000_000;
    let threads = [
        mk("a", "Old", now - 86_400_000),
        mk("b", "Fresh", now - 60_000),
    ];
    let v = recent_chats_view(&threads, now);
    assert_eq!(v.text.big, "Fresh");
    assert_eq!(v.rows.len(), 2);
    assert_eq!(v.tap, Some(TileAction::OpenChat { id: "b".into() }));
}

#[test]
fn home_redraws_at_its_fastest_tile_but_never_faster_than_five_seconds() {
    use super::page::repaint_every;
    use std::time::Duration;
    assert_eq!(
        repaint_every(["pulse", "storage", "game_mode"]),
        Some(Duration::from_secs(10))
    );
    assert_eq!(repaint_every(["frame_rate"]), Some(Duration::from_secs(5)));
    assert_eq!(repaint_every(std::iter::empty()), None);
}

#[test]
fn feed_tiles_list_only_their_own_live_cards() {
    use grokhub_core::home::tiles::feed_items;
    use grokhub_core::update_feed::digest_card;
    let mut ach = digest_card(
        "source:steam-achievements:620:48",
        "Portal 2: 2 to go",
        "",
        5,
    );
    ach.pulse.source_name = Some(grokhub_core::steam_achievements::SOURCE_NAME.into());
    ach.citations = vec!["https://steamcommunity.com/app/620/guides/".into()];
    let mut gone = ach.clone();
    gone.source_id = "source:steam-achievements:620:47".into();
    gone.status = grokhub_core::UpdateStatus::Dismissed;
    let news = digest_card("source:game-companion:570", "Back to Dota 2", "", 6);
    let cards = [ach, gone, news];
    let a = feed_items(&cards, "achievements");
    assert_eq!(a.len(), 1, "a dismissed card stays off Home");
    assert_eq!(a[0].title, "Portal 2: 2 to go");
    assert_eq!(feed_items(&cards, "patch_notes")[0].title, "Back to Dota 2");
    assert!(feed_items(&cards, "deals").is_empty());
}

#[test]
fn tonight_lists_only_automations_that_are_on_and_scheduled() {
    let auto =
        |name: &str, enabled: bool, next: Option<u64>| -> grokhub_core::automation::Automation {
            serde_json::from_value(serde_json::json!({
                "name": name,
                "schedule": "daily",
                "time": "20:00",
                "instructions": "check",
                "enabled": enabled,
                "nextRun": next,
            }))
            .unwrap()
        };
    let due = super::views::tonight_due(&[
        auto("Raid reminder", true, Some(1_000)),
        auto("Paused", false, Some(2_000)),
        auto("Not set", true, None),
    ]);
    assert_eq!(due, vec![("Raid reminder".to_string(), 1_000)]);
}

#[test]
fn turn_on_switches_a_plain_feature_in_place_and_leaves_the_rest_to_settings() {
    let root = crate::config::test_config_root("home-turn-on");
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::create_dir_all(&root);
    let _pin = crate::config::TestConfigDir::set(root.clone());
    let mut cabin = crate::app::Cabin::quiet_for_test();
    cabin.cfg.net_log = false;
    cabin.cfg.game_companion = false;
    assert!(cabin.turn_on_from_home("net_log"));
    assert!(cabin.cfg.net_log);
    assert!(cabin.turn_on_from_home("game_companion"));
    assert!(cabin.cfg.game_companion);
    assert!(
        !cabin.turn_on_from_home("local_ai"),
        "a model needs its own setup"
    );
    assert!(!cabin.turn_on_from_home("steam_wishlist"));
    let _ = std::fs::remove_dir_all(&root);
}

/// Every module, painted at every size it offers on the Streamer grid (all
/// seven slots filled) at a wide and a narrow window: each one draws words,
/// none says "soon", and no line runs out past its tile's sides.
#[test]
fn every_module_paints_inside_its_tile_at_every_size() {
    use crate::app::Cabin;
    use eframe::egui;
    use grokhub_core::home::{place_slots, HomeLayout, HomePreset, ModuleSize, REGISTRY};

    let root = crate::config::test_config_root("home-paint");
    let _ = std::fs::remove_dir_all(&root);
    let _pin = crate::config::TestConfigDir::set(root.clone());
    let mut cabin = Cabin::quiet_for_test();
    let ctx = egui::Context::default();
    crate::theme::install_fonts_on(&ctx);
    let grid = HomeLayout::preset(HomePreset::Streamer);

    fn texts(shape: &egui::Shape, out: &mut Vec<(String, egui::Rect)>) {
        match shape {
            egui::Shape::Text(t) => out.push((t.galley.job.text.clone(), t.visual_bounding_rect())),
            egui::Shape::Vec(v) => v.iter().for_each(|s| texts(s, out)),
            _ => {}
        }
    }

    let mut painted = 0;
    for width in [1600.0, 900.0] {
        let slots = place_slots((0.0, 0.0, width, 470.0), &grid);
        for size in [ModuleSize::Large, ModuleSize::Medium, ModuleSize::Small] {
            let Some(slot) = slots.iter().find(|s| s.size == size) else {
                continue;
            };
            let rect =
                egui::Rect::from_min_size(egui::pos2(20.0, 20.0), egui::vec2(slot.w, slot.h));
            for m in REGISTRY.iter().filter(|m| m.supports(size)) {
                let raw = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(width + 40.0, 600.0),
                    )),
                    ..Default::default()
                };
                let out = crate::theme::test_pass(&ctx, raw, |ui| {
                    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect));
                    child.set_clip_rect(rect);
                    child.set_min_size(rect.size());
                    let _ = super::paint::paint_module(&mut cabin, &mut child, m.id, size);
                });
                let mut found = Vec::new();
                out.shapes.iter().for_each(|c| texts(&c.shape, &mut found));
                let words: Vec<&str> = found
                    .iter()
                    .map(|(t, _)| t.as_str())
                    .filter(|t| t.chars().any(char::is_alphanumeric))
                    .collect();
                let at = format!("{} at {} in a {width} window", m.id, size.label());
                assert!(!words.is_empty(), "{at} drew no words");
                assert!(
                    !words.iter().any(|t| *t == "SOON" || *t == "Coming soon"),
                    "{at} says soon: {words:?}"
                );
                for (t, r) in found
                    .iter()
                    .filter(|(t, _)| t.chars().any(char::is_alphanumeric))
                {
                    assert!(
                        r.min.x >= rect.min.x - 1.0 && r.max.x <= rect.max.x + 1.0,
                        "{at}: {t:?} runs out of the tile ({r:?} in {rect:?})"
                    );
                }
                painted += 1;
            }
        }
    }
    assert!(painted > REGISTRY.len(), "{painted}");
    let _ = std::fs::remove_dir_all(&root);
}
