use super::*;
use crate::local_profile::{PowerInfo, Thermal};

const AC: PowerInfo = PowerInfo {
    battery: false,
    on_ac: true,
    percent: None,
    power_saver: false,
};

/// Feeds `n` samples of `s`, `step` ms apart after `t0`; returns each change
/// as (sample number from 1, new level).
fn feed(
    g: &mut Governor,
    s: &Signals,
    rules: &Rules,
    n: u32,
    t0: u64,
    step: u64,
) -> Vec<(u32, Level)> {
    (1..=n)
        .filter_map(|i| {
            g.observe(target(s, rules).map(|w| w.level), t0 + u64::from(i) * step)
                .map(|l| (i, l))
        })
        .collect()
}

fn desk(f: impl FnOnce(&mut Signals)) -> Signals {
    let mut s = Signals {
        power: Some(AC),
        thermal: Thermal::Nominal,
        idle_secs: Some(10),
        memory: Some(Memory {
            avail_mb: 8_192,
            total_mb: 16_384,
        }),
        ..Signals::default()
    };
    f(&mut s);
    s
}

fn reason(s: &Signals, rules: &Rules) -> (Level, &'static str) {
    let w = target(s, rules).expect("signals were read");
    (w.level, w.reason)
}

#[test]
fn windows_nvidia_game_throttles_after_5_and_returns_after_the_cooldown() {
    let r = Rules::default();
    let mut g = Governor::new();
    let game = desk(|s| s.fullscreen = Some(quns_fullscreen(3)));
    assert_eq!(
        reason(&game, &r),
        (Level::Throttle, "a fullscreen app is in front")
    );
    assert_eq!(
        feed(&mut g, &game, &r, 10, 0, 2_000),
        vec![(5, Level::Throttle)]
    );
    // Back to the desktop at t=20 s: 30 samples is t=80 s, but the change at
    // t=10 s holds a step up until t=130 s, the 55th sample.
    let calm = desk(|s| s.fullscreen = Some(quns_fullscreen(1)));
    assert_eq!(reason(&calm, &r), (Level::Normal, "normal use"));
    assert_eq!(
        feed(&mut g, &calm, &r, 90, 20_000, 2_000),
        vec![(55, Level::Normal)]
    );
}

#[test]
fn windows_amd_gpu_load_leaves_grokhub_out_and_reads_engines_not_sums() {
    let rows = vec![
        (
            "pid_100_luid_0x0_0x1_phys_0_eng_0_engtype_3D".to_string(),
            70.0,
        ),
        (
            "pid_200_luid_0x0_0x1_phys_0_eng_0_engtype_3D".to_string(),
            20.0,
        ),
        (
            "pid_100_luid_0x0_0x1_phys_0_eng_1_engtype_Compute".to_string(),
            5.0,
        ),
    ];
    assert_eq!(gpu_from_pdh(&rows, Some(200)), 70);
    assert_eq!(gpu_from_pdh(&rows, None), 90);
    assert_eq!(gpu_from_pdh(&rows, Some(100)), 20);
    let r = Rules::default();
    let busy = desk(|s| s.external_gpu_pct = Some(gpu_from_pdh(&rows, Some(200))));
    assert_eq!(
        reason(&busy, &r),
        (Level::Throttle, "another app is keeping the GPU busy")
    );
    let ours = desk(|s| s.external_gpu_pct = Some(gpu_from_pdh(&rows, Some(100))));
    assert_eq!(reason(&ours, &r), (Level::Normal, "normal use"));
}

#[test]
fn multi_gpu_engines_on_two_adapters_are_not_added_together() {
    let rows = vec![
        (
            "pid_1_luid_0x0_0xA_phys_0_eng_0_engtype_3D".to_string(),
            30.0,
        ),
        (
            "pid_2_luid_0x0_0xB_phys_0_eng_0_engtype_3D".to_string(),
            50.0,
        ),
    ];
    assert_eq!(gpu_from_pdh(&rows, None), 50);
}

#[test]
fn windows_intel_igpu_laptop_on_battery_throttles_then_unloads_then_recovers() {
    let r = Rules::default();
    let mut g = Governor::new();
    let bat = power_from_status(0, 1, 55, false);
    assert_eq!(
        bat,
        PowerInfo {
            battery: true,
            on_ac: false,
            percent: Some(55),
            power_saver: false
        }
    );
    assert_eq!(interval_ms(Some(&bat)), 5_000);
    let on_bat = desk(|s| s.power = Some(bat));
    assert_eq!(reason(&on_bat, &r), (Level::Throttle, "on battery"));
    assert_eq!(
        feed(&mut g, &on_bat, &r, 5, 0, 5_000),
        vec![(5, Level::Throttle)]
    );
    let low = desk(|s| s.power = Some(power_from_status(0, 1, 15, false)));
    assert_eq!(reason(&low, &r), (Level::Unload, "the battery is low"));
    assert_eq!(
        feed(&mut g, &low, &r, 5, 25_000, 5_000),
        vec![(5, Level::Unload)]
    );
    // Plugged in (charging flag 8) at t=50 s: up needs 30 samples and t ≥ 170 s.
    let plugged = desk(|s| s.power = Some(power_from_status(1, 8, 15, false)));
    assert_eq!(interval_ms(plugged.power.as_ref()), 2_000);
    assert_eq!(
        feed(&mut g, &plugged, &r, 80, 50_000, 2_000),
        vec![(60, Level::Normal)]
    );
}

#[test]
fn power_saver_and_the_battery_rule() {
    let saver = power_from_status(1, 128, 255, true);
    assert_eq!(
        saver,
        PowerInfo {
            battery: false,
            on_ac: true,
            percent: None,
            power_saver: true
        }
    );
    let r = Rules::default();
    assert_eq!(
        reason(&desk(|s| s.power = Some(saver)), &r),
        (Level::Throttle, "power saver is on")
    );
    let keep = Rules {
        throttle_on_battery: false,
        ..Rules::default()
    };
    let bat = desk(|s| s.power = Some(power_from_status(0, 1, 55, false)));
    assert_eq!(reason(&bat, &keep), (Level::Normal, "normal use"));
}

#[test]
fn macos_apple_silicon_8_to_64_gb_pressure_throttles_and_idle_ramps_up() {
    let r = Rules::default();
    let idle = parse_ioreg_idle_secs("    |   \"HIDIdleTime\" = 400000000000\n");
    assert_eq!(idle, Some(400));
    for gb in [8_u64, 16, 32, 48, 64] {
        let total = gb * 1_024;
        let mut g = Governor::new();
        let ramp = desk(|s| {
            s.idle_secs = idle;
            s.os_pressure = parse_mac_pressure("1\n");
            s.memory = Some(Memory {
                avail_mb: total / 2,
                total_mb: total,
            });
        });
        assert_eq!(
            reason(&ramp, &r),
            (Level::RampUp, "idle on AC power"),
            "{gb} GB"
        );
        assert_eq!(
            feed(&mut g, &ramp, &r, 40, 0, 2_000),
            vec![(30, Level::RampUp)],
            "{gb} GB"
        );
        let tight = desk(|s| {
            s.idle_secs = idle;
            s.os_pressure = parse_mac_pressure("2\n");
        });
        assert_eq!(
            reason(&tight, &r),
            (Level::Throttle, "memory is running low"),
            "{gb} GB"
        );
        assert_eq!(
            feed(&mut g, &tight, &r, 6, 80_000, 2_000),
            vec![(5, Level::Throttle)],
            "{gb} GB"
        );
    }
}

#[test]
fn macos_intel_thermal_limits_and_games_category() {
    let hot = "Note: No thermal warning level has been recorded\nNote: No performance warning level has been recorded\nCPU_Scheduler_Limit \t= 100\nCPU_Available_CPUs \t= 8\nCPU_Speed_Limit \t= 40\n";
    assert_eq!(parse_pmset_therm(hot), Thermal::Critical);
    assert_eq!(
        parse_pmset_therm(&hot.replace("= 40", "= 80")),
        Thermal::Serious
    );
    assert_eq!(
        parse_pmset_therm(&hot.replace("= 40", "= 100")),
        Thermal::Nominal
    );
    assert_eq!(parse_pmset_therm(""), Thermal::Unknown);
    let r = Rules::default();
    let mut g = Governor::new();
    let s = desk(|s| s.thermal = parse_pmset_therm(hot));
    assert_eq!(reason(&s, &r), (Level::Unload, "the machine is too hot"));
    assert_eq!(feed(&mut g, &s, &r, 5, 0, 2_000), vec![(5, Level::Unload)]);
    let warm = desk(|s| s.thermal = Thermal::Serious);
    assert_eq!(
        reason(&warm, &r),
        (Level::Throttle, "the machine is running hot")
    );
    assert_eq!(
        parse_lsappinfo_bundle("\"LSBundlePath\"=\"/Applications/Chess.app\"\n"),
        Some("/Applications/Chess.app".into())
    );
    assert_eq!(parse_lsappinfo_bundle("\"LSBundlePath\"=[ NULL ]\n"), None);
    assert!(is_game_category("public.app-category.games"));
    assert!(is_game_category("public.app-category.action-games"));
    assert!(!is_game_category("public.app-category.productivity"));
}

#[test]
fn linux_cuda_own_sidecar_load_is_not_a_game() {
    assert_eq!(parse_nvidia_util("85\n12\n"), Some(85));
    let pmon = "# gpu         pid   type     sm    mem    enc    dec    jpg    ofa    command\n# Idx           #    C/G      %      %      %      %      %      %    name\n    0       4242     C     80     40      -      -      -      -    llama-server\n    0       1111     G      5      2      -      -      -      -    Xorg\n";
    assert_eq!(parse_nvidia_pmon(pmon, 4242), 80);
    assert_eq!(parse_nvidia_pmon(pmon, 9999), 0);
    assert_eq!(external_gpu(85, parse_nvidia_pmon(pmon, 4242)), 5);
    let r = Rules::default();
    let mut g = Governor::new();
    let s = desk(|s| s.external_gpu_pct = Some(external_gpu(85, 80)));
    assert_eq!(feed(&mut g, &s, &r, 50, 0, 2_000), vec![]);
    assert_eq!(g.level, Level::Normal);
}

#[test]
fn linux_rocm_x11_fullscreen_throttles() {
    let r = Rules::default();
    let mut g = Governor::new();
    let s = desk(|s| s.fullscreen = Some(true));
    assert_eq!(
        feed(&mut g, &s, &r, 7, 0, 2_000),
        vec![(5, Level::Throttle)]
    );
}

#[test]
fn linux_wayland_without_fullscreen_falls_back_to_games_then_gpu() {
    let r = Rules::default();
    assert!(gamemode_active("gamemode is active\n"));
    assert!(!gamemode_active("gamemode is inactive\n"));
    assert!(game_in_processes(&[(
        "gamescope".into(),
        "gamescope -- %command%".into()
    )]));
    assert!(game_in_processes(&[(
        "reaper".into(),
        "/home/u/.steam/ubuntu12_32/reaper SteamLaunch AppId=1091500 -- game".into()
    )]));
    assert!(game_in_processes(&[(
        "python3".into(),
        "python3 /home/u/.steam/steam/steamapps/common/Proton 9.0/proton waitforexitandrun /x/game.exe".into()
    )]));
    assert!(!game_in_processes(&[
        ("bash".into(), "bash".into()),
        ("firefox".into(), "/usr/lib/firefox/firefox".into())
    ]));
    let game = desk(|s| s.game = Some(gamemode_active("gamemode is active\n")));
    assert_eq!(game.fullscreen, None);
    assert_eq!(reason(&game, &r), (Level::Throttle, "a game is running"));
    // Only a busy GPU: a 4-sample spike, one calm sample, another spike, then
    // a sustained run that throttles on its 5th sample.
    let mut g = Governor::new();
    let hot = desk(|s| s.external_gpu_pct = Some(70));
    let calm = desk(|s| s.external_gpu_pct = Some(10));
    assert_eq!(feed(&mut g, &hot, &r, 4, 0, 2_000), vec![]);
    assert_eq!(feed(&mut g, &calm, &r, 1, 8_000, 2_000), vec![]);
    assert_eq!(feed(&mut g, &hot, &r, 4, 10_000, 2_000), vec![]);
    assert_eq!(feed(&mut g, &calm, &r, 1, 18_000, 2_000), vec![]);
    assert_eq!(
        feed(&mut g, &hot, &r, 6, 20_000, 2_000),
        vec![(5, Level::Throttle)]
    );
}

#[test]
fn compositor_hints_from_hyprland_and_sway() {
    assert_eq!(
        parse_hyprctl_active(r#"{"class":"steam_app_1091500","fullscreen":2}"#),
        (Some(true), Some("steam_app_1091500".into()))
    );
    assert_eq!(
        parse_hyprctl_active(r#"{"class":"firefox","fullscreen":false}"#),
        (Some(false), Some("firefox".into()))
    );
    // Maximized is not fullscreen, in either format.
    assert_eq!(
        parse_hyprctl_active(r#"{"class":"kitty","fullscreen":1}"#),
        (Some(false), Some("kitty".into()))
    );
    assert_eq!(
        parse_hyprctl_active(r#"{"class":"mpv","fullscreen":3}"#).0,
        Some(true)
    );
    assert_eq!(
        parse_hyprctl_active(r#"{"class":"kitty","fullscreen":true,"fullscreenMode":1}"#).0,
        Some(false)
    );
    assert_eq!(
        parse_hyprctl_active(r#"{"class":"mpv","fullscreen":true,"fullscreenMode":0}"#).0,
        Some(true)
    );
    assert_eq!(parse_hyprctl_active("not json"), (None, None));
    let tree = r#"{"focused":false,"nodes":[{"focused":false,"nodes":[
        {"focused":false,"app_id":"foot","fullscreen_mode":0},
        {"focused":true,"app_id":"mpv","fullscreen_mode":1}]}],"floating_nodes":[]}"#;
    assert_eq!(parse_sway_tree(tree), (Some(true), Some("mpv".into())));
    let x = r#"{"focused":false,"nodes":[{"focused":true,"app_id":null,"fullscreen_mode":0,"window_properties":{"class":"Steam"}}]}"#;
    assert_eq!(parse_sway_tree(x), (Some(false), Some("steam".into())));
}

#[test]
fn cpu_only_vulkan_or_intel_without_idle_never_ramps_up() {
    let r = Rules::default();
    let mut g = Governor::new();
    let s = desk(|s| s.idle_secs = None);
    assert_eq!(reason(&s, &r), (Level::Normal, "normal use"));
    assert_eq!(feed(&mut g, &s, &r, 200, 0, 2_000), vec![]);
    assert_eq!(g.level, Level::Normal);
}

#[test]
fn low_end_4_gb_throttles_and_8_gb_with_room_does_not() {
    let r = Rules::default();
    let four = desk(|s| {
        s.memory = Some(Memory {
            avail_mb: 300,
            total_mb: 4_096,
        })
    });
    assert_eq!(
        reason(&four, &r),
        (Level::Throttle, "memory is running low")
    );
    let eight = desk(|s| {
        s.memory = Some(Memory {
            avail_mb: 1_000,
            total_mb: 8_192,
        })
    });
    assert_eq!(reason(&eight, &r), (Level::Normal, "normal use"));
    assert_eq!(
        parse_vmrss_mb("Name:\tllama-server\nVmRSS:\t 2097152 kB\n"),
        Some(2_048)
    );
}

#[test]
fn thermal_critical_unloads_and_recovery_waits_for_30_and_the_cooldown() {
    let r = Rules::default();
    let mut g = Governor::new();
    let hot = desk(|s| s.thermal = Thermal::Critical);
    assert_eq!(
        feed(&mut g, &hot, &r, 5, 0, 2_000),
        vec![(5, Level::Unload)]
    );
    let ok = desk(|_| {});
    // Change at t=10 s; samples from t=10 s: the 60th is t=130 s.
    assert_eq!(
        feed(&mut g, &ok, &r, 70, 10_000, 2_000),
        vec![(60, Level::Normal)]
    );
}

#[test]
fn no_signals_keep_todays_behaviour() {
    let r = Rules::default();
    assert_eq!(target(&Signals::default(), &r), None);
    let mut g = Governor::new();
    assert_eq!(feed(&mut g, &Signals::default(), &r, 100, 0, 2_000), vec![]);
    assert_eq!(g.level, Level::Normal);
    assert_eq!(interval_ms(None), 2_000);
}

#[test]
fn an_unread_sample_breaks_the_streak() {
    let r = Rules::default();
    let mut g = Governor::new();
    let s = desk(|s| s.fullscreen = Some(true));
    assert_eq!(feed(&mut g, &s, &r, 4, 0, 2_000), vec![]);
    assert_eq!(g.observe(None, 10_000), None);
    assert_eq!(feed(&mut g, &s, &r, 4, 10_000, 2_000), vec![]);
    assert_eq!(g.level, Level::Normal);
}

#[test]
fn allow_and_deny_lists_win_over_the_screen() {
    let r =
        Rules::from_json(r#"{"allow":["VLC.exe"],"deny":["blender"],"throttleOnBattery":false}"#);
    assert_eq!(
        r,
        Rules {
            allow: vec!["VLC.exe".into()],
            deny: vec!["blender".into()],
            throttle_on_battery: false,
            pause_for_games: true,
            use_idle_time: true,
        }
    );
    let vlc = desk(|s| {
        s.fullscreen = Some(true);
        s.foreground = Some("vlc.exe".into());
    });
    assert_eq!(reason(&vlc, &r), (Level::Normal, "normal use"));
    let blender = desk(|s| {
        s.fullscreen = Some(false);
        s.foreground = Some("blender".into());
    });
    assert_eq!(
        reason(&blender, &r),
        (Level::Throttle, "an app on your deny list is in front")
    );
    assert_eq!(Rules::from_json("{broken"), Rules::default());
    assert!(Rules::default().throttle_on_battery);
}

#[test]
fn games_and_idle_time_follow_the_users_two_switches() {
    let off = Rules::from_json(r#"{"pauseForGames":false,"useIdleTime":false}"#);
    assert_eq!(
        off,
        Rules {
            pause_for_games: false,
            use_idle_time: false,
            ..Rules::default()
        }
    );
    let game = desk(|s| {
        s.game = Some(true);
        s.fullscreen = Some(true);
    });
    assert_eq!(
        reason(&game, &Rules::default()),
        (Level::Throttle, "a game is running")
    );
    assert_eq!(reason(&game, &off), (Level::Normal, "normal use"));
    // The deny list still throttles with games switched off.
    let deny = Rules {
        deny: vec!["blender".into()],
        ..off.clone()
    };
    let blender = desk(|s| s.foreground = Some("blender".into()));
    assert_eq!(
        reason(&blender, &deny),
        (Level::Throttle, "an app on your deny list is in front")
    );
    let idle = desk(|s| s.idle_secs = Some(IDLE_RAMP_SECS));
    assert_eq!(
        reason(&idle, &Rules::default()),
        (Level::RampUp, "idle on AC power")
    );
    assert_eq!(reason(&idle, &off), (Level::Normal, "normal use"));
}

#[test]
fn crash_breaker_opens_on_three_failures_in_ten_minutes() {
    let mut b = CrashBreaker::default();
    b.failed(0);
    b.failed(60_000);
    assert!(b.allows(60_001));
    b.failed(120_000);
    assert!(!b.allows(120_001));
    assert!(!b.allows(120_000 + CRASH_HOLD_MS - 1));
    assert!(b.allows(120_000 + CRASH_HOLD_MS));
    let mut spread = CrashBreaker::default();
    for t in [0, 700_000, 1_400_000] {
        spread.failed(t);
    }
    assert!(spread.allows(1_400_001));
}

#[test]
fn windows_screen_helpers() {
    assert!(!quns_fullscreen(1));
    assert!(quns_fullscreen(2));
    assert!(quns_fullscreen(3));
    assert!(quns_fullscreen(4));
    assert!(!quns_fullscreen(5));
    assert!(covers_monitor((0, 0, 1920, 1080), (0, 0, 1920, 1080)));
    assert!(covers_monitor((-8, -8, 1928, 1088), (0, 0, 1920, 1080)));
    assert!(!covers_monitor((0, 0, 1920, 1040), (0, 0, 1920, 1080)));
}

#[test]
fn macos_battery_from_pmset() {
    let batt = "Now drawing from 'Battery Power'\n -InternalBattery-0 (id=4653155)\t15%; discharging; 1:02 remaining present: true\n";
    assert_eq!(
        parse_pmset_batt(batt, " lowpowermode         1\n sleep 1\n"),
        PowerInfo {
            battery: true,
            on_ac: false,
            percent: Some(15),
            power_saver: true
        }
    );
    let ac = "Now drawing from 'AC Power'\n -InternalBattery-0 (id=4653155)\t100%; charged; 0:00 remaining present: true\n";
    assert_eq!(
        parse_pmset_batt(ac, " lowpowermode         0\n"),
        PowerInfo {
            battery: true,
            on_ac: true,
            percent: Some(100),
            power_saver: false
        }
    );
    let desktop = "Now drawing from 'AC Power'\n";
    assert_eq!(parse_pmset_batt(desktop, ""), AC);
    let r = Rules::default();
    let s = desk(|s| s.power = Some(parse_pmset_batt(batt, "")));
    assert_eq!(reason(&s, &r), (Level::Unload, "the battery is low"));
}

#[test]
fn the_apps_own_window_in_front_is_never_a_fullscreen_app() {
    // A borderless GrokHub window fills the work area: Windows says busy and
    // the rect covers the monitor, but it is us, not a game.
    assert_eq!(
        foreground_fullscreen(Some(true), Some(true), true),
        Some(false)
    );
    assert_eq!(foreground_fullscreen(None, None, true), Some(false));
    // Someone else's window: either reading counts.
    assert_eq!(
        foreground_fullscreen(Some(true), Some(false), false),
        Some(true)
    );
    assert_eq!(
        foreground_fullscreen(Some(false), Some(true), false),
        Some(true)
    );
    assert_eq!(
        foreground_fullscreen(Some(false), Some(false), false),
        Some(false)
    );
    assert_eq!(foreground_fullscreen(None, None, false), None);
    assert_eq!(foreground_fullscreen(None, Some(false), false), Some(false));
}
