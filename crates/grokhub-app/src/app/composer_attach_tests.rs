//! Composer, plus menu, file pick and attach tests (rearch stage 5).
//!
//! Mechanical move from `tests.rs` Cursor fold. Wired via `#[path]` so
//! `app/mod.rs` stays untouched.

use super::*;
use eframe::egui;

// ---- Cursor fold: Composer, plus menu, file pick and attach ----

// Folded from PR #251.
#[test]
fn blank_composer_send_stays_off_a_run() {
    let mut app = Cabin::quiet_for_test();
    app.chat_tail_frames = 0;
    app.send_from_composer("   ".into());
    assert_eq!(app.chat_tail_frames, 3);
    assert!(app.messages.is_empty());
    assert!(!app.running);
    assert!(app.chat_job_thread.is_none());
    assert!(!app.turn_retried);
}

// Folded from PR #252.
#[test]
fn open_plus_menu_stays_off_a_run() {
    let mut app = Cabin::quiet_for_test();
    app.open_plus(PlusTarget::Chat, egui::pos2(4.0, 8.0));
    assert!(matches!(app.plus_menu, Some(PlusTarget::Chat)));
    assert_eq!(app.plus_anchor, egui::pos2(4.0, 8.0));
    assert!(app.plus_ignore_close);
    assert!(app.file_pick.is_none());
    assert!(!app.running);
    assert!(app.chat_job_thread.is_none());
}

// Folded from PR #256.
#[test]
fn cancelled_file_pick_clears_the_status() {
    let mut app = Cabin::quiet_for_test();
    app.status = "Harbor".into();
    app.poll_pick();
    assert_eq!(app.status, "Harbor");
    assert!(app.pick_rx.is_none());

    app.status = "Choose a file…".into();
    let (tx, rx) = std::sync::mpsc::channel::<(PlusTarget, PlusPick)>();
    drop(tx);
    app.pick_rx = Some(rx);
    app.poll_pick();
    assert!(app.pick_rx.is_none());
    assert!(app.status.is_empty());
    assert!(!app.running);
    assert!(app.chat_job_thread.is_none());
}

// Folded from PR #258.
/// A pick already in flight must not start another file dialog or clipboard read.
#[test]
fn plus_act_while_busy_stays_off_a_run() {
    let mut app = Cabin::quiet_for_test();
    let (_tx, rx) = std::sync::mpsc::channel::<(PlusTarget, PlusPick)>();
    app.pick_rx = Some(rx);
    app.run_plus_act(PlusTarget::Chat, PlusAct::Upload);
    assert_eq!(app.status, "Choose a file…");
    assert!(app.pick_rx.is_some());
    assert!(!app.running);
    assert!(app.chat_job_thread.is_none());
    let (_tx2, rx2) = std::sync::mpsc::channel::<(PlusTarget, PlusPick)>();
    app.pick_rx = Some(rx2);
    app.run_plus_act(PlusTarget::Imagine, PlusAct::Paste);
    assert_eq!(app.status, "Reading clipboard…");
    assert!(app.pick_rx.is_some());
    assert!(!app.running);
    assert!(app.chat_job_thread.is_none());
}

// Folded from PR #260.
#[test]
fn dropped_read_clears_the_status() {
    let mut app = Cabin::quiet_for_test();
    app.status = "Harbor".into();
    let (tx, rx) = std::sync::mpsc::channel::<(PlusTarget, PlusPick)>();
    drop(tx);
    app.pick_rx = Some(rx);
    app.poll_pick();
    assert!(app.pick_rx.is_none());
    assert_eq!(app.status, "Harbor");
    assert!(!app.running);
    assert!(app.chat_job_thread.is_none());

    app.status = "Reading clipboard…".into();
    let (tx, rx) = std::sync::mpsc::channel::<(PlusTarget, PlusPick)>();
    drop(tx);
    app.pick_rx = Some(rx);
    app.poll_pick();
    assert!(app.pick_rx.is_none());
    assert!(app.status.is_empty());
    assert!(!app.running);
    assert!(app.chat_job_thread.is_none());

    app.status = "Reading file…".into();
    let (tx, rx) = std::sync::mpsc::channel::<(PlusTarget, PlusPick)>();
    drop(tx);
    app.pick_rx = Some(rx);
    app.poll_pick();
    assert!(app.pick_rx.is_none());
    assert!(app.status.is_empty());
    assert!(!app.running);
    assert!(app.chat_job_thread.is_none());
}

// Folded from PR #262.
#[test]
fn plus_path_while_busy_stays_off_a_run() {
    let mut app = Cabin::quiet_for_test();
    let (_tx, rx) = std::sync::mpsc::channel::<(PlusTarget, PlusPick)>();
    app.pick_rx = Some(rx);
    app.status = "Harbor".into();
    app.start_plus_path(PlusTarget::Chat, std::path::PathBuf::from("harbor.png"));
    assert_eq!(app.status, "Reading file…");
    assert!(app.pick_rx.is_some());
    assert!(!app.running);
    assert!(app.chat_job_thread.is_none());
    app.start_plus_path(PlusTarget::Imagine, std::path::PathBuf::from("sunset.png"));
    assert_eq!(app.status, "Reading file…");
    assert!(app.pick_rx.is_some());
    assert!(!app.running);
    assert!(app.chat_job_thread.is_none());
}

// Folded from PR #278.
#[test]
fn file_list_poll_keeps_the_matching_dir() {
    let mut app = Cabin::quiet_for_test();
    app.pick_dir = "/tmp/harbor".into();
    app.poll_pick_list();
    assert!(app.pick_list_rx.is_none());
    assert!(app.pick_cache.is_none());

    let (tx, rx) = std::sync::mpsc::channel();
    app.pick_list_rx = Some(rx);
    app.poll_pick_list();
    assert!(app.pick_list_rx.is_some());
    assert!(app.pick_cache.is_none());
    assert_eq!(app.status, "");

    tx.send(("/tmp/other".into(), vec![("a.txt".into(), false)]))
        .unwrap();
    app.poll_pick_list();
    assert!(app.pick_list_rx.is_none());
    assert!(
        app.pick_cache.is_none(),
        "a different dir must not fill the cache"
    );

    let (tx, rx) = std::sync::mpsc::channel();
    app.pick_list_rx = Some(rx);
    tx.send(("/tmp/harbor".into(), vec![("shot.png".into(), false)]))
        .unwrap();
    app.poll_pick_list();
    assert!(app.pick_list_rx.is_none());
    let (dir, entries) = app
        .pick_cache
        .as_ref()
        .expect("matching dir fills the cache");
    assert_eq!(dir, "/tmp/harbor");
    assert_eq!(entries, &vec![("shot.png".to_string(), false)]);

    let (tx, rx) = std::sync::mpsc::channel::<(String, Vec<(String, bool)>)>();
    drop(tx);
    app.pick_list_rx = Some(rx);
    app.poll_pick_list();
    assert!(app.pick_list_rx.is_none());
    assert!(
        app.pick_cache.is_some(),
        "a dropped list must not clear a filled cache"
    );
    assert!(!app.running);
    assert!(app.chat_job_thread.is_none());
}

// Folded from PR #333.
#[test]
fn clear_chat_attach_drops_pending_files() {
    let mut cabin = Cabin::quiet_for_test();
    cabin.status = "Harbor".into();
    cabin.attach_url = Some("data:image/png;base64,aGFyYm9y".into());
    cabin.attach_name = Some("harbor.png".into());
    cabin.attach_path = Some("/tmp/harbor.png".into());
    cabin.attach_kind = Some(grokhub_core::AttachKind::Image);
    cabin.clear_chat_attach();
    assert!(cabin.attach_url.is_none());
    assert!(cabin.attach_name.is_none());
    assert!(cabin.attach_path.is_none());
    assert!(cabin.attach_kind.is_none());
    assert!(cabin.status.is_empty());
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
}

// Folded from PR #334.
#[test]
fn drop_leaving_thread_chrome_clears_edit_state() {
    let mut cabin = Cabin::quiet_for_test();
    cabin.running = false;
    cabin.attach_name = Some("shot.png".into());
    cabin.attach_url = Some("file://shot.png".into());
    cabin.attach_path = Some("/tmp/shot.png".into());
    cabin.attach_kind = Some(grokhub_core::AttachKind::Image);
    cabin.followup_step = 3;
    cabin.active_skill_follow = Some("follow".into());
    cabin.hands_attach = true;
    cabin.eyes_attach = true;
    cabin.last_receipt_ok = Some(true);
    cabin.elicit_draft = "typed".into();
    cabin.tool_cards.push(grokhub_core::wire::ToolCard {
        id: "t1".into(),
        title: "grep".into(),
        kind: String::new(),
        status: "completed".into(),
        detail: String::new(),
        diff: String::new(),
        image_data_url: None,
        raw_input: String::new(),
    });
    cabin.live_blocks.push(grokhub_core::LiveBlock {
        kind: grokhub_core::LiveKind::Say,
        body: "hello".into(),
        tool_id: String::new(),
        tool_title: String::new(),
        tool_status: String::new(),
        tool_detail: String::new(),
        fold_slot: 1,
    });

    cabin.drop_leaving_thread_chrome();

    assert!(cabin.attach_name.is_none());
    assert!(cabin.attach_url.is_none());
    assert!(cabin.attach_path.is_none());
    assert!(cabin.attach_kind.is_none());
    assert_eq!(cabin.followup_step, 0);
    assert!(cabin.active_skill_follow.is_none());
    assert!(!cabin.hands_attach);
    assert!(!cabin.eyes_attach);
    assert!(cabin.last_receipt_ok.is_none());
    assert!(cabin.elicit_draft.is_empty());
    assert!(cabin.tool_cards.is_empty());
    assert!(cabin.live_blocks.is_empty());
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
}

// Folded from PR #413.
#[test]
fn drop_hint_line_matches_shipped_wording() {
    use super::super::plus::drop_hint_line;
    assert_eq!(
        drop_hint_line(false, 1),
        "Drop to attach to your next message"
    );
    assert_eq!(
        drop_hint_line(true, 1),
        "Drop to use as the Imagine reference"
    );
    assert_eq!(
        drop_hint_line(false, 3),
        "Drop to attach to your next message — 3 files"
    );
    assert_eq!(
        drop_hint_line(true, 3),
        "Drop to use as the Imagine reference — 3 files"
    );
}
