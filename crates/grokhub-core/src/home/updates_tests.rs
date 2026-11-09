use super::*;

fn item(name: &str, from: Option<&str>, to: Option<&str>) -> UpdateItem {
    UpdateItem {
        name: name.into(),
        from: from.map(Into::into),
        to: to.map(Into::into),
    }
}

#[test]
fn apt_rows_keep_name_new_and_old_versions() {
    let out = "Listing... Done\n\
               mesa-vulkan-drivers/noble-updates 24.0.9-0ubuntu0.2 amd64 [upgradable from: 24.0.9-0ubuntu0.1]\n\
               firefox/noble 130.0 amd64 [upgradable from: 129.0]\n";
    assert_eq!(
        parse_linux("apt", out),
        vec![
            item(
                "mesa-vulkan-drivers",
                Some("24.0.9-0ubuntu0.1"),
                Some("24.0.9-0ubuntu0.2")
            ),
            item("firefox", Some("129.0"), Some("130.0")),
        ]
    );
}

#[test]
fn pacman_dnf_and_zypper_rows_parse() {
    assert_eq!(
        parse_linux(
            "pacman",
            "linux 6.10.1-1 -> 6.10.2-1\nnvidia-utils 555.58-1 -> 560.35-1\n"
        ),
        vec![
            item("linux", Some("6.10.1-1"), Some("6.10.2-1")),
            item("nvidia-utils", Some("555.58-1"), Some("560.35-1")),
        ]
    );
    assert_eq!(
        parse_linux("dnf", "\nkernel.x86_64  6.10.2-200.fc40  updates\n"),
        vec![item("kernel", None, Some("6.10.2-200.fc40"))]
    );
    assert_eq!(
        parse_linux(
            "zypper",
            "S | Repository | Name | Current Version | Available Version | Arch\n\
             --+------------+------+-----------------+-------------------+-----\n\
             v | Main | vim | 9.1.0-1.1 | 9.1.1-1.1 | x86_64\n"
        ),
        vec![item("vim", Some("9.1.0-1.1"), Some("9.1.1-1.1"))]
    );
}

#[test]
fn winget_rows_are_cut_at_the_header_columns() {
    let out = "Name               Id                    Version   Available Source\n\
               ------------------------------------------------------------------\n\
               Discord            Discord.Discord       1.0.9150  1.0.9160  winget\n\
               Visual Studio Code Microsoft.VSCode      1.92.0    1.93.1    winget\n\
               2 upgrades available.\n";
    assert_eq!(
        parse_winget(out),
        vec![
            item("Discord", Some("1.0.9150"), Some("1.0.9160")),
            item("Visual Studio Code", Some("1.92.0"), Some("1.93.1")),
        ]
    );
    assert_eq!(
        parse_winget("No installed package found matching input criteria."),
        vec![]
    );
}

#[test]
fn windows_update_titles_become_items() {
    assert_eq!(
        parse_windows_update(
            "2024-09 Cumulative Update for Windows 11\r\n\r\nNVIDIA - Display - 32.0.15.6094\r\n"
        ),
        vec![
            UpdateItem::named("2024-09 Cumulative Update for Windows 11"),
            UpdateItem::named("NVIDIA - Display - 32.0.15.6094"),
        ]
    );
}

fn linux_sources() -> Sources {
    Sources {
        version: "3.5.0".into(),
        loadout_newer: None,
        loadout_updatable: true,
        steam: Some(vec!["Hades II".into()]),
        system: Some(Found::items(vec![item(
            "linux",
            Some("6.10.1"),
            Some("6.10.2"),
        )])),
        apps: None,
        windows: Some(Found::NotHere),
        gpu_driver: Some("NVIDIA 560.35".into()),
        gaming: false,
        drivers: None,
    }
}

#[test]
fn the_page_lists_each_source_with_one_offer() {
    let p = page(&linux_sources());
    assert_eq!(p.headline, "2 waiting");
    assert_eq!(p.driver.as_deref(), Some("GPU driver NVIDIA 560.35"));
    let kinds: Vec<Kind> = p.groups.iter().map(|g| g.kind).collect();
    // Windows Update isn't on this PC and apps weren't read: both left off.
    assert_eq!(kinds, vec![Kind::Loadout, Kind::Steam, Kind::System]);
    assert_eq!(p.groups[0].line, "3.5.0 is the newest");
    assert_eq!(p.groups[0].offer, None);
    assert_eq!(p.groups[1].line, "1 game waiting");
    assert_eq!(
        p.groups[1].offer.as_ref().unwrap().offer,
        Offer::OpenSteamDownloads
    );
    assert_eq!(p.groups[2].line, "1 package waiting");
    let sys = p.groups[2].offer.as_ref().unwrap();
    assert_eq!(
        (sys.offer, sys.label.as_str(), sys.held.as_deref()),
        (Offer::UpdateSystem, "Update system", None)
    );
}

#[test]
fn a_running_game_holds_the_heavy_updates_only() {
    let mut s = linux_sources();
    s.gaming = true;
    s.loadout_newer = Some("3.6.0".into());
    let p = page(&s);
    assert_eq!(p.headline, "3 waiting");
    assert_eq!(p.groups[0].line, "3.6.0 is ready (you have 3.5.0)");
    assert_eq!(p.groups[0].offer.as_ref().unwrap().held, None);
    assert_eq!(p.groups[1].offer.as_ref().unwrap().held, None);
    assert_eq!(
        p.groups[2].offer.as_ref().unwrap().held.as_deref(),
        Some("Waits until your game closes")
    );
}

#[test]
fn up_to_date_and_unreadable_sources_say_so() {
    let s = Sources {
        version: "3.5.0".into(),
        loadout_updatable: false,
        loadout_newer: Some("3.6.0".into()),
        steam: Some(vec![]),
        apps: Some(Found::items(vec![])),
        windows: Some(Found::CantCheck {
            why: "Windows Update didn't answer".into(),
        }),
        ..Sources::default()
    };
    let p = page(&s);
    // The installer can't update itself here (a package build): no chip.
    assert_eq!(p.groups[0].offer, None);
    assert_eq!(p.groups[1].line, "Up to date");
    assert_eq!(p.groups[1].offer, None);
    assert_eq!(p.groups[2].line, "Up to date");
    assert_eq!(p.groups[2].offer, None);
    assert_eq!(
        p.groups[3].line,
        "Couldn't check: Windows Update didn't answer"
    );
    // Its own page can still be opened.
    assert_eq!(
        p.groups[3].offer.as_ref().unwrap().offer,
        Offer::OpenWindowsUpdate
    );
}

#[test]
fn the_system_upgrade_snapshots_first_then_runs_the_distros_own_upgrade() {
    let apt = system_upgrade_script("apt").unwrap();
    assert!(apt.starts_with("if command -v snapper"));
    assert!(apt.ends_with(
        "\napt-get update && DEBIAN_FRONTEND=noninteractive apt-get -y -o Dpkg::Options::=--force-confdef -o Dpkg::Options::=--force-confold upgrade"
    ));
    assert!(system_upgrade_script("pacman")
        .unwrap()
        .ends_with("\npacman -Syu --noconfirm"));
    assert_eq!(system_upgrade_script("brew"), None);
}

#[cfg(unix)]
#[test]
fn every_upgrade_script_parses_as_sh() {
    for pm in ["apt", "pacman", "dnf", "zypper"] {
        let script = system_upgrade_script(pm).unwrap();
        let ok = std::process::Command::new("sh")
            .args(["-n", "-c", &script])
            .status()
            .unwrap()
            .success();
        assert!(ok, "{pm}: {script}");
    }
}

fn driver(name: &str) -> UpdateItem {
    item(name, Some("560.35.03-1"), Some("565.57.01-1"))
}

#[test]
fn checked_drivers_offer_an_update_that_waits_for_the_game() {
    let mut s = linux_sources();
    s.drivers = Some(Drivers::Ready {
        items: vec![driver("nvidia-utils")],
        way_back: WayBack::OldPackages,
    });
    let p = page(&s);
    assert_eq!(p.headline, "3 waiting");
    let g = p.groups.iter().find(|g| g.kind == Kind::Drivers).unwrap();
    assert_eq!(
        g.line,
        "1 driver update checked twice. The old version stays, so it can go back."
    );
    let chip = g.offer.as_ref().unwrap();
    assert_eq!(
        (chip.offer, chip.held.as_deref()),
        (Offer::UpdateDrivers, None)
    );
    s.gaming = true;
    let p = page(&s);
    let g = p.groups.iter().find(|g| g.kind == Kind::Drivers).unwrap();
    assert_eq!(
        g.offer.as_ref().unwrap().held.as_deref(),
        Some("Waits until your game closes")
    );
}

#[test]
fn an_updated_driver_offers_its_way_back() {
    let mut s = linux_sources();
    s.drivers = Some(Drivers::Updated {
        items: vec![driver("nvidia-utils")],
        way_back: WayBack::OldPackages,
    });
    let p = page(&s);
    let g = p.groups.iter().find(|g| g.kind == Kind::Drivers).unwrap();
    assert!(g.line.starts_with("Updated. Restart to load it."));
    assert_eq!(g.offer.as_ref().unwrap().offer, Offer::RollBackDrivers);
    // A snapshot goes back with its own tool: no chip, just the name.
    s.drivers = Some(Drivers::Updated {
        items: vec![driver("nvidia-utils")],
        way_back: WayBack::Snapshot("Snapper".into()),
    });
    let p = page(&s);
    let g = p.groups.iter().find(|g| g.kind == Kind::Drivers).unwrap();
    assert!(g
        .line
        .ends_with("restore the before-driver-update snapshot in Snapper."));
    assert_eq!(g.offer, None);
}

#[test]
fn windows_drivers_open_optional_updates() {
    let s = Sources {
        version: "3.5.0".into(),
        windows: Some(Found::NotHere),
        drivers: Some(Drivers::Windows {
            items: vec![UpdateItem::named("NVIDIA - Display - 32.0.15.6094")],
        }),
        ..Sources::default()
    };
    let p = page(&s);
    assert_eq!(p.headline, "1 waiting");
    let g = p.groups.iter().find(|g| g.kind == Kind::Drivers).unwrap();
    assert_eq!(g.line, "1 driver update in Windows Update");
    assert_eq!(g.offer.as_ref().unwrap().offer, Offer::OpenOptionalUpdates);
}
