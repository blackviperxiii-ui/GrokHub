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
    assert_eq!(tested.rows.len(), 1, "no ping row before a probe");
}

#[test]
fn speeds_shows_ping_and_jitter_from_the_probe() {
    let mut log = grokhub_core::net_log::NetLog {
        on: true,
        ..Default::default()
    };
    for (i, ms) in [20u32, 30, 25].into_iter().enumerate() {
        log.note(Some(ms), NOW - (3 - i as u64) * 60_000, 0);
    }
    let v = speeds_view(&log, NOW);
    let row = |label: &str| {
        v.rows
            .iter()
            .find(|r| r.label == label)
            .map(|r| r.value.clone())
    };
    assert_eq!(row("Ping").as_deref(), Some("25 ms"));
    assert_eq!(row("Jitter").as_deref(), Some("8 ms"));
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
fn feed_tiles_read_their_pulse_cards_and_turn_on_in_place() {
    let d = std::env::temp_dir().join(format!(
        "grokhub-shell-feed-{}-{}",
        std::process::id(),
        grokhub_core::now_ms()
    ));
    std::fs::create_dir_all(&d).unwrap();
    // Achievements and Deals turn on right here, no other app needed.
    let off = feed("achievements", &d, NOW);
    assert_eq!(off.state, TileState::Off);
    assert_eq!(off.buttons.len(), 1);
    assert_eq!(off.buttons[0].label, "Turn on");
    assert_eq!(
        off.buttons[0].action,
        TileAction::setup("achievements", false)
    );
    assert_eq!(off.tap, Some(TileAction::setup("achievements", false)));
    // Patch Notes turns on here too, with the launch companion's own switch.
    let companion = feed("patch_notes", &d, NOW);
    assert_eq!(companion.buttons[0].label, "Turn on");
    assert_eq!(
        companion.buttons[0].action,
        TileAction::setup("patch_notes", false)
    );
    crate::game_companion::set(&d, true).unwrap();
    let ours = feed("patch_notes", &d, NOW);
    assert_ne!(ours.state, TileState::Off);
    assert!(ours
        .buttons
        .iter()
        .any(|b| b.action == TileAction::setup("patch_notes", true)));
    crate::game_companion::set(&d, false).unwrap();

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
    // Turned on in the desktop app: that app's to turn off, not this one's.
    assert!(!on
        .buttons
        .iter()
        .any(|b| b.action == TileAction::setup("patch_notes", true)));
    assert_eq!(feed("deals", &d, NOW).state, TileState::Off);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn pc_loadout_shows_temps_and_what_starts_with_the_pc() {
    let m = local_ai::Machine {
        gpus: Vec::new(),
        reader: local_ai::GpuReader::None,
        unified: false,
        ram_mb: Some(32 * 1024),
        free_disk_mb: None,
        simd: grokhub_core::local_picker::CpuSimd::Avx2,
        llama_build: None,
    };
    let extras = PcExtras {
        cpu_c: Some(55),
        gpu_c: Some(86),
        startup: Some(vec!["Discord".into(), "Steam".into(), "Spotify".into()]),
    };
    let v = pc_view(&m, &extras);
    let row = |label: &str| v.rows.iter().find(|r| r.label == label).cloned();
    let gpu = row("GPU temp").unwrap();
    assert_eq!(gpu.value, "86°C");
    // Past the hot line: the row says so.
    assert_eq!(gpu.tone, Tone::Warn);
    assert_eq!(row("CPU temp").unwrap().tone, Tone::Normal);
    assert_eq!(row("Startup apps").unwrap().value, "3 apps");
    // Nothing read: no rows invented.
    let bare = pc_view(&m, &PcExtras::default());
    assert!(bare.rows.iter().all(|r| !r.label.contains("temp")));
    assert!(bare.rows.iter().all(|r| r.label != "Startup apps"));
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

#[test]
fn an_on_feed_tile_says_what_its_check_found_and_can_turn_off() {
    let on = tiles::feed_view("deals", true, &[], NOW);
    let v = feed_turn(
        "deals",
        on,
        Some("Checked 4 wishlisted games. No new sales.".into()),
        true,
    );
    assert_eq!(
        v.text.detail.as_deref(),
        Some("Checked 4 wishlisted games. No new sales.")
    );
    assert!(v
        .buttons
        .iter()
        .any(|b| b.label == "Turn off" && b.action == TileAction::setup("deals", true)));
    // A turn-on that couldn't find an account says why on the Off tile.
    let off = tiles::feed_view("achievements", false, &[], NOW);
    let v = feed_turn(
        "achievements",
        off,
        Some("Sign in to Steam on this PC once, then tap Turn on again.".into()),
        true,
    );
    assert_eq!(v.buttons[0].label, "Turn on");
    assert!(v
        .text
        .detail
        .as_deref()
        .unwrap()
        .starts_with("Sign in to Steam"));
}

#[test]
fn updates_opens_its_page_and_shows_what_the_page_last_found() {
    use grokhub_core::home::updates::{Found, Sources, UpdateItem};
    let fresh = updates_view("3.5.0", None);
    assert_eq!(fresh.text.caption, "Tap to see what's waiting to update");
    assert_eq!(fresh.tap, Some(TileAction::nav("updates")));
    assert_eq!(fresh.buttons[0].action, TileAction::nav("updates"));
    let read = Sources {
        version: "3.5.0".into(),
        steam: Some(vec!["Hades II".into()]),
        system: Some(Found::items(vec![
            UpdateItem::named("linux"),
            UpdateItem::named("mesa"),
        ])),
        gpu_driver: Some("560.35".into()),
        ..Sources::default()
    };
    let v = updates_view("3.5.0", Some(&read));
    assert_eq!(v.text.big, "2 waiting");
    assert_eq!(v.text.caption, "Hades II · 2 system packages");
    assert_eq!(v.text.detail.as_deref(), Some("GPU driver 560.35"));
}

#[test]
fn the_journal_tile_opens_a_mission_chat_only_for_a_viewer_who_sees_it() {
    let question = |mission: Option<&str>| journal::Question {
        id: "q1".into(),
        question: "Which router?".into(),
        status: "Investigating".into(),
        active: true,
        options: Vec::new(),
        leaning: None,
        chosen: None,
        mission: mission.map(str::to_string),
    };
    let page = journal::Page {
        active: vec![question(Some("shell-desktop-1")), question(None)],
        history: Vec::new(),
    };
    let open = Some(TileAction::OpenChat {
        id: "shell-desktop-1".into(),
    });

    // A paired browser that didn't start the chat gets no way into it, and
    // no Journal page to fall back to.
    let stranger = journal_view(&page, &Viewer::Web(&|_| false));
    assert_eq!(stranger.rows[0].label, "Which router?");
    assert_eq!(stranger.rows[0].tap, None);
    assert_eq!(stranger.rows[1].tap, None);

    let owner = journal_view(&page, &Viewer::Web(&|id| id == "shell-desktop-1"));
    assert_eq!(owner.rows[0].tap, open);

    let desktop = journal_view(&page, &Viewer::Desktop);
    assert_eq!(desktop.rows[0].tap, open);
    assert_eq!(desktop.rows[1].tap, Some(TileAction::nav("journal")));
}
