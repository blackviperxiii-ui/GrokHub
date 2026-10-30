use super::*;

const NONE: &[&str] = &[];

#[test]
fn anticheat_executables_are_matched_in_any_case_and_by_file_name() {
    let cases: &[(&str, &str)] = &[
        ("EasyAntiCheat.exe", "Easy Anti-Cheat"),
        ("EasyAntiCheat_EOS.exe", "Easy Anti-Cheat"),
        ("BEService.exe", "BattlEye"),
        ("BEService_x64.exe", "BattlEye"),
        ("vgc.exe", "Vanguard"),
        ("VGTRAY.EXE", "Vanguard"),
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
        ("vgc", "Vanguard"),
        ("vgk", "Vanguard"),
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
            gaming: true
        }
    );
    let gaming = GameGuard {
        anticheat: None,
        gaming: true,
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
}

#[test]
fn gaming_pauses_desktop_control_and_capture_but_not_recording_or_window_checks() {
    let g = GameGuard {
        anticheat: None,
        gaming: true,
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
