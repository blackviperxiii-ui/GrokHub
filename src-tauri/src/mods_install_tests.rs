use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use grokhub_core::game_profile::Profile;
use grokhub_core::mods::{allowlist, Download, ModFile, Source};
use zip::write::{SimpleFileOptions, ZipWriter};

use super::*;

/// The sheet store is one per app; tests that park a sheet take turns.
static SERIAL: Mutex<()> = Mutex::new(());

fn temp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("gh-mods-install-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn write_zip(path: &Path, entries: &[(&str, &[u8])]) {
    let mut z = ZipWriter::new(File::create(path).unwrap());
    for (name, bytes) in entries {
        z.start_file(*name, SimpleFileOptions::default()).unwrap();
        z.write_all(bytes).unwrap();
    }
    z.finish().unwrap();
}

fn file(id: &str, version: &str, download: Download) -> ModFile {
    ModFile {
        id: id.into(),
        name: format!("Main file {version}"),
        file_name: "better-crops.zip".into(),
        version: version.into(),
        size: 0,
        hash: None,
        download,
        game_versions: Vec::new(),
        deps: vec!["SMAPI".into()],
        primary: true,
        uploaded: 0,
    }
}

/// A Stardew Valley folder with one mod the app didn't put there.
fn stardew(base: &Path) -> Target {
    let game_dir = base.join("Stardew Valley");
    std::fs::create_dir_all(game_dir.join("Mods/Other")).unwrap();
    std::fs::write(game_dir.join("Mods/Other/keep.txt"), b"theirs").unwrap();
    Target {
        layout: layout_for(allowlist::by_id("stardew").unwrap()).unwrap(),
        root: game_dir.clone(),
        game_dir,
        build: None,
        running: Arc::new(|| None),
    }
}

fn plan_zip(
    base: &Path,
    t: &Target,
    entries: &[(&str, &[u8])],
    version: &str,
) -> Result<Sheet, String> {
    let config = base.join("config");
    let zip = base.join(format!("download-{version}.zip"));
    write_zip(&zip, entries);
    let staging = base.join(format!("staging-{version}"));
    let entry = allowlist::by_id("stardew").unwrap();
    prepare(
        &config,
        t,
        entry,
        &Profile::default(),
        Source::Nexus,
        ("2100", "Better Crops"),
        &file(
            "7",
            version,
            Download::Direct("https://example.invalid/x".into()),
        ),
        &zip,
        &staging,
    )
}

fn files_under(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    fn walk(root: &Path, d: &Path, out: &mut Vec<String>) {
        for e in std::fs::read_dir(d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(root, &p, out);
            } else {
                out.push(
                    p.strip_prefix(root)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/"),
                );
            }
        }
    }
    walk(dir, dir, &mut out);
    out.sort();
    out
}

#[test]
fn install_writes_exactly_the_planned_files_and_uninstall_puts_the_folder_back() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let base = temp("roundtrip");
    let t = stardew(&base);
    let config = base.join("config");
    let sheet = plan_zip(
        &base,
        &t,
        &[
            ("BetterCrops/manifest.json", b"{}"),
            ("BetterCrops/BetterCrops.dll", b"dll"),
        ],
        "1.0.0",
    )
    .unwrap();
    let view = park(sheet);
    assert_eq!(view.title, "Install Better Crops 1.0.0?");
    assert_eq!(view.primary, "Install");
    assert_eq!(
        view.lists,
        vec![SheetList {
            title: "Adds (2)".into(),
            rows: vec![
                "Mods/BetterCrops/BetterCrops.dll".into(),
                "Mods/BetterCrops/manifest.json".into(),
            ],
        }]
    );
    // The sheet is only shown: nothing is in the game folder yet.
    assert_eq!(files_under(&t.game_dir), vec!["Mods/Other/keep.txt"]);

    let done = confirm(&config, view.token).unwrap();
    assert_eq!(
        done,
        "Better Crops 1.0.0 is in. Uninstall puts every file back."
    );
    assert_eq!(
        files_under(&t.game_dir),
        vec![
            "Mods/BetterCrops/BetterCrops.dll",
            "Mods/BetterCrops/manifest.json",
            "Mods/Other/keep.txt",
        ]
    );
    // The token was used up.
    assert!(confirm(&config, view.token).is_err());

    let ours = engine::installed(&config, Some("stardew"));
    assert_eq!(ours.len(), 1);
    assert_eq!(ours[0].mod_key, "nexus:2100");
    assert_eq!(ours[0].version, "1.0.0");

    let remove = remove_sheet(&config, "stardew", "nexus:2100", None, Arc::new(|| None)).unwrap();
    let view = park(remove);
    assert_eq!(view.title, "Uninstall Better Crops?");
    assert_eq!(view.lists[0].title, "Deletes (2)");
    assert_eq!(files_under(&t.game_dir).len(), 3);
    let done = confirm(&config, view.token).unwrap();
    assert_eq!(done, "Better Crops is out; the folder is as it was.");
    assert_eq!(files_under(&t.game_dir), vec!["Mods/Other/keep.txt"]);
    assert!(engine::installed(&config, Some("stardew")).is_empty());
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn a_new_version_is_an_update_and_replaces_the_old_files() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let base = temp("update");
    let t = stardew(&base);
    let config = base.join("config");
    let v1 = park(plan_zip(&base, &t, &[("BetterCrops/a.json", b"1")], "1.0.0").unwrap());
    confirm(&config, v1.token).unwrap();
    let v2 = park(plan_zip(&base, &t, &[("BetterCrops/b.json", b"2")], "1.1.0").unwrap());
    assert_eq!(v2.title, "Update Better Crops 1.1.0?");
    assert_eq!(v2.primary, "Update");
    assert_eq!(v2.lists[0].title, "The installed version comes out first");
    confirm(&config, v2.token).unwrap();
    assert_eq!(
        files_under(&t.game_dir),
        vec!["Mods/BetterCrops/b.json", "Mods/Other/keep.txt"]
    );
    assert_eq!(
        engine::installed(&config, Some("stardew"))[0].version,
        "1.1.0"
    );
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn cancel_writes_nothing_and_drops_the_staged_files() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let base = temp("cancel");
    let t = stardew(&base);
    let view = park(plan_zip(&base, &t, &[("BetterCrops/a.json", b"1")], "1.0.0").unwrap());
    assert!(base.join("staging-1.0.0").is_dir());
    cancel(view.token);
    assert!(!base.join("staging-1.0.0").exists());
    assert_eq!(
        confirm(&base.join("config"), view.token),
        Err("That sheet is gone. Start again.".into())
    );
    assert_eq!(files_under(&t.game_dir), vec!["Mods/Other/keep.txt"]);
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn an_anti_cheat_file_is_refused() {
    let base = temp("anticheat");
    let t = stardew(&base);
    let err = match plan_zip(&base, &t, &[("NoEAC/EasyAntiCheat/x.dll", b"x")], "1.0.0") {
        Err(e) => e,
        Ok(_) => panic!("an anti-cheat file was planned"),
    };
    assert!(err.starts_with("Loadout won't install this:"), "{err}");
    assert!(err.contains("can get the account banned"), "{err}");
    assert_eq!(files_under(&t.game_dir), vec!["Mods/Other/keep.txt"]);
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn a_path_that_climbs_out_of_the_archive_is_refused() {
    let base = temp("traversal");
    let t = stardew(&base);
    assert!(plan_zip(&base, &t, &[("../x", b"x")], "1.0.0").is_err());
    assert!(!base.join("x").exists());
    assert!(!base.join("staging-1.0.0/x").exists());
    assert_eq!(files_under(&t.game_dir), vec!["Mods/Other/keep.txt"]);
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn the_pickup_finds_the_browser_download() {
    let base = temp("pickup");
    std::fs::write(base.join("better-crops.zip"), b"zipbytes").unwrap();
    std::fs::write(base.join("unrelated.zip"), b"zipbytes").unwrap();
    let p = Pickup {
        key: "nexus:2100".into(),
        file_name: "better-crops.zip".into(),
        size: 8,
        since: SystemTime::now() - Duration::from_secs(60),
    };
    assert_eq!(
        find_download(&base, &p),
        Some(base.join("better-crops.zip"))
    );
    let wrong_size = Pickup {
        size: 9,
        ..p.clone()
    };
    assert_eq!(find_download(&base, &wrong_size), None);
    let later = Pickup {
        since: SystemTime::now() + Duration::from_secs(3600),
        ..p
    };
    assert_eq!(find_download(&base, &later), None);
    let _ = std::fs::remove_dir_all(&base);
}

fn manifest(key: &str, version: &str) -> Manifest {
    Manifest {
        game: "stardew".into(),
        mod_key: key.into(),
        name: "Better Crops".into(),
        version: version.into(),
        state: grokhub_core::mods::install::State::Installed,
        root: PathBuf::from("game"),
        files: Vec::new(),
        dirs: vec!["Mods/BetterCrops".into()],
        build: None,
        at: 0,
        offline_only: None,
    }
}

#[test]
fn each_file_offers_what_the_cabin_offers() {
    let direct = file(
        "7",
        "1.1.0",
        Download::Direct("https://cdn.invalid/a.zip".into()),
    );
    let browser = file(
        "8",
        "1.1.0",
        Download::Browser("https://site.invalid/f/8".into()),
    );
    let steam = file("9", "1.1.0", Download::Steam(42));
    let key = "nexus:2100";
    assert_eq!(offer(&direct, key, &[], false), Offer::Install);
    assert_eq!(offer(&direct, key, &[], true), Offer::Blocked);
    assert_eq!(
        offer(&browser, key, &[], false),
        Offer::Browser {
            url: "https://site.invalid/f/8".into()
        }
    );
    assert_eq!(
        offer(&steam, key, &[], true),
        Offer::Steam {
            url: grokhub_core::mods::workshop::item_url("42")
        }
    );
    let old = [manifest(key, "1.0.0")];
    assert_eq!(
        offer(&direct, key, &old, false),
        Offer::Update {
            was: "1.0.0".into()
        }
    );
    let same = [manifest(key, "1.1.0")];
    assert_eq!(offer(&direct, key, &same, false), Offer::Installed);
    let json = serde_json::to_value(Offer::Update {
        was: "1.0.0".into(),
    })
    .unwrap();
    assert_eq!(json, serde_json::json!({"kind": "update", "was": "1.0.0"}));
}

#[test]
fn the_drawer_lists_files_with_their_line_and_what_the_main_file_needs() {
    let mut f = file(
        "7",
        "1.1.0",
        Download::Browser("https://site.invalid/f/7".into()),
    );
    f.size = 4_200_000;
    let p = Pickup {
        key: "nexus:2100".into(),
        file_name: "better-crops.zip".into(),
        size: 0,
        since: SystemTime::now(),
    };
    let (rows, needs) = drawer_rows(&[f], "nexus:2100", &[], false, Some(&p));
    assert_eq!(rows[0].name, "Main file 1.1.0");
    assert_eq!(rows[0].line, "1.1.0 · 4.2 MB · download on the site");
    assert!(rows[0].waiting);
    assert_eq!(needs, vec!["SMAPI".to_string()]);
    assert_eq!(size_label(880_000), "880 KB");
}

#[test]
fn installed_lists_ours_once_and_keeps_the_rest() {
    let view = installed_view(
        vec![manifest("nexus:2100", "1.0.0")],
        vec![
            OtherRow {
                name: "BetterCrops".into(),
                detail: "addon".into(),
                link: None,
            },
            OtherRow {
                name: "Workshop item 5".into(),
                detail: "1.0 MB, from Steam Workshop".into(),
                link: Some("https://steamcommunity.com/sharedfiles/filedetails/?id=5".into()),
            },
        ],
    );
    assert_eq!(view.ours[0].key, "nexus:2100");
    assert_eq!(view.ours[0].version, "1.0.0");
    assert_eq!(view.others.len(), 1);
    assert_eq!(view.others[0].name, "Workshop item 5");
}

#[test]
fn a_running_game_must_close_first() {
    let base = temp("running");
    std::fs::write(base.join("StardewValley.exe"), b"").unwrap();
    let procs = vec![("stardewvalley.exe".to_string(), String::new())];
    assert_eq!(
        running_line(&procs, "Stardew Valley", Some(413150), &base, false),
        Some("Close Stardew Valley first.".into())
    );
    let idle = vec![("bash".to_string(), "bash".to_string())];
    assert_eq!(
        running_line(&idle, "Stardew Valley", Some(413150), &base, false),
        None
    );
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn games_the_app_does_not_write_say_why() {
    let rimworld = allowlist::by_id("rimworld").unwrap();
    assert_eq!(
        layout_for(rimworld).err(),
        Some("Steam installs Workshop items: subscribe in Steam.".into())
    );
}
