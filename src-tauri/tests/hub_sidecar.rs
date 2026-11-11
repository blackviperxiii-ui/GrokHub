//! Windows and macOS bundle `grokhub-hub` as a Tauri sidecar
//! (`bundle.externalBin` in the platform configs); Linux ships it as a deb
//! and AppImage file. Tauri installs a sidecar next to the app binary with the
//! target triple dropped, which is the first place `episode::hub_exe` looks.

use std::collections::BTreeSet;
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

/// The step both workflows run before any cargo or tauri step in `src-tauri`,
/// as whole lines: its name, its condition and its command together.
const STAGE_STEP: [&str; 3] = [
    "      - name: Stage the grokhub-hub sidecar",
    "        if: runner.os != 'Linux'",
    "        run: node scripts/stage-hub-sidecar.mjs",
];

/// The lines of one job in a workflow: from its `  <job>:` line up to the next
/// line indented two spaces or less that is not blank or a comment.
fn job_lines<'a>(workflow: &'a str, job: &str) -> Vec<&'a str> {
    let head = format!("  {job}:");
    let mut inside = false;
    let mut lines = Vec::new();
    for line in workflow.lines() {
        let text = line.trim_start();
        if !line.starts_with("   ") && !text.is_empty() && !text.starts_with('#') {
            inside = line == head;
        }
        if inside {
            lines.push(line);
        }
    }
    assert!(!lines.is_empty(), "the workflow has a `{job}` job");
    lines
}

/// A workflow line that runs cargo or the Tauri CLI (comments do not count).
fn runs_cargo_or_tauri(line: &str) -> bool {
    !line.trim_start().starts_with('#')
        && line
            .split_whitespace()
            .any(|word| word == "cargo" || word == "tauri" || word.ends_with("/tauri"))
}

#[test]
fn the_stage_script_writes_the_path_tauri_checks_and_ci_runs_it_before_cargo() {
    let script = include_str!("../../scripts/stage-hub-sidecar.mjs");
    assert!(script.contains(r#"path.join(root, "src-tauri", `grokhub-hub-${host}${exe}`)"#));
    // The build names the host triple and the copy reads that triple's folder,
    // so CARGO_BUILD_TARGET or build.target cannot leave an older host binary
    // in the path the script copies from.
    assert!(script.contains(r#""--target", host,"#));
    assert!(script.contains(r#"path.join(targetDir, host, "release", `grokhub-hub${exe}`)"#));
    for (name, workflow, job, first_cargo) in [
        (
            "ci.yml",
            include_str!("../../.github/workflows/ci.yml"),
            "shell",
            "      - run: cargo fmt --manifest-path src-tauri/Cargo.toml --check",
        ),
        (
            "release.yml",
            include_str!("../../.github/workflows/release.yml"),
            "shell-bundles",
            "        run: ui/node_modules/.bin/tauri build -- --locked",
        ),
    ] {
        let lines = job_lines(workflow, job);
        let stage: Vec<usize> = (0..lines.len())
            .filter(|at| lines[*at..].starts_with(&STAGE_STEP))
            .collect();
        assert_eq!(
            stage.len(),
            1,
            "{name}: the `{job}` job stages the sidecar once, on Windows and macOS"
        );
        let cargo = lines
            .iter()
            .position(|line| runs_cargo_or_tauri(line))
            .unwrap_or_else(|| panic!("{name}: the `{job}` job builds src-tauri"));
        assert_eq!(
            lines[cargo], first_cargo,
            "{name}: the first cargo or tauri line of the `{job}` job"
        );
        assert!(
            stage[0] + STAGE_STEP.len() <= cargo,
            "{name}: the `{job}` job stages the sidecar before src-tauri cargo"
        );
    }
}

/// The value of every `path = "…"` key in a Cargo manifest.
fn manifest_paths(manifest: &str) -> Vec<&str> {
    manifest
        .split("path")
        .skip(1)
        .filter_map(|rest| {
            let rest = rest.trim_start().strip_prefix('=')?;
            rest.trim_start().strip_prefix('"')?.split('"').next()
        })
        .collect()
}

/// The crates a manifest depends on by path, where `prefix` is how that
/// manifest reaches `crates/`. A `path` that stays inside the package (a
/// `[[bin]]` source) is not a dependency; any other shape fails the test.
fn path_dependencies(manifest: &Path, prefix: &str) -> Vec<String> {
    let text =
        std::fs::read_to_string(manifest).unwrap_or_else(|e| panic!("{}: {e}", manifest.display()));
    manifest_paths(&text)
        .into_iter()
        .filter(|value| value.starts_with("../"))
        .map(|value| match value.strip_prefix(prefix) {
            Some(name) if !name.is_empty() && !name.contains('/') => name.to_owned(),
            _ => panic!(
                "{}: path dependency `{value}` is outside crates/",
                manifest.display()
            ),
        })
        .collect()
}

/// The entries of the `shell` paths filter in ci.yml.
fn shell_filter(ci: &str) -> Vec<&str> {
    ci.lines()
        .skip_while(|line| *line != "            shell:")
        .skip(1)
        .map_while(|line| line.strip_prefix("              - '")?.strip_suffix('\''))
        .collect()
}

#[test]
fn the_shell_filter_names_every_crate_the_shell_builds_and_every_file_these_tests_read() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("src-tauri sits in the repository root");
    let mut reached = BTreeSet::new();
    let mut todo = path_dependencies(&root.join("src-tauri/Cargo.toml"), "../crates/");
    while let Some(name) = todo.pop() {
        let manifest = root.join("crates").join(&name).join("Cargo.toml");
        if reached.insert(name) {
            todo.extend(path_dependencies(&manifest, "../"));
        }
    }
    // A new local crate goes in this list and in the `shell` filter.
    assert_eq!(
        reached.iter().map(String::as_str).collect::<Vec<_>>(),
        [
            "grokhub-agent",
            "grokhub-code",
            "grokhub-core",
            "grokhub-desktop",
            "grokhub-fps-probe",
            "grokhub-hub",
            "grokhub-proto",
        ]
    );

    let filter = shell_filter(include_str!("../../.github/workflows/ci.yml"));
    for name in &reached {
        let entry = format!("crates/{name}/**");
        assert!(
            filter.contains(&entry.as_str()),
            "the shell filter in ci.yml has no `{entry}`, so a change there skips the shell job"
        );
    }
    for entry in [
        // The sidecar that scripts/stage-hub-sidecar.mjs builds into the bundles.
        "crates/grokhub-hub/**",
        // The files this test file reads from outside src-tauri.
        ".github/workflows/ci.yml",
        ".github/workflows/release.yml",
        "scripts/stage-hub-sidecar.mjs",
    ] {
        assert!(
            filter.contains(&entry),
            "the shell filter in ci.yml has no `{entry}`"
        );
    }
}
