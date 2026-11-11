//! Windows and macOS bundle `grokhub-hub` as a Tauri sidecar
//! (`bundle.externalBin` in the platform configs); Linux ships it as a deb
//! and AppImage file. Tauri installs a sidecar next to the app binary with the
//! target triple dropped, which is the first place `episode::hub_exe` looks.

use std::path::{Path, PathBuf};

use grokhub_shell::episode::hub_candidates;

#[test]
fn the_windows_and_macos_sidecar_sits_where_the_hub_lookup_checks_first() {
    // NSIS installer: `<install dir>\grokhub-hub.exe` beside the app.
    assert_eq!(
        hub_candidates(
            Path::new("C:/Program Files/Loadout/grokhub-shell.exe"),
            true
        )[0],
        PathBuf::from("C:/Program Files/Loadout/grokhub-hub.exe")
    );
    // macOS app bundle: `Contents/MacOS/grokhub-hub` beside the app binary.
    assert_eq!(
        hub_candidates(
            Path::new("/Applications/Loadout.app/Contents/MacOS/grokhub-shell"),
            false
        )[0],
        PathBuf::from("/Applications/Loadout.app/Contents/MacOS/grokhub-hub")
    );
}

#[test]
fn only_windows_and_macos_declare_the_hub_sidecar() {
    let main: serde_json::Value =
        serde_json::from_str(include_str!("../tauri.conf.json")).expect("tauri.conf.json");
    assert!(
        main["bundle"].get("externalBin").is_none(),
        "Linux jobs build src-tauri without a staged sidecar"
    );
    for (name, conf) in [
        ("windows", include_str!("../tauri.windows.conf.json")),
        ("macos", include_str!("../tauri.macos.conf.json")),
    ] {
        let conf: serde_json::Value = serde_json::from_str(conf).expect(name);
        assert_eq!(
            conf["bundle"]["externalBin"],
            serde_json::json!(["grokhub-hub"]),
            "{name} config"
        );
    }
}

#[test]
fn the_stage_script_writes_the_path_tauri_checks_and_ci_runs_it_before_cargo() {
    let script = include_str!("../../scripts/stage-hub-sidecar.mjs");
    assert!(script.contains(r#"path.join(root, "src-tauri", `grokhub-hub-${host}${exe}`)"#));
    for (name, workflow, first_cargo) in [
        (
            "ci.yml",
            include_str!("../../.github/workflows/ci.yml"),
            "cargo fmt --manifest-path src-tauri/Cargo.toml --check",
        ),
        (
            "release.yml",
            include_str!("../../.github/workflows/release.yml"),
            "ui/node_modules/.bin/tauri build -- --locked",
        ),
    ] {
        let stage = workflow
            .find("run: node scripts/stage-hub-sidecar.mjs")
            .unwrap_or_else(|| panic!("{name} stages the sidecar"));
        let cargo = workflow
            .find(first_cargo)
            .unwrap_or_else(|| panic!("{name} builds src-tauri"));
        assert!(
            stage < cargo,
            "{name} stages the sidecar before src-tauri cargo"
        );
    }
}
