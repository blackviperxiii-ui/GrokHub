use super::*;

const NONE: &[&str] = &[];

#[test]
fn anticheat_executables_are_matched_in_any_case_and_by_file_name() {
    let cases: &[(&str, &str)] = &[
        ("EasyAntiCheat.exe", "Easy Anti-Cheat"),
        ("EasyAntiCheat_EOS.exe", "Easy Anti-Cheat"),
        ("BEService.exe", "BattlEye"),
        ("BEService_x64.exe", "BattlEye"),
        ("VALORANT-Win64-Shipping.exe", "Vanguard"),
        ("League of Legends.exe", "Vanguard"),
        ("faceit.exe", "FACEIT"),
        ("FaceitClient.exe", "FACEIT"),
        ("PnkBstrA.exe", "PunkBuster"),
        ("PnkBstrB.exe", "PunkBuster"),
        ("GameMon.exe", "nProtect GameGuard"),
        ("GameMon64.exe", "nProtect GameGuard"),
        ("xxd-0.xem", "XIGNCODE3"),
        ("EAAntiCheat.GameService.exe", "EA Javelin"),
        (
            r"C:\Program Files (x86)\EasyAntiCheat_EOS\EasyAntiCheat_EOS.exe",
            "Easy Anti-Cheat",
        ),
        ("/opt/game/BEService_x64.exe", "BattlEye"),
    ];
    for (exe, want) in cases {
        assert_eq!(detect_anticheat(&[*exe], NONE), Some(*want), "{exe}");
    }
}

#[test]
fn anticheat_services_and_drivers_are_matched_in_any_case() {
    let cases: &[(&str, &str)] = &[
        ("EasyAntiCheat", "Easy Anti-Cheat"),
        ("EasyAntiCheat_EOS", "Easy Anti-Cheat"),
        ("BEService", "BattlEye"),
        ("BEDaisy", "BattlEye"),
        ("FACEIT", "FACEIT"),
        ("ESEADriver2", "ESEA"),
        ("PnkBstrA", "PunkBuster"),
        ("xhunter1", "XIGNCODE3"),
        ("EAAntiCheatService", "EA Javelin"),
    ];
    for (svc, want) in cases {
        assert_eq!(detect_anticheat(NONE, &[*svc]), Some(*want), "{svc}");
    }
}

#[test]
fn vanguard_alone_is_installed_and_idle_not_running() {
    // vgk (driver), vgc (service) and the tray icon run from install on.
    for (exes, services) in [
        (&["vgc.exe"][..], NONE),
        (&["VGTRAY.EXE"][..], NONE),
        (NONE, &["vgk"][..]),
        (NONE, &["VGC"][..]),
        (&["vgc.exe", "vgtray.exe"][..], &["vgk", "vgc"][..]),
    ] {
        let scan = scan_anticheats(exes, services);
        assert_eq!(
            scan,
            AntiCheatScan {
                running: None,
                idle: Some("Vanguard")
            },
            "{exes:?} {services:?}"
        );
        assert_eq!(detect_anticheat(exes, services), None);
    }
    // The Riot Client and League's lobby client are not the protected game.
    let launchers = [
        "RiotClientServices.exe",
        "RiotClientUx.exe",
        "Riot Client.exe",
        "LeagueClient.exe",
        "LeagueClientUx.exe",
    ];
    assert_eq!(
        scan_anticheats(&launchers, &["vgk", "vgc"]),
        AntiCheatScan {
            running: None,
            idle: Some("Vanguard")
        }
    );
    assert_eq!(scan_anticheats(&launchers, NONE), AntiCheatScan::default());
}

#[test]
fn a_riot_game_makes_vanguard_count_as_running() {
    for game in [
        "VALORANT.exe",
        "VALORANT-Win64-Shipping.exe",
        "League of Legends.exe",
        r"C:\Riot Games\League of Legends\Game\League of Legends.exe",
        "2XKO.exe",
        "Lion-Win64-Shipping.exe",
    ] {
        let scan = scan_anticheats(&["vgtray.exe", game], &["vgk", "vgc"]);
        assert_eq!(
            scan,
            AntiCheatScan {
                running: Some("Vanguard"),
                idle: None
            },
            "{game}"
        );
    }
    // The game can't run without Vanguard, so the game alone is enough.
    assert_eq!(
        detect_anticheat(&["League of Legends.exe"], NONE),
        Some("Vanguard")
    );
}

#[test]
fn every_entry_but_vanguard_counts_its_own_names_as_running() {
    for ac in ANTI_CHEATS {
        assert_eq!(ac.always_on.is_some(), ac.name == "Vanguard", "{}", ac.name);
    }
    // BattlEye's service alone still blocks as before, Vanguard idle or not.
    let scan = scan_anticheats(&["explorer.exe"], &["BEService", "vgk"]);
    assert_eq!(
        scan,
        AntiCheatScan {
            running: Some("BattlEye"),
            idle: Some("Vanguard")
        }
    );
    assert_eq!(scan_anticheats(NONE, &["BEService"]).idle, None);
}

#[test]
fn ordinary_processes_and_lookalikes_are_not_an_anticheat() {
    let exes = [
        "explorer.exe",
        "steam.exe",
        "vgc.exe.bak",
        "notvgc.exe",
        "EasyAntiCheat_Setup.exe",
        "faceit-overlay.exe",
    ];
    let services = ["Spooler", "vgcx", "BEServiceHelper", "WinDefend"];
    assert_eq!(detect_anticheat(&exes, &services), None);
    assert_eq!(scan_anticheats(&exes, &services), AntiCheatScan::default());
    // Lookalikes of Riot games are not the game.
    assert_eq!(
        scan_anticheats(
            &["valorant.exe.bak", "League of Legends Setup.exe"],
            &["vgk"]
        )
        .running,
        None
    );
    assert_eq!(detect_anticheat(NONE, NONE), None);
}

#[test]
fn windows_games_are_read_from_exe_names_only() {
    let none: Vec<String> = Vec::new();
    assert_eq!(
        windows_game_exe(
            &["explorer.exe", "FortniteClient-Win64-Shipping.exe"],
            &none
        ),
        Some("fortniteclient-win64-shipping.exe".to_string())
    );
    assert_eq!(
        windows_game_exe(&["RocketLeague-WinGDK-Shipping.exe"], &none),
        Some("rocketleague-wingdk-shipping.exe".to_string())
    );
    // A launcher manifest names the exe.
    let known = vec!["hades.exe".to_string()];
    assert_eq!(
        windows_game_exe(&["steam.exe", "Hades.exe"], &known),
        Some("hades.exe".to_string())
    );
    // Launchers, the shell and a bare suffix are not games.
    assert_eq!(
        windows_game_exe(
            &[
                "steam.exe",
                "EpicGamesLauncher.exe",
                "explorer.exe",
                "-win64-shipping.exe"
            ],
            &known
        ),
        None
    );
    assert_eq!(windows_game_exe::<&str>(&[], &known), None);
    assert_eq!(
        game_label("FortniteClient-Win64-Shipping.exe"),
        "fortniteclient-win64-shipping"
    );
    assert_eq!(game_label(r"C:\Games\Hades.exe"), "hades");
}

#[test]
fn epic_manifests_name_game_exes_but_not_engines() {
    let game = r#"{"DisplayName":"Hades","LaunchExecutable":"x64/Hades.exe","AppCategories":["public","games","applications"]}"#;
    assert_eq!(epic_manifest_exe(game), Some("hades.exe".to_string()));
    let no_cats = r#"{"LaunchExecutable":"Binaries\\Win64\\Game.exe"}"#;
    assert_eq!(epic_manifest_exe(no_cats), Some("game.exe".to_string()));
    let engine = r#"{"LaunchExecutable":"Engine/Binaries/Win64/UnrealEditor.exe","AppCategories":["engines"]}"#;
    assert_eq!(epic_manifest_exe(engine), None);
    assert_eq!(
        epic_manifest_exe(r#"{"LaunchExecutable":"readme.txt"}"#),
        None
    );
    assert_eq!(epic_manifest_exe("not json"), None);
}

#[test]
fn every_gaming_signal_turns_the_guard_on_and_none_leaves_it_clear() {
    assert_eq!(decide(&GuardSignals::default()), GameGuard::default());
    assert!(decide(&GuardSignals::default()).is_clear());
    let on = |s: GuardSignals| decide(&s);
    let ac = on(GuardSignals {
        anticheat: Some("Vanguard"),
        ..Default::default()
    });
    assert_eq!(
        ac,
        GameGuard {
            anticheat: Some("Vanguard"),
            gaming: true,
            idle_anticheat: None,
        }
    );
    let gaming = GameGuard {
        anticheat: None,
        gaming: true,
        idle_anticheat: None,
    };
    assert_eq!(
        on(GuardSignals {
            steam_app: Some(730),
            ..Default::default()
        }),
        gaming
    );
    // Steam writes 0 when no game runs.
    assert!(on(GuardSignals {
        steam_app: Some(0),
        ..Default::default()
    })
    .is_clear());
    assert_eq!(
        on(GuardSignals {
            game_process: true,
            ..Default::default()
        }),
        gaming
    );
    assert_eq!(
        on(GuardSignals {
            listed_game: true,
            ..Default::default()
        }),
        gaming
    );
    assert_eq!(
        on(GuardSignals {
            exclusive_fullscreen: true,
            ..Default::default()
        }),
        gaming
    );
}

#[test]
fn a_clear_guard_blocks_nothing() {
    let g = GameGuard::default();
    for what in [
        Guarded::DesktopControl,
        Guarded::ScreenCapture,
        Guarded::ScreenRecording,
        Guarded::WindowCheck,
    ] {
        assert_eq!(g.blocks(what), None, "{what:?}");
    }
    assert_eq!(g.status_line(), None);
    assert_eq!(g.idle_note(), None);
    assert_eq!(g.panel_note(), None);
}

/// The guard the readers build from these process and service names.
fn guard_from(exes: &[&str], services: &[&str]) -> GameGuard {
    let scan = scan_anticheats(exes, services);
    decide(&GuardSignals {
        anticheat: scan.running,
        idle_anticheat: scan.idle,
        ..Default::default()
    })
}

#[test]
fn vanguard_installed_with_no_riot_game_pauses_nothing_and_says_so() {
    let g = guard_from(&["vgtray.exe"], &["vgk", "vgc"]);
    assert_eq!(
        g,
        GameGuard {
            anticheat: None,
            gaming: false,
            idle_anticheat: Some("Vanguard"),
        }
    );
    assert!(g.is_clear());
    for what in [
        Guarded::DesktopControl,
        Guarded::ScreenCapture,
        Guarded::ScreenRecording,
        Guarded::WindowCheck,
    ] {
        assert_eq!(g.blocks(what), None, "{what:?}");
    }
    assert_eq!(g.status_line(), None);
    let note = "Riot Vanguard is installed. It runs all the time, even with no Riot game open; desktop control pauses only while a Riot game runs.";
    assert_eq!(g.idle_note(), Some(note));
    assert_eq!(g.panel_note().as_deref(), Some(note));
    // A non-Riot game still pauses as gaming, and the note stays under it.
    let steam = decide(&GuardSignals {
        steam_app: Some(730),
        idle_anticheat: Some("Vanguard"),
        ..Default::default()
    });
    assert!(steam.blocks(Guarded::DesktopControl).is_some());
    assert_eq!(
        steam.panel_note(),
        Some(format!("Game mode: desktop control paused\n{note}"))
    );
}

#[test]
fn a_riot_game_with_vanguard_blocks_everything_as_an_anticheat() {
    for (exes, services) in [
        (
            &["vgtray.exe", "VALORANT-Win64-Shipping.exe"][..],
            &["vgk"][..],
        ),
        (&["League of Legends.exe"][..], &["vgc"][..]),
    ] {
        let g = guard_from(exes, services);
        assert_eq!(
            g,
            GameGuard {
                anticheat: Some("Vanguard"),
                gaming: true,
                idle_anticheat: None,
            },
            "{exes:?}"
        );
        assert_eq!(
            g.blocks(Guarded::ScreenRecording).as_deref(),
            Some(
                "Screen recording is blocked while Vanguard is running so it doesn't flag this PC."
            )
        );
        assert_eq!(
            g.panel_note().as_deref(),
            Some("Anti-cheat running (Vanguard): desktop control, screen capture and window checks paused")
        );
    }
}

#[test]
fn battleye_alone_still_blocks_everything() {
    let g = guard_from(&["BEService.exe"], &[]);
    assert_eq!(
        g,
        GameGuard {
            anticheat: Some("BattlEye"),
            gaming: true,
            idle_anticheat: None,
        }
    );
    assert_eq!(
        g.blocks(Guarded::WindowCheck).as_deref(),
        Some("Reading other windows is blocked while BattlEye is running so it doesn't flag this PC.")
    );
    assert_eq!(g.idle_note(), None);
}

#[test]
fn gaming_pauses_desktop_control_and_capture_but_not_recording_or_window_checks() {
    let g = GameGuard {
        anticheat: None,
        gaming: true,
        idle_anticheat: None,
    };
    assert_eq!(
        g.blocks(Guarded::DesktopControl).as_deref(),
        Some("Desktop control is paused while you're gaming so the game's anti-cheat doesn't flag this PC.")
    );
    assert_eq!(
        g.blocks(Guarded::ScreenCapture).as_deref(),
        Some("Screen capture is paused while you're gaming so the game's anti-cheat doesn't flag this PC.")
    );
    assert_eq!(g.blocks(Guarded::ScreenRecording), None);
    assert_eq!(g.blocks(Guarded::WindowCheck), None);
    assert_eq!(
        g.status_line().as_deref(),
        Some("Game mode: desktop control paused")
    );
}

#[test]
fn a_running_anticheat_blocks_everything_and_names_itself() {
    let g = decide(&GuardSignals {
        anticheat: Some("Vanguard"),
        ..Default::default()
    });
    assert_eq!(
        g.blocks(Guarded::DesktopControl).as_deref(),
        Some("Desktop control is blocked while Vanguard is running so it doesn't flag this PC.")
    );
    assert_eq!(
        g.blocks(Guarded::ScreenCapture).as_deref(),
        Some("Screen capture is blocked while Vanguard is running so it doesn't flag this PC.")
    );
    assert_eq!(
        g.blocks(Guarded::ScreenRecording).as_deref(),
        Some("Screen recording is blocked while Vanguard is running so it doesn't flag this PC.")
    );
    assert_eq!(
        g.blocks(Guarded::WindowCheck).as_deref(),
        Some("Reading other windows is blocked while Vanguard is running so it doesn't flag this PC.")
    );
    assert_eq!(
        g.status_line().as_deref(),
        Some("Anti-cheat running (Vanguard): desktop control, screen capture and window checks paused")
    );
}

#[test]
fn input_window_and_screen_tools_are_guarded_and_file_tools_are_not() {
    for t in [
        "screenshot",
        "click",
        "move",
        "drag",
        "scroll",
        "type",
        "key",
        "open_app",
        "focus_window",
        "get_window_geometry",
        "set_window_geometry",
    ] {
        assert!(desktop_tool_guarded(t), "{t}");
    }
    for t in [
        "list_monitors",
        "delete_files",
        "watch_path",
        "watch_events",
        "unwatch_path",
        "nope",
    ] {
        assert!(!desktop_tool_guarded(t), "{t}");
    }
}
