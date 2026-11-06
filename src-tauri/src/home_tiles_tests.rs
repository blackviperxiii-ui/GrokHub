//! The shell's shared-model tiles say what the data says.

use super::*;
use grokhub_core::disk::DiskUse;
use grokhub_core::game_guard::GameGuard;
use grokhub_core::home::TileState;

const NOW: u64 = 1_800_000_000_000;

fn game(
    app_id: u32,
    name: &str,
    installed: bool,
    last_played: u64,
    playtime_min: u64,
) -> library::Game {
    library::Game {
        app_id,
        name: name.into(),
        installed,
        last_played,
        playtime_min,
    }
}

#[test]
fn storage_lists_the_fullest_drive_first_with_a_free_up_button() {
    let drives = [
        DiskUse {
            label: "/".into(),
            total: 1_000_000_000_000,
            free: 400_000_000_000,
        },
        DiskUse {
            label: "Games".into(),
            total: 2_000_000_000_000,
            free: 60_000_000_000,
        },
    ];
    let v = storage_view(&drives);
    assert_eq!(v.text.big, "60 GB free");
    assert_eq!(v.rows[0].label, "Games");
    assert_eq!(v.buttons[0].action, TileAction::ask(FREE_STORAGE_ASK));
    assert_eq!(storage_view(&[]).state, TileState::Error);
}

#[test]
fn speeds_is_off_until_the_internet_log_is_on() {
    let mut log = grokhub_core::net_log::NetLog::default();
    let off = speeds_view(&log, NOW);
    assert_eq!(off.state, TileState::Off);
    log.on = true;
    let quiet = speeds_view(&log, NOW);
    assert_eq!(quiet.text.big, "Online");
    assert_eq!(quiet.text.detail.as_deref(), Some("No drops this week"));
    log.speed.push(grokhub_core::net_log::SpeedTest {
        at_ms: NOW - 120_000,
        mbps: 512.4,
    });
    let tested = speeds_view(&log, NOW);
    assert_eq!(tested.text.big, "512 Mbps");
    assert_eq!(tested.text.caption, "Down · 2m ago");
    assert_eq!(tested.updated_ms, Some(NOW - 120_000));
}

#[test]
fn now_playing_leads_with_the_last_game_played() {
    let secs = NOW / 1000;
    let games = [
        game(10, "Old", true, secs - 30 * 86_400, 600),
        game(20, "Fresh", true, secs - 3_600, 90),
        game(30, "Never", true, 0, 0),
    ];
    let v = now_playing_view(&games, NOW);
    assert_eq!(v.text.big, "Fresh");
    assert_eq!(v.text.caption, "Last played 1h ago · 1.5 h total");
    assert_eq!(v.rows.len(), 2, "a never-played game isn't now playing");
    assert_eq!(v.rows[0].tap, Some(TileAction::Play { app_id: 20 }));
    assert_eq!(now_playing_view(&[], NOW).state, TileState::Empty);
}

#[test]
fn what_to_play_holds_one_pick_all_day_and_plays_only_on_the_pc() {
    let secs = NOW / 1000;
    let games = [
        game(1, "A", true, secs - 10 * 86_400, 60),
        game(2, "B", true, secs - 20 * 86_400, 60),
        game(3, "Today", true, secs - 60, 60),
        game(4, "Updating", false, secs - 30 * 86_400, 60),
    ];
    let a = what_to_play_view(&games, NOW, true);
    let b = what_to_play_view(&games, NOW + 3_600_000, true);
    assert_eq!(a.text.big, b.text.big, "the pick holds through the day");
    assert!(["A", "B"].contains(&a.text.big.as_str()), "{}", a.text.big);
    assert!(matches!(a.buttons[0].action, TileAction::Play { .. }));
    assert!(what_to_play_view(&games, NOW, false).buttons.is_empty());
}

#[test]
fn game_mode_tags_on_while_a_game_runs() {
    let idle = game_mode_view(GameGuard::default(), false);
    assert_eq!(idle.tag.as_deref(), Some("OFF"));
    let playing = GameGuard {
        gaming: true,
        ..GameGuard::default()
    };
    assert_eq!(game_mode_view(playing, true).tag.as_deref(), Some("ON"));
}

#[test]
fn connection_and_usage_read_like_the_desktop_app() {
    let facts = SendFacts {
        sends_24h: Some(3),
        top_dest: Some("api.x.ai".into()),
        local_ready: true,
    };
    let v = connection_from(NetLink::Online, true, &facts);
    assert_eq!(v.text.big, "3 sends");
    assert_eq!(v.text.caption, "Online · local AI ready");
    let u = usage_from(3.5, Some(42), 80);
    assert_eq!(u.text.big, "$3.50");
    assert_eq!(u.text.detail.as_deref(), Some("Prompt cache 42% (low)"));
    assert_eq!(usage_from(0.0, None, 80).text.tone, Tone::Quiet);
}

#[test]
fn coach_tip_puts_a_full_drive_first() {
    let full = [DiskUse {
        label: "C:".into(),
        total: 1_000,
        free: 20,
    }];
    let v = coach_tip_view(&full, GameGuard::default());
    assert_eq!(v.text.big, "Free some space");
    let ok = [DiskUse {
        label: "C:".into(),
        total: 1_000,
        free: 600,
    }];
    let ac = GameGuard {
        anticheat: Some("Vanguard"),
        gaming: true,
        idle_anticheat: None,
    };
    assert_eq!(coach_tip_view(&ok, ac).text.big, "Hands off");
}
