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
    let (ws, mut ed) = project("open");
    assert_eq!(ed.pick("src/a.rs"), Pick::Opened);
    let open = ed.open.clone().unwrap();
    assert_eq!(open.path, "src/a.rs");
    assert_eq!(open.root, ws.to_string_lossy());
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

#[cfg(unix)]
#[test]
fn a_save_is_refused_when_the_project_folder_became_another_one() {
    let (first, _) = project("swap-first");
    let (second, _) = project("swap-second");
    let config = temp("swap-config");
    // The cabin's project folder is a link, and the link is moved to another
    // project while the file is open.
    let link = temp("swap-link").join("project");
    std::os::unix::fs::symlink(&first, &link).unwrap();
    let mut ed = CodeEditor::default();
    ed.set_workspace(&link);
    assert_eq!(ed.pick("src/a.rs"), Pick::Opened);
    assert_eq!(ed.open.clone().unwrap().root, first.to_string_lossy());
    ed.open.as_mut().unwrap().text = "fn a() { 7 }\n".into();
    std::fs::remove_file(&link).unwrap();
    std::os::unix::fs::symlink(&second, &link).unwrap();
    assert!(ed.save(&config, Some(activity::KEEP_DEFAULT)));
    finish_save(&mut ed);
    assert_eq!(
        ed.note.as_deref(),
        Some("The project folder changed since this file was opened. Nothing was saved.")
    );
    assert!(ed.open.clone().unwrap().dirty());
    for ws in [&first, &second] {
        assert_eq!(
            std::fs::read_to_string(ws.join("src/a.rs")).unwrap(),
            "fn a() {}\n"
        );
    }
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

// ---- in the window, lists and reads run off the UI thread ----

/// A project like `project`, with the editor set up as the window sets it
/// up: lists and reads go to a worker and come back through `poll`.
fn windowed(name: &str) -> (PathBuf, CodeEditor) {
    let ws = temp(name);
    std::fs::create_dir_all(ws.join("src")).unwrap();
    std::fs::write(ws.join("src/a.rs"), "fn a() {}\n").unwrap();
    std::fs::write(ws.join("src/b.rs"), "fn b() {}\n").unwrap();
    let mut ed = CodeEditor::default();
    ed.wake_with(|| {});
    (ws, ed)
}

/// Poll until `done` holds (or 10 s pass).
fn poll_until(ed: &mut CodeEditor, done: impl Fn(&CodeEditor) -> bool) -> bool {
    for _ in 0..1000 {
        ed.poll();
        if done(ed) {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    false
}

fn read_of(ws: &Path, ask: u64, path: &str, text: &str) -> Answer {
    Answer::Read {
        ask,
        workspace: ws.to_path_buf(),
        discard: false,
        path: path.into(),
        result: Ok(FileText {
            path: path.into(),
            text: text.into(),
            root: ws.to_string_lossy().into_owned(),
        }),
    }
}

#[test]
fn in_the_window_a_list_and_a_read_come_back_through_poll() {
    let (ws, mut ed) = windowed("off-thread");
    // The call returns before the folder is listed.
    ed.set_workspace(&ws);
    assert!(ed.listing.is_none(), "the list ran on the calling thread");
    assert!(ed.listing_out);
    assert!(poll_until(&mut ed, |e| e.listing.is_some()));
    assert!(!ed.listing_out);
    let names: Vec<String> = ed
        .listing
        .clone()
        .unwrap()
        .entries
        .into_iter()
        .map(|e| e.name)
        .collect();
    assert_eq!(names, vec!["src"]);

    // So does a pick: the file opens when its read lands.
    assert_eq!(ed.pick("src/a.rs"), Pick::Loading);
    assert!(ed.open.is_none(), "the read ran on the calling thread");
    assert!(poll_until(&mut ed, |e| e.open.is_some()));
    let open = ed.open.clone().unwrap();
    assert_eq!(open.path, "src/a.rs");
    assert_eq!(open.text, "fn a() {}\n");
    assert_eq!(open.workspace, ws);
    assert_eq!(ed.pick("src/a.rs"), Pick::Same);

    // A refusal arrives the same way.
    assert_eq!(ed.pick("../secret.txt"), Pick::Loading);
    assert!(poll_until(&mut ed, |e| e.note.is_some()));
    assert_eq!(ed.note.as_deref(), Some(files::OUTSIDE));
    assert_eq!(ed.open.clone().unwrap().path, "src/a.rs");
}

#[test]
fn only_the_newest_pick_opens_and_another_projects_answer_is_dropped() {
    let (ws, mut ed) = windowed("stale");
    ed.set_workspace(&ws);
    assert!(poll_until(&mut ed, |e| e.listing.is_some()));
    // Two picks in a row; the first read is the slow one.
    assert_eq!(ed.pick("src/a.rs"), Pick::Loading);
    assert_eq!(ed.pick("src/b.rs"), Pick::Loading);
    assert_eq!(ed.read_ask, 2);
    assert_eq!(
        ed.apply(read_of(&ws, 2, "src/b.rs", "fn b() {}\n")),
        Some(Pick::Opened)
    );
    assert_eq!(ed.apply(read_of(&ws, 1, "src/a.rs", "fn a() {}\n")), None);
    assert_eq!(ed.open.clone().unwrap().path, "src/b.rs");

    // An answer read in a project folder the editor has left.
    let other = temp("stale-other");
    assert_eq!(
        ed.apply(read_of(&other, 2, "src/a.rs", "from the other project\n")),
        None
    );
    assert_eq!(ed.open.clone().unwrap().text, "fn b() {}\n");
    let listed = Answer::Listed {
        ask: ed.list_ask,
        workspace: other,
        result: Ok(Listing {
            path: String::new(),
            entries: Vec::new(),
            more: false,
        }),
    };
    assert_eq!(ed.apply(listed), None);
    assert_eq!(ed.listing.clone().unwrap().entries.len(), 1);

    // An older list is dropped too: the tree shows the folder asked for last.
    ed.browse("src");
    ed.browse("");
    let older = Answer::Listed {
        ask: ed.list_ask - 1,
        workspace: ws.clone(),
        result: Ok(Listing {
            path: "src".into(),
            entries: Vec::new(),
            more: false,
        }),
    };
    assert_eq!(ed.apply(older), None);
    assert_eq!(ed.listing.clone().unwrap().path, "");
}

#[test]
fn edits_typed_while_a_read_was_out_are_not_replaced() {
    let (ws, mut ed) = windowed("typed-meanwhile");
    ed.set_workspace(&ws);
    assert_eq!(ed.pick("src/a.rs"), Pick::Loading);
    assert!(poll_until(&mut ed, |e| e.open.is_some()));
    // b.rs is picked, and a.rs is typed into before b.rs has been read.
    assert_eq!(ed.pick("src/b.rs"), Pick::Loading);
    ed.open
        .as_mut()
        .unwrap()
        .text
        .push_str("// typed meanwhile\n");
    let ask = ed.read_ask;
    assert_eq!(
        ed.apply(read_of(&ws, ask, "src/b.rs", "fn b() {}\n")),
        Some(Pick::Asked)
    );
    let open = ed.open.clone().unwrap();
    assert_eq!(open.path, "src/a.rs");
    assert_eq!(open.text, "fn a() {}\n// typed meanwhile\n");
    assert_eq!(ed.pending.as_deref(), Some("src/b.rs"));
    // "Discard and open" is the one read that replaces unsaved edits.
    assert_eq!(ed.discard_and_open(), Pick::Loading);
    assert!(poll_until(&mut ed, |e| e
        .open
        .as_ref()
        .is_some_and(|f| f.path == "src/b.rs")));
    assert_eq!(ed.open.clone().unwrap().text, "fn b() {}\n");
}
