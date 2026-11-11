use super::*;

fn temp_root(tag: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!(
        "grokhub-onboard-ui-{tag}-{}-{}",
        std::process::id(),
        grokhub_core::now_ms()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("memory")).expect("dir");
    root
}

fn read(root: &std::path::Path, name: &str) -> String {
    std::fs::read_to_string(root.join("memory").join(name)).unwrap_or_default()
}

#[test]
fn the_stage_waits_for_the_setup_wizard_and_opens_after_it() {
    let root = temp_root("after");
    let _pin = crate::config::TestConfigDir::set(root.clone());
    let mut app = Cabin::quiet_for_test();
    app.tick_setup_wizard();
    // The wizard is up; Get to know you is not.
    assert_eq!(app.setup.step, Some(super::SetupStep::Welcome));
    assert!(!app.setup.onboard.open);
    app.finish_setup(false);
    assert_eq!(app.setup.step, None);
    assert!(app.setup.onboard.open);
    assert_eq!(app.setup.onboard.step, Step::Welcome);
    // The voice auto setup picked is where the soul step starts.
    assert_eq!(app.setup.onboard.answers.tone, "friendly");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn save_with_amr_on_writes_the_three_files_rows_and_the_name() {
    // No app.json yet: a new install, so AMR is on.
    let root = temp_root("amr-on");
    let _pin = crate::config::TestConfigDir::set(root.clone());
    let _agent = grokhub_agent::perm::ConfigGuard::set(root.clone());
    let mut app = Cabin::quiet_for_test();
    app.open_onboard(false);
    assert!(app.setup.onboard.open);
    app.onboard_go(Step::About);
    app.setup.onboard.answers.name = "Sam Example".into();
    app.setup.onboard.answers.call_me = "Sam".into();
    app.onboard_go(Step::Soul);
    app.setup.onboard.answers.tone = "direct".into();
    app.onboard_go(Step::Life);
    app.setup.onboard.answers.routines = "Gym before work".into();
    // Quit part way: the next launch picks up on the same step.
    let mut again = Cabin::quiet_for_test();
    again.open_onboard(false);
    assert_eq!(again.setup.onboard.step, Step::Life);
    assert_eq!(again.setup.onboard.answers.call_me, "Sam");
    app.onboard_go(Step::Review);
    let d = app.setup.onboard.drafts.clone().expect("drafts");
    assert_eq!(d.user, "# User\n\n- Name: Sam Example\n- Call me: Sam\n");
    assert_eq!(d.rows.len(), 3);
    app.onboard_save();
    assert_eq!(app.setup.onboard.step, Step::Done);
    let saved = app.setup.onboard.saved.clone().expect("saved");
    assert_eq!((saved.rows, saved.amr), (3, true));
    assert_eq!(
        saved_line(&saved),
        "Saved USER.md, SOUL.md, MEMORY.md and 3 Personal rows."
    );
    assert_eq!(
        read(&root, "USER.md"),
        "# User\n\n- Name: Sam Example\n- Call me: Sam\n"
    );
    assert_eq!(
        read(&root, "SOUL.md"),
        "# Soul\n\n- Tone: direct. Short answers, the point first, no filler.\n"
    );
    assert!(
        read(&root, "MEMORY.md").ends_with(": Routines: Gym before work\n"),
        "{}",
        read(&root, "MEMORY.md")
    );
    assert_eq!(app.cfg.display_name, "Sam Example");
    // It does not open on its own again.
    let mut next = Cabin::quiet_for_test();
    next.open_onboard(false);
    assert!(!next.setup.onboard.open);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn save_with_amr_off_writes_no_rows() {
    let root = temp_root("amr-off");
    std::fs::write(root.join("app.json"), "{}\n").unwrap();
    let _pin = crate::config::TestConfigDir::set(root.clone());
    let _agent = grokhub_agent::perm::ConfigGuard::set(root.clone());
    let mut app = Cabin::quiet_for_test();
    app.open_onboard(false);
    app.setup.onboard.answers.call_me = "Sam".into();
    app.onboard_go(Step::Review);
    app.onboard_save();
    let saved = app.setup.onboard.saved.clone().expect("saved");
    assert_eq!((saved.rows, saved.amr), (0, false));
    assert_eq!(saved_line(&saved), "Saved USER.md, SOUL.md.");
    assert_eq!(read(&root, "USER.md"), "# User\n\n- Call me: Sam\n");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn review_says_which_notes_file_save_will_leave_alone() {
    let root = temp_root("review-note");
    std::fs::write(root.join("app.json"), "{}\n").unwrap();
    // A folder where SOUL.md should be: there is no text to show.
    std::fs::create_dir_all(root.join("memory").join("SOUL.md")).unwrap();
    let _pin = crate::config::TestConfigDir::set(root.clone());
    let _agent = grokhub_agent::perm::ConfigGuard::set(root.clone());
    let mut app = Cabin::quiet_for_test();
    app.open_onboard(false);
    assert_eq!(app.setup.onboard.note, None);
    app.onboard_go(Step::Review);
    assert_eq!(
        app.setup.onboard.note.as_deref(),
        Some("SOUL.md is not a file; Save leaves it alone.")
    );
    // Back: the note is Review's, so it goes.
    app.onboard_go(Step::Life);
    assert_eq!(app.setup.onboard.note, None);
    app.onboard_go(Step::Review);
    // Quit on Review: the next launch opens there and says it again.
    let mut again = Cabin::quiet_for_test();
    again.open_onboard(false);
    assert_eq!(again.setup.onboard.step, Step::Review);
    assert_eq!(
        again.setup.onboard.note.as_deref(),
        Some("SOUL.md is not a file; Save leaves it alone.")
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn save_names_a_notes_file_that_is_not_text_and_leaves_its_bytes_alone() {
    let root = temp_root("not-text");
    std::fs::write(root.join("app.json"), "{}\n").unwrap();
    // Old notes saved in another encoding: not valid UTF-8.
    let memory = root.join("memory").join("MEMORY.md");
    let bytes = [0xff, 0xfe, b'o', 0x00, b'l', 0x00, b'd', 0x00];
    std::fs::write(&memory, bytes).unwrap();
    let _pin = crate::config::TestConfigDir::set(root.clone());
    let _agent = grokhub_agent::perm::ConfigGuard::set(root.clone());
    let mut app = Cabin::quiet_for_test();
    app.open_onboard(false);
    app.setup.onboard.answers.call_me = "Sam".into();
    app.setup.onboard.answers.routines = "Gym before work".into();
    app.onboard_go(Step::Review);
    // The person reviewed a MEMORY.md draft, so Save was asked to write it.
    let d = app.setup.onboard.drafts.clone().expect("drafts");
    assert!(
        d.memory.ends_with(": Routines: Gym before work\n"),
        "{}",
        d.memory
    );
    app.onboard_save();
    assert_eq!(app.setup.onboard.step, Step::Done);
    let saved = app.setup.onboard.saved.clone().expect("saved");
    assert_eq!(saved.files, ["USER.md", "SOUL.md"]);
    assert_eq!(saved.skipped, ["MEMORY.md"]);
    assert_eq!(
        done_line(&app.setup.onboard),
        "Saved USER.md, SOUL.md. MEMORY.md is not plain text (UTF-8); it was left alone."
    );
    assert_eq!(std::fs::read(&memory).unwrap(), bytes);
    assert!(!root.join("memory").join("MEMORY.md.prev").exists());
    assert_eq!(read(&root, "USER.md"), "# User\n\n- Call me: Sam\n");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn save_that_wrote_nothing_still_names_the_file_it_left_alone() {
    let root = temp_root("too-big");
    std::fs::write(root.join("app.json"), "{}\n").unwrap();
    let memory = root.join("memory").join("MEMORY.md");
    let big = "a line of old notes\n".repeat(52_500);
    assert_eq!(big.len(), 1_050_000);
    std::fs::write(&memory, &big).unwrap();
    let _pin = crate::config::TestConfigDir::set(root.clone());
    let _agent = grokhub_agent::perm::ConfigGuard::set(root.clone());
    let mut app = Cabin::quiet_for_test();
    app.open_onboard(false);
    app.onboard_go(Step::Review);
    assert_eq!(
        app.setup.onboard.note.as_deref(),
        Some("MEMORY.md is too big to change here; Save leaves it alone.")
    );
    // The person empties the other two boxes and types one note.
    app.setup.onboard.drafts = Some(Drafts {
        memory: "# Memory\n\n- Keep this\n".into(),
        ..Drafts::default()
    });
    app.onboard_save();
    let saved = app.setup.onboard.saved.clone().expect("saved");
    assert_eq!(saved.files, Vec::<String>::new());
    assert_eq!(saved.skipped, ["MEMORY.md"]);
    assert_eq!(
        done_line(&app.setup.onboard),
        "Nothing changed. MEMORY.md is too big to change here; it was left alone."
    );
    assert_eq!(std::fs::read_to_string(&memory).unwrap(), big);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_save_that_left_nothing_alone_says_only_what_it_saved() {
    let root = temp_root("plain-save");
    std::fs::write(root.join("app.json"), "{}\n").unwrap();
    let _pin = crate::config::TestConfigDir::set(root.clone());
    let _agent = grokhub_agent::perm::ConfigGuard::set(root.clone());
    let mut app = Cabin::quiet_for_test();
    app.open_onboard(false);
    app.setup.onboard.answers.call_me = "Sam".into();
    app.onboard_go(Step::Review);
    assert_eq!(app.setup.onboard.note, None);
    app.onboard_save();
    assert_eq!(app.setup.onboard.note, None);
    assert_eq!(done_line(&app.setup.onboard), "Saved USER.md, SOUL.md.");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn adding_from_openclaw_says_when_the_notes_are_already_there() {
    let root = temp_root("openclaw");
    std::fs::write(root.join("app.json"), "{}\n").unwrap();
    std::fs::write(root.join("memory").join("MEMORY.md"), [0xff, 0xfe]).unwrap();
    let home = root.join("home");
    let ws = home.join(".openclaw").join("workspace");
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::write(ws.join("SOUL.md"), "Be calm.\n").unwrap();
    let _pin = crate::config::TestConfigDir::set(root.clone());
    let _agent = grokhub_agent::perm::ConfigGuard::set(root.clone());
    let mut app = Cabin::quiet_for_test();
    app.open_onboard(false);
    app.setup.onboard.answers.tone.clear();
    app.onboard_go(Step::Review);
    app.onboard_openclaw(&home);
    let first = app.setup.onboard.drafts.clone().expect("drafts");
    assert_eq!(
        first.soul,
        "# Soul\n\n## From OpenClaw SOUL.md\n\nBe calm.\n"
    );
    // The import had nothing to say; Review's own note stays.
    assert_eq!(
        app.setup.onboard.note.as_deref(),
        Some("MEMORY.md is not plain text (UTF-8); Save leaves it alone.")
    );
    // Pressed again: nothing to add, and it says so.
    app.onboard_openclaw(&home);
    assert_eq!(app.setup.onboard.drafts, Some(first));
    assert_eq!(
        app.setup.onboard.note.as_deref(),
        Some(
            "MEMORY.md is not plain text (UTF-8); Save leaves it alone. \
             The OpenClaw notes are already in these files; nothing was added."
        )
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn skip_closes_it_and_personal_opens_it_again_over_real_files() {
    let root = temp_root("skip");
    let _pin = crate::config::TestConfigDir::set(root.clone());
    let mut app = Cabin::quiet_for_test();
    app.open_onboard(false);
    app.onboard_skip();
    assert!(!app.setup.onboard.open);
    assert_eq!(
        app.status,
        "Get to know you skipped. Settings → Personal opens it again."
    );
    std::fs::write(
        root.join("memory").join("USER.md"),
        "# User\n\n- Likes tea\n",
    )
    .unwrap();
    app.open_onboard(false);
    assert!(!app.setup.onboard.open);
    app.open_onboard(true);
    assert!(app.setup.onboard.open);
    app.onboard_go(Step::Review);
    // The drafts start from what is written; nothing is lost.
    let d = app.setup.onboard.drafts.clone().expect("drafts");
    assert!(d.user.starts_with("# User\n\n- Likes tea\n"), "{}", d.user);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn v1_offers_no_chat_export_or_account_import() {
    let src = include_str!("onboard_ui.rs").to_ascii_lowercase();
    for word in [
        "chat export",
        "export your",
        "connected account",
        "sign in",
        "gmail",
        "oauth",
    ] {
        assert!(!src.contains(word), "{word}");
    }
    // The only import is the optional OpenClaw one.
    assert_eq!(src.matches("add from openclaw").count(), 1);
}
