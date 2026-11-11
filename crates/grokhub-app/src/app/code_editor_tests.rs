use super::*;
use grokhub_code::activity;

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "cabin-code-editor-{name}-{}-{}",
        std::process::id(),
        grokhub_core::now_ms()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

/// A project with `src/a.rs` and `src/b.rs`, and the editor pointed at it.
fn project(name: &str) -> (PathBuf, CodeEditor) {
    let ws = temp(name);
    std::fs::create_dir_all(ws.join("src")).unwrap();
    std::fs::write(ws.join("src/a.rs"), "fn a() {}\n").unwrap();
    std::fs::write(ws.join("src/b.rs"), "fn b() {}\n").unwrap();
    let mut ed = CodeEditor::default();
    ed.set_workspace(&ws);
    (ws, ed)
}

/// Wait for a started save to come back.
fn finish_save(ed: &mut CodeEditor) {
    for _ in 0..500 {
        if ed.poll_save().is_some() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("the save never finished");
}

#[test]
fn languages_follow_the_extension() {
    assert_eq!(language_for("src/lib.rs"), "rs");
    assert_eq!(language_for("tools/gen.py"), "py");
    assert_eq!(language_for("native/x.HPP"), "cpp");
    assert_eq!(language_for("ui/App.tsx"), "cpp");
    assert_eq!(language_for("package.json"), "cpp");
    assert_eq!(language_for("Cargo.toml"), "toml");
    assert_eq!(language_for("ci/run.sh"), "toml");
    assert_eq!(language_for("README.md"), "");
    assert_eq!(language_for("notes.txt"), "");
    assert_eq!(language_for("Makefile"), "");
    assert_eq!(language_for(".gitignore"), "");
    assert_eq!(language_for("dir.rs/README"), "");
}

#[test]
fn parent_and_gutter() {
    assert_eq!(parent_of("src/app/mod.rs"), "src/app");
    assert_eq!(parent_of("src"), "");
    assert_eq!(parent_of(""), "");
    assert_eq!(gutter(""), "1");
    assert_eq!(gutter("one"), "1");
    assert_eq!(gutter("a\nb\n"), "1\n2\n3");
    assert_eq!(
        gutter(&"x\n".repeat(10)),
        " 1\n 2\n 3\n 4\n 5\n 6\n 7\n 8\n 9\n10\n11"
    );
    assert_eq!(discard_label("src/b.rs"), "Discard and open src/b.rs");
}

#[test]
fn lists_the_project_root_folders_first_without_dot_git() {
    let (ws, mut ed) = project("list");
    std::fs::create_dir_all(ws.join(".git")).unwrap();
    std::fs::write(ws.join("Cargo.toml"), "[package]\n").unwrap();
    ed.browse("");
    let listing = ed.listing.clone().unwrap();
    let names: Vec<&str> = listing.entries.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, vec!["src", "Cargo.toml"]);
    ed.browse("src");
    let listing = ed.listing.clone().unwrap();
    assert_eq!(listing.path, "src");
    let paths: Vec<&str> = listing.entries.iter().map(|e| e.path.as_str()).collect();
    assert_eq!(paths, vec!["src/a.rs", "src/b.rs"]);
}

#[test]
fn opens_a_file_inside_the_project() {
    let (_ws, mut ed) = project("open");
    assert_eq!(ed.pick("src/a.rs"), Pick::Opened);
    let open = ed.open.clone().unwrap();
    assert_eq!(open.path, "src/a.rs");
    assert_eq!(open.text, "fn a() {}\n");
    assert!(!open.dirty());
    assert_eq!(ed.pick("src/a.rs"), Pick::Same);
}

#[test]
fn refuses_paths_outside_the_project_and_dot_git() {
    let (ws, mut ed) = project("outside");
    std::fs::create_dir_all(ws.join(".git")).unwrap();
    std::fs::write(ws.join(".git/config"), "[core]\n").unwrap();
    assert_eq!(ed.pick("../secret.txt"), Pick::Refused);
    assert_eq!(ed.note.as_deref(), Some(files::OUTSIDE));
    assert_eq!(ed.pick("src/../../secret.txt"), Pick::Refused);
    assert_eq!(ed.note.as_deref(), Some(files::OUTSIDE));
    assert_eq!(ed.pick(".git/config"), Pick::Refused);
    assert_eq!(ed.note.as_deref(), Some(files::GIT_DIR));
    assert!(ed.open.is_none());
}

#[cfg(unix)]
#[test]
fn refuses_a_symlink_that_leads_out_of_the_project() {
    let (ws, mut ed) = project("symlink");
    let outside = temp("symlink-target");
    std::fs::write(outside.join("key.txt"), "do not show").unwrap();
    std::os::unix::fs::symlink(outside.join("key.txt"), ws.join("link.txt")).unwrap();
    assert_eq!(ed.pick("link.txt"), Pick::Refused);
    assert_eq!(ed.note.as_deref(), Some(files::OUTSIDE));
    assert!(ed.open.is_none());
}

#[test]
fn refuses_an_oversize_or_binary_file() {
    let (ws, mut ed) = project("big");
    std::fs::write(ws.join("big.txt"), "a".repeat(files::MAX_BYTES + 1)).unwrap();
    std::fs::write(ws.join("blob.bin"), [0xff_u8, 0xfe, 0x00, 0x81]).unwrap();
    assert_eq!(ed.pick("big.txt"), Pick::Refused);
    assert_eq!(ed.note.as_deref(), Some(files::TOO_BIG));
    assert_eq!(ed.pick("blob.bin"), Pick::Refused);
    assert_eq!(ed.note.as_deref(), Some(files::NOT_TEXT));
}

#[test]
fn unsaved_edits_ask_before_another_file_opens() {
    let (_ws, mut ed) = project("dirty");
    ed.pick("src/a.rs");
    ed.open.as_mut().unwrap().text.push_str("// edited\n");
    assert_eq!(ed.pick("src/b.rs"), Pick::Asked);
    assert_eq!(ed.pending.as_deref(), Some("src/b.rs"));
    // Keep editing: the edits and the open file stay.
    ed.keep_editing();
    assert_eq!(ed.pending, None);
    let open = ed.open.clone().unwrap();
    assert_eq!(open.path, "src/a.rs");
    assert_eq!(open.text, "fn a() {}\n// edited\n");
    // Discard and open: the other file opens, the edits are gone.
    assert_eq!(ed.pick("src/b.rs"), Pick::Asked);
    assert_eq!(ed.discard_and_open(), Pick::Opened);
    let open = ed.open.clone().unwrap();
    assert_eq!(open.path, "src/b.rs");
    assert_eq!(open.text, "fn b() {}\n");
    assert_eq!(ed.discard_and_open(), Pick::Refused);
}

#[test]
fn unsaved_edits_survive_a_tab_switch() {
    let (ws, mut ed) = project("switch");
    ed.pick("src/a.rs");
    ed.open.as_mut().unwrap().text = "fn a() { 1 }\n".into();
    // Leaving the Code tab and coming back draws the editor with the same
    // project folder again; nothing reloads or drops the edits.
    ed.set_workspace(&ws);
    let open = ed.open.clone().unwrap();
    assert_eq!(open.text, "fn a() { 1 }\n");
    assert!(open.dirty());
    // The file on disk is untouched until Save.
    assert_eq!(
        std::fs::read_to_string(ws.join("src/a.rs")).unwrap(),
        "fn a() {}\n"
    );
}

#[test]
fn a_project_change_keeps_unsaved_edits_bound_to_their_own_folder() {
    let (ws, mut ed) = project("move-dirty");
    let other = temp("move-dirty-other");
    ed.pick("src/a.rs");
    ed.open.as_mut().unwrap().text = "fn a() { 2 }\n".into();
    ed.set_workspace(&other);
    let open = ed.open.clone().unwrap();
    assert_eq!(open.workspace, ws);
    assert_eq!(open.text, "fn a() { 2 }\n");
    assert!(ed.listing.clone().unwrap().entries.is_empty());
    // A clean file closes on a project change instead.
    let (_ws2, mut clean) = project("move-clean");
    clean.pick("src/a.rs");
    clean.set_workspace(&other);
    assert!(clean.open.is_none());
}

#[test]
fn save_writes_the_file_and_logs_only_path_and_size() {
    let (ws, mut ed) = project("save");
    let config = temp("save-config");
    ed.pick("src/a.rs");
    // A clean file has nothing to save.
    assert!(!ed.save(&config, Some(activity::KEEP_DEFAULT)));
    let body = "fn a() { let token = \"sk-not-in-the-log\"; }\n";
    ed.open.as_mut().unwrap().text = body.into();
    assert!(ed.save(&config, Some(activity::KEEP_DEFAULT)));
    assert!(ed.is_saving());
    // A second press while the first save runs does nothing.
    assert!(!ed.save(&config, Some(activity::KEEP_DEFAULT)));
    finish_save(&mut ed);
    assert!(!ed.is_saving());
    assert_eq!(std::fs::read_to_string(ws.join("src/a.rs")).unwrap(), body);
    assert!(!ed.open.clone().unwrap().dirty());
    assert_eq!(ed.note.as_deref(), Some("Saved src/a.rs."));
    let log = activity::list(&config, 10);
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].kind, activity::Kind::Edit);
    assert_eq!(
        log[0].summary,
        format!("Saved src/a.rs ({} bytes).", body.len())
    );
    let raw = std::fs::read_to_string(config.join(activity::FILE)).unwrap();
    assert!(!raw.contains("sk-not-in-the-log"));
    assert!(!raw.contains("let token"));
}

#[test]
fn typing_during_a_save_stays_unsaved() {
    let (ws, mut ed) = project("save-race");
    let config = temp("save-race-config");
    ed.pick("src/a.rs");
    ed.open.as_mut().unwrap().text = "fn a() { 3 }\n".into();
    assert!(ed.save(&config, Some(activity::KEEP_DEFAULT)));
    ed.open.as_mut().unwrap().text = "fn a() { 4 }\n".into();
    finish_save(&mut ed);
    let open = ed.open.clone().unwrap();
    assert_eq!(open.saved, "fn a() { 3 }\n");
    assert!(open.dirty());
    assert_eq!(
        std::fs::read_to_string(ws.join("src/a.rs")).unwrap(),
        "fn a() { 3 }\n"
    );
}

#[test]
fn a_refused_save_keeps_the_edits_and_says_why() {
    let (ws, mut ed) = project("save-refused");
    let config = temp("save-refused-config");
    ed.pick("src/a.rs");
    ed.open.as_mut().unwrap().text = "a".repeat(files::MAX_BYTES + 1);
    assert!(ed.save(&config, Some(activity::KEEP_DEFAULT)));
    finish_save(&mut ed);
    assert_eq!(ed.note.as_deref(), Some(files::TOO_BIG));
    assert!(ed.open.clone().unwrap().dirty());
    assert_eq!(
        std::fs::read_to_string(ws.join("src/a.rs")).unwrap(),
        "fn a() {}\n"
    );
    assert!(activity::list(&config, 10).is_empty());
}

#[test]
fn the_ui_save_keeps_as_many_log_lines_as_the_desktop_setting_says() {
    let (_ws, mut ed) = project("save-keep");
    let config = temp("save-keep-config");
    std::fs::write(
        config.join("shell.json"),
        r#"{"codeShip":{"activityKeep":1}}"#,
    )
    .unwrap();
    ed.pick("src/a.rs");
    ed.open.as_mut().unwrap().text = "fn a() { 5 }\n".into();
    assert!(ed.save(&config, None));
    finish_save(&mut ed);
    ed.open.as_mut().unwrap().text = "fn a() { 6 }\n".into();
    assert!(ed.save(&config, None));
    finish_save(&mut ed);
    let log = activity::list(&config, 10);
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].summary, "Saved src/a.rs (13 bytes).");
}
