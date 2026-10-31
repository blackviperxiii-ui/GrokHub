//! Home tiles say what the app really knows.

use grokhub_agent::route::connectivity::NetState;
use grokhub_core::game_guard::GameGuard;

use super::live::{summarize_sends, LiveFacts};
use super::paint::{
    connection_text, game_mode_text, gpu_temp_text, storage_rows, storage_text, updates_text,
    usage_text, Tone,
};
use crate::app::budget_ui::WeekSpend;
use grokhub_agent::indexers::system_state::DiskUse;

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
    );
    assert_eq!(t.big, "812k");
    assert_eq!(t.caption, "Tokens today · 81% of 1.0M");
    assert_eq!(t.detail.as_deref(), Some("$3.50 of $10 this week"));
    assert_eq!(t.tone, Tone::Warn);

    let over = usage_text(1_200_000, 1_000_000, None, 0);
    assert_eq!(over.tone, Tone::Alert);
    assert_eq!(over.detail, None);

    let none = usage_text(0, 0, None, 0);
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
    );
    assert_eq!(uncapped.detail.as_deref(), Some("$0.42 this week"));
    assert_eq!(uncapped.tone, Tone::Normal);
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
    let rows = storage_rows(&drives);
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
    use super::updates::{OsUpdates, UpdateFacts};
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
}
