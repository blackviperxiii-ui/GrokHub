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
    let live = frame_rate_view(Some(FpsRead::Rate(143.6)), false, None, false);
    assert_eq!(live.text.big, "144 FPS");
    assert_eq!(live.tag.as_deref(), Some("LIVE"));
    assert!(live.buttons.is_empty());
    let win = frame_rate_view(Some(FpsRead::Rate(239.7)), true, Some("cs2.exe"), false);
    assert_eq!(win.text.caption, "From cs2");
    let none = frame_rate_view(None, false, None, false);
    assert_eq!(none.text.big, "Not read");
    assert!(none.text.caption.ends_with("never hooks into games"));
    assert_eq!(none.buttons.len(), 1);
    let setup = frame_rate_view(Some(FpsRead::NeedsSetup), true, None, false);
    assert_eq!(setup.text.caption, "Needs a one-time admin setup");
}

#[test]
fn frame_rate_setup_is_one_tap_with_the_steps_beside_it() {
    use grokhub_core::home::text::{FpsRead, FpsSetup};
    let run = TileAction::setup("frame_rate", false);
    let copy = TileAction::copy_setup("frame_rate");
    let need = frame_rate_view(Some(FpsRead::NeedsSetup), false, None, false);
    assert_eq!(need.text.caption, "Needs a one-time setup");
    assert_eq!(need.buttons[0].label, "Set up");
    assert!(need.buttons[0].primary);
    assert_eq!(need.buttons[0].action, run);
    assert_eq!(need.buttons[1].action, copy);
    // Small tiles show no buttons, so the tap runs it too.
    assert_eq!(need.tap, Some(run.clone()));
    let closed = frame_rate_view(
        Some(FpsRead::Setup(FpsSetup::Dismissed)),
        false,
        None,
        false,
    );
    assert_eq!(closed.text.caption, "The password prompt was closed");
    assert_eq!(closed.tap, Some(run));
    let no_agent = frame_rate_view(Some(FpsRead::Setup(FpsSetup::NoPrompt)), false, None, false);
    assert_eq!(no_agent.buttons.len(), 1);
    assert_eq!(no_agent.buttons[0].action, copy);
    let sign_out = frame_rate_view(Some(FpsRead::Setup(FpsSetup::SignOut)), true, None, false);
    assert_eq!(sign_out.text.caption, "Sign out of Windows to finish");
    assert!(sign_out.buttons.is_empty());
    let undo = frame_rate_view(Some(FpsRead::Rate(60.0)), false, None, true);
    assert_eq!(undo.buttons[0].label, "Undo setup");
    assert_eq!(
        undo.buttons[0].action,
        TileAction::setup("frame_rate", true)
    );
}

#[test]
fn feed_tiles_read_their_pulse_cards_and_say_where_the_switch_is() {
    let d = std::env::temp_dir().join(format!(
        "grokhub-shell-feed-{}-{}",
        std::process::id(),
        grokhub_core::now_ms()
    ));
    std::fs::create_dir_all(&d).unwrap();
    let off = feed("achievements", &d, NOW);
    assert_eq!(off.state, TileState::Off);
    assert_eq!(off.buttons.len(), 1);
    assert_eq!(off.buttons[0].action, TileAction::nav("pulse"));
    assert!(off.text.detail.as_deref().unwrap().contains("Pulse"));

    std::fs::write(
        d.join("app.json"),
        r#"{"gameCompanion":true,"theme":"dark"}"#,
    )
    .unwrap();
    let mut card = grokhub_core::update_feed::digest_card(
        "source:game-companion:570",
        "Back to Dota 2",
        "",
        6,
    );
    card.created_at = NOW - 60_000;
    crate::pulse::add_post(&d, card).unwrap();
    let on = feed("patch_notes", &d, NOW);
    assert_eq!(on.state, TileState::Ready);
    assert_eq!(on.text.big, "Back to Dota 2");
    assert_eq!(feed("deals", &d, NOW).state, TileState::Off);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_playbooks_row_runs_its_playbook() {
    let page = crate::playbooks::Page {
        rows: vec![crate::playbooks::Row {
            id: "linux_repair".into(),
            name: "Linux repair".into(),
            summary: "Fix a broken package".into(),
            source: "shipped",
            fits: "Fits repair · linux".into(),
            steps: "3 steps, up to Safe".into(),
            text: String::new(),
        }],
        problems: Vec::new(),
    };
    let v = playbooks_view(&page);
    assert_eq!(
        v.rows[0].tap,
        Some(TileAction::RunPlaybook {
            id: "linux_repair".into()
        })
    );
    // With none yet, the tile still opens Playbooks to make one.
    let none = crate::playbooks::Page {
        rows: Vec::new(),
        problems: Vec::new(),
    };
    assert_eq!(
        playbooks_view(&none).buttons[0].action,
        TileAction::settings("playbooks")
    );
}
