//! The shell's shared-model tiles say what the data says.

use super::*;
use grokhub_core::disk::DiskUse;
use grokhub_core::game_guard::GameGuard;
use grokhub_core::home::TileState;

const NOW: u64 = 1_800_000_000_000;

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
fn disk_scanning_tiles_are_kept_for_their_refresh_time_only() {
    let v = storage_view(&[DiskUse {
        label: "C:".into(),
        total: 1_000,
        free: 600,
    }])
    .at(NOW);
    keep("storage", false, &v);
    assert_eq!(cached("storage", false, NOW + 30_000), Some(v.clone()));
    assert_eq!(
        cached("storage", true, NOW + 30_000),
        None,
        "web asks keep their own"
    );
    assert_eq!(cached("storage", false, NOW + 61_000), None);
    keep("workboard", false, &v);
    assert_eq!(
        cached("workboard", false, NOW),
        None,
        "small reads stay fresh"
    );
}

#[test]
fn frame_rate_reads_the_os_count_and_never_the_game() {
    use grokhub_core::home::text::FpsRead;
    let live = frame_rate_view(Some(FpsRead::Rate(143.6)), false, None);
    assert_eq!(live.text.big, "144 FPS");
    assert_eq!(live.tag.as_deref(), Some("LIVE"));
    assert!(live.buttons.is_empty());
    let win = frame_rate_view(Some(FpsRead::Rate(239.7)), true, Some("cs2.exe"));
    assert_eq!(win.text.caption, "From cs2");
    let none = frame_rate_view(None, false, None);
    assert_eq!(none.text.big, "Not read");
    assert!(none.text.caption.ends_with("never hooks into games"));
    assert_eq!(none.buttons.len(), 1);
    let setup = frame_rate_view(Some(FpsRead::NeedsSetup), true, None);
    assert_eq!(setup.text.caption, "Needs a one-time admin setup");
}
