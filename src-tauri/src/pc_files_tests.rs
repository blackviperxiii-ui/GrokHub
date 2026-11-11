use super::*;
use grokhub_core::print_alerts::PrinterKind;

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "grokhub-pc-files-{name}-{}-{}",
        std::process::id(),
        grokhub_core::now_ms()
    ));
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn every_switch_defaults_off_and_the_printer_camera_is_its_own_opt_in() {
    let d = dir("defaults");
    let h = read(&d);
    assert!(!h.downloads_tidy && !h.model_drop && !h.backup_nudge);
    assert!(!h.resume_code && !h.net_log);
    assert!(!h.print_alerts && !h.print_camera);
    assert!(h.code_folders.is_empty() && h.important_folders.is_empty());
    assert!(h.printers.is_empty());
    assert_eq!(h.net_drops, 0);
    assert_eq!(h.net_by_hour.len(), 24);
    assert_eq!(h.models_folder, "");
}

#[test]
fn each_switch_round_trips_in_the_cabins_app_json_keys() {
    let d = dir("roundtrip");
    std::fs::write(
        grokhub_core::app_json::path(&d),
        r#"{"quietStart":"23:00","futureKey":{"a":[1,2]}}"#,
    )
    .unwrap();
    for key in [
        "downloadsTidy",
        "modelDrop",
        "backupNudge",
        "resumeCode",
        "netLog",
    ] {
        let on = set_switch(&d, key, true).unwrap();
        let map = grokhub_core::app_json::read(&d).unwrap();
        assert_eq!(map[key], json!(true), "{key} is written under its own key");
        assert!(switch_on(&d, key));
        drop(on);
        set_switch(&d, key, false).unwrap();
        assert!(!switch_on(&d, key));
    }
    let map = grokhub_core::app_json::read(&d).unwrap();
    assert_eq!(map["quietStart"], json!("23:00"));
    assert_eq!(map["futureKey"], json!({"a": [1, 2]}));
}

#[test]
fn the_internet_log_switch_also_flips_the_log_files_own_flag() {
    let d = dir("netlog");
    set_switch(&d, "netLog", true).unwrap();
    assert!(net_log::load(&d).on);
    set_switch(&d, "netLog", false).unwrap();
    assert!(!net_log::load(&d).on);
}

#[test]
fn print_alerts_and_camera_save_as_the_cabins_object() {
    let d = dir("print");
    set_switch(&d, "printAlerts", true).unwrap();
    let map = grokhub_core::app_json::read(&d).unwrap();
    assert_eq!(
        map["printAlerts"],
        json!({"enabled": true, "camera": false})
    );
    let h = set_switch(&d, "printCamera", true).unwrap();
    assert!(h.print_alerts && h.print_camera);
    let h = set_switch(&d, "printAlerts", false).unwrap();
    assert!(
        !h.print_alerts && h.print_camera,
        "the camera keeps its own choice"
    );
    assert!(set_switch(&d, "nope", true).is_err());
}

#[test]
fn folder_lists_keep_real_folders_once_and_refuse_the_rest() {
    let d = dir("folders");
    let a = d.join("code");
    std::fs::create_dir_all(&a).unwrap();
    let a = a.to_string_lossy().to_string();
    let h = set_folders(&d, "codeFolders", &[format!(" {a} "), a.clone(), "".into()]).unwrap();
    assert_eq!(h.code_folders, vec![a.clone()]);
    assert_eq!(code_folders(&d), vec![a.clone()]);
    let err = set_folders(&d, "importantFolders", &["relative/dir".into()]).unwrap_err();
    assert_eq!(err, "relative/dir isn't a full folder path.");
    let missing = d.join("missing").to_string_lossy().to_string();
    assert!(set_folders(&d, "importantFolders", &[missing]).is_err());
    assert!(
        important_folders(&d).is_empty(),
        "a refused list changes nothing"
    );
    assert!(set_folders(&d, "other", &[]).is_err());
    let h = set_models_folder(&d, &a).unwrap();
    assert_eq!(h.models_folder, a);
    assert_eq!(set_models_folder(&d, "").unwrap().models_folder, "");
}

#[test]
fn only_printers_on_this_network_can_be_watched() {
    assert_eq!(
        lan_endpoint("192.168.1.50:7125").unwrap(),
        "192.168.1.50:7125"
    );
    assert_eq!(
        lan_endpoint("http://10.0.0.8:5000/").unwrap(),
        "10.0.0.8:5000"
    );
    assert_eq!(lan_endpoint("127.0.0.1:7125").unwrap(), "127.0.0.1:7125");
    assert_eq!(
        lan_endpoint("8.8.8.8:80").unwrap_err(),
        "Only printers on this network (or this PC) can be watched."
    );
    assert!(lan_endpoint("printer.example.com:80").is_err());
    assert!(lan_endpoint("192.168.1.50").is_err());
    assert!(lan_endpoint("192.168.1.50:http").is_err());
}

#[test]
fn printers_save_toggle_and_remove_in_the_cabins_ledger() {
    let d = dir("printers");
    let p = SavedPrinter {
        endpoint: "192.168.1.50:7125".into(),
        kind: PrinterKind::Moonraker,
        name: "voron".into(),
        enabled: true,
        api_key: None,
    };
    let h = save_printer(&d, p).unwrap();
    assert_eq!(
        h.printers,
        vec![PrinterRow {
            key: "192.168.1.50:7125".into(),
            name: "voron".into(),
            kind: "Moonraker".into(),
            enabled: true,
        }]
    );
    let h = set_printer_enabled(&d, "192.168.1.50:7125", false).unwrap();
    assert!(!h.printers[0].enabled);
    assert!(set_printer_enabled(&d, "10.0.0.1:1", true).is_err());
    assert!(d.join("print-alerts.json").exists());
    let h = remove_printer(&d, "192.168.1.50:7125").unwrap();
    assert!(h.printers.is_empty());
}

#[test]
fn a_poll_never_undoes_a_printer_removed_meanwhile() {
    let d = dir("merge");
    let p = SavedPrinter {
        endpoint: "192.168.1.50:7125".into(),
        kind: PrinterKind::Moonraker,
        name: String::new(),
        enabled: true,
        api_key: None,
    };
    save_printer(&d, p).unwrap();
    let mut polled = load_ledger(&d);
    polled.last_poll_ms = 77;
    polled
        .last_state
        .insert("192.168.1.50:7125".into(), "printing".into());
    remove_printer(&d, "192.168.1.50:7125").unwrap();
    merge_poll(&d, polled).unwrap();
    let ledger = load_ledger(&d);
    assert!(ledger.printers.is_empty());
    assert!(ledger.last_state.is_empty());
    assert_eq!(ledger.last_poll_ms, 77);
}

#[test]
fn the_csv_export_never_replaces_an_earlier_one_the_same_day() {
    let d = dir("csv");
    // Files go under a folder of this test's own, never the real Reports.
    let root = dir("csv-files").join("my-files");
    grokhub_core::user_files::set_files_root(&d, &root).unwrap();
    let now = 1_800_000_000_000;
    let first = export_net_csv(&d, now).unwrap();
    let second = export_net_csv(&d, now).unwrap();
    assert_ne!(first, second);
    assert_eq!(
        PathBuf::from(&first),
        root.join("Reports").join("internet-log-2027-01-15.csv")
    );
    assert_eq!(
        PathBuf::from(&second),
        root.join("Reports").join("internet-log-2027-01-15-2.csv")
    );
}
