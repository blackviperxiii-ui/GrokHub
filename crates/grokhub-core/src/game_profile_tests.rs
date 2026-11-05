use super::*;

fn dir(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("gh-game-profile-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn the_folder_shows_the_anti_cheat() {
    let none: [&str; 0] = [];
    let eac = dir("eac");
    std::fs::create_dir_all(eac.join("EasyAntiCheat")).unwrap();
    let p = profile(Protection::None, Some(&eac), &none);
    assert_eq!(p.anti_cheat.as_deref(), Some("Easy Anti-Cheat"));
    assert!(p.sign.unwrap().contains("easyanticheat"));
    let be = dir("be");
    std::fs::write(be.join("BEService_x64.exe"), b"").unwrap();
    assert_eq!(
        profile(Protection::None, Some(&be), &none)
            .anti_cheat
            .as_deref(),
        Some("BattlEye")
    );
    // A file named like the folder is not the folder.
    let odd = dir("odd");
    std::fs::write(odd.join("BattlEye"), b"").unwrap();
    assert!(!profile(Protection::None, Some(&odd), &none).protected());
    for d in [eac, be, odd] {
        let _ = std::fs::remove_dir_all(d);
    }
}

#[test]
fn the_game_list_counts_when_the_folder_says_nothing() {
    let none: [&str; 0] = [];
    let plain = dir("plain");
    assert_eq!(
        profile(Protection::None, Some(&plain), &none),
        Profile::default()
    );
    let p = profile(Protection::UserMode("Warden"), Some(&plain), &none);
    assert_eq!(p.anti_cheat.as_deref(), Some("Warden"));
    let p = profile(Protection::Kernel("BattlEye"), None, &none);
    assert_eq!(p.anti_cheat.as_deref(), Some("BattlEye"));
    let _ = std::fs::remove_dir_all(plain);
}

#[test]
fn an_always_on_driver_counts_only_beside_a_game_it_protects() {
    let riot = dir("riot");
    std::fs::write(riot.join("VALORANT.exe"), b"").unwrap();
    let p = profile(Protection::None, Some(&riot), &["vgk"]);
    assert_eq!(p.anti_cheat.as_deref(), Some("Vanguard"));
    let none: [&str; 0] = [];
    assert!(!profile(Protection::None, Some(&riot), &none).protected());
    let other = dir("other");
    std::fs::write(other.join("game.exe"), b"").unwrap();
    assert!(!profile(Protection::None, Some(&other), &["vgk"]).protected());
    for d in [riot, other] {
        let _ = std::fs::remove_dir_all(d);
    }
}
