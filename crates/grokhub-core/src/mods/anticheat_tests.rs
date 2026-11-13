use std::path::PathBuf;

use super::*;
use crate::mods::allowlist::by_id;
use crate::mods::install::Planned;

fn plan(name: &str, files: &[&str]) -> Plan {
    Plan {
        game: "skyrimse".into(),
        mod_key: "nexus:1".into(),
        name: name.into(),
        version: "1.0".into(),
        root: PathBuf::from("/game"),
        files: files
            .iter()
            .map(|to| Planned {
                from: PathBuf::from("/staged").join(to),
                to: to.to_string(),
                replaces: false,
                sha256: String::new(),
            })
            .collect(),
        dirs: vec![],
        build: None,
        offline_only: None,
    }
}

fn eac() -> Profile {
    Profile {
        anti_cheat: Some("Easy Anti-Cheat".into()),
        sign: Some("folder easyanticheat in the game folder".into()),
    }
}

#[test]
fn an_unprotected_game_takes_loaders_and_reshade() {
    let sky = by_id("skyrimse").unwrap();
    let p = plan(
        "ENB",
        &["d3d11.dll", "reshade-shaders/Shaders/a.fx", "SKSE/x.dll"],
    );
    assert_eq!(screen(&p, sky, &Profile::default()), Screen::Clear);
}

#[test]
fn a_protected_offline_game_gets_the_loader_as_offline_only() {
    let sky = by_id("skyrimse").unwrap();
    for file in [
        "dxgi.dll",
        "bin/x64/version.dll",
        "DINPUT8.DLL",
        "ReShade.ini",
        "SweetFX/Shaders/a.fx",
    ] {
        let Screen::OfflineOnly(note) = screen(&plan("Look", &[file]), sky, &eac()) else {
            panic!("{file} should be offline only");
        };
        assert!(note.contains(file), "{note}");
        assert!(note.contains("Easy Anti-Cheat"), "{note}");
        assert!(note.contains("before you play online"), "{note}");
    }
    // Plain data files stay clear even under an anti-cheat, and words that
    // only hold the letters do not trip the tampering check.
    let p = plan(
        "Canoe Accessories",
        &["canoe_accessory.esp", "Noeacute.txt"],
    );
    assert_eq!(screen(&p, sky, &eac()), Screen::Clear);
    let p = plan("Better Sky", &["BetterSky.esp", "textures/sky.dds"]);
    assert_eq!(screen(&p, sky, &eac()), Screen::Clear);
}

#[test]
fn a_game_played_online_refuses_the_loader() {
    let wow = by_id("wow").unwrap();
    let warden = Profile {
        anti_cheat: Some("Warden".into()),
        sign: None,
    };
    let Screen::Refused(why) = screen(&plan("Shiny", &["Shiny/dxgi.dll"]), wow, &warden) else {
        panic!("a loader in a game played online is refused");
    };
    assert!(why.contains("played online under Warden"), "{why}");
    let addon = plan("Details", &["Details/Details.toc", "Details/core.lua"]);
    assert_eq!(screen(&addon, wow, &warden), Screen::Clear);
}

#[test]
fn turning_the_anti_cheat_off_is_refused_everywhere() {
    let sky = by_id("skyrimse").unwrap();
    let cases = [
        plan("Anti-Cheat Toggler", &["toggle.exe"]),
        plan("Seamless", &["EasyAntiCheat/EasyAntiCheat_EOS_Setup.exe"]),
        plan("Launcher fix", &["start_protected_game.exe"]),
        plan("Fix", &["BattlEye/BEClient_x64.dll"]),
        plan("Fix", &["no_eac.txt"]),
        plan("Fix", &["bin/EAC-Bypass.dll"]),
    ];
    for p in cases {
        for profile in [Profile::default(), eac()] {
            assert!(
                matches!(screen(&p, sky, &profile), Screen::Refused(_)),
                "{:?} under {:?}",
                p.files,
                profile.anti_cheat
            );
        }
    }
}

#[test]
fn the_rule_table_is_dated_and_sourced() {
    use crate::mods::allowlist::days_from_ymd;
    let checked = days_from_ymd(RULES_CHECKED).expect("a date");
    let today = (crate::now_ms() / 86_400_000) as i64;
    assert!(
        today - checked <= 365,
        "re-check the anti-cheat mod rules against their sources, then update RULES_CHECKED"
    );
    assert_eq!(SOURCES.len(), 3);
    for name in [
        "d3d9.dll",
        "dxgi.dll",
        "dsound.dll",
        "dinput8.dll",
        "version.dll",
        "winmm.dll",
    ] {
        assert!(PROXY_DLLS.contains(&name), "{name}");
    }
}
