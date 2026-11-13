use super::*;

fn list(names: &[&str]) -> Blocklist {
    Blocklist {
        fetched_at: 1,
        names: names.iter().map(|n| n.to_string()).collect(),
    }
}

#[test]
fn install_commands_name_what_they_install() {
    assert_eq!(
        installs("winget install -e --id Rem0o.FanControl"),
        vec!["rem0o.fancontrol"]
    );
    assert_eq!(
        installs("winget install --id=Guru3D.Afterburner"),
        vec!["guru3d.afterburner"]
    );
    assert_eq!(installs("choco install speedfan -y"), vec!["speedfan"]);
    assert_eq!(installs("sudo pacman -S --needed openrgb"), vec!["openrgb"]);
    assert_eq!(
        installs(r#"msiexec /i "C:\Downloads\HWiNFO_Setup.msi" /qn"#),
        vec!["hwinfo_setup.msi"]
    );
    // Reading about a tool installs nothing.
    assert!(installs("winget show Rem0o.FanControl").is_empty());
    assert!(installs("cat fancontrol.json").is_empty());
}

#[test]
fn input_drivers_are_refused_however_they_come() {
    for cmd in [
        "winget install interception",
        r#"cd "Interception\command line installer" && install-interception.exe /install"#,
        "install-interception /install",
        "choco install interception-driver -y",
    ] {
        let why = input_driver_install(cmd).unwrap_or_else(|| panic!("{cmd}"));
        assert!(why.contains("Interception"), "{why}");
    }
    assert_eq!(
        input_driver_install("winget install Rem0o.FanControl"),
        None
    );
    assert_eq!(input_driver_install("git log -- interception.rs"), None);
}

#[test]
fn a_tool_is_flagged_only_when_the_blocklist_names_its_driver() {
    let cmd = "winget install Rem0o.FanControl";
    assert_eq!(proposed_tool(cmd).map(|t| t.name), Some("Fan Control"));
    // No cache: nothing is flagged, never a guess.
    assert_eq!(flag(cmd, &Blocklist::default()), None);
    assert_eq!(flag(cmd, &list(&["rtcore64.sys"])), None);
    let f = flag(cmd, &list(&["winring0x64.sys"])).expect("flagged");
    assert_eq!(f.driver, "winring0x64.sys");
    let words = explain(&f);
    assert!(
        words.contains("Fan Control (fan control) installs winring0x64.sys"),
        "{words}"
    );
    assert!(words.contains("BattlEye and Vanguard"), "{words}");
    // A tool the table doesn't know is left alone.
    assert_eq!(
        flag(
            "winget install Mozilla.Firefox",
            &list(&["winring0x64.sys"])
        ),
        None
    );
}

#[test]
fn the_policy_xml_gives_driver_file_names() {
    let xml = r#"<SiPolicy><FileRules>
      <Deny ID="ID_DENY_RTCORE_1" FriendlyName="RTCore64.sys Hash Sha1" Hash="AB"/>
      <Deny ID="ID_DENY_X" FriendlyName="\speedfan.sys\ Hash Page Sha256" Hash="CD"/>
      <FileAttrib ID="ID_FILEATTRIB_GDRV" FriendlyName="" FileName="gdrv.sys" MinimumFileVersion="65535.0.0.0"/>
      <Deny ID="ID_DENY_Y" FriendlyName="Some vendor driver" Hash="EF"/>
    </FileRules></SiPolicy>"#;
    let names = parse_xml(xml);
    assert_eq!(
        names.into_iter().collect::<Vec<_>>(),
        vec!["gdrv.sys", "rtcore64.sys", "speedfan.sys"]
    );
}

#[test]
fn the_zip_and_the_cache_round_trip() {
    use std::io::Write;
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buf);
        let opts = zip::write::SimpleFileOptions::default();
        zip.start_file("VulnerableDriverBlockList/SiPolicy_Enforced.xml", opts)
            .unwrap();
        zip.write_all(br#"<FileAttrib FileName="WinRing0x64.sys"/>"#)
            .unwrap();
        zip.start_file("readme.txt", opts).unwrap();
        zip.write_all(b"FileName=\"ignored.sys\"").unwrap();
        zip.finish().unwrap();
    }
    let names = names_from_zip(buf.get_ref()).unwrap();
    assert_eq!(
        names.into_iter().collect::<Vec<_>>(),
        vec!["winring0x64.sys"]
    );
    assert!(names_from_zip(b"not a zip").is_err());

    let dir = std::env::temp_dir().join(format!("gh-driver-check-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    assert!(due(&dir, 100));
    assert!(cached(&dir).names.is_empty());
    let l = list(&["winring0x64.sys"]);
    save(
        &dir,
        &Blocklist {
            fetched_at: 100,
            ..l
        },
    )
    .unwrap();
    assert!(!due(&dir, 100 + REFRESH_DAYS * 86_400));
    assert!(due(&dir, 101 + REFRESH_DAYS * 86_400));
    assert!(cached(&dir).names.contains("winring0x64.sys"));
    let _ = std::fs::remove_dir_all(dir);
}
