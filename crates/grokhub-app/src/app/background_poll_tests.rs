//! Background poll tests (rearch stage 5).
//!
//! Mechanical move from `tests.rs`. Wired via `#[path]` so
//! `app/mod.rs` stays untouched.

use super::*;

// ---- Cursor fold: Background polls: empty keeps the receiver, a drop clears it ----

// Folded from PR #270.
#[test]
fn session_show_poll_while_idle_stays_off_a_run() {
    let mut app = Cabin::quiet_for_test();
    app.poll_session_show();
    assert!(app.session_show_rx.is_none());
    assert!(app.messages.is_empty());
    assert!(!app.running);
    assert!(app.chat_job_thread.is_none());

    let (tx, rx) = std::sync::mpsc::channel::<String>();
    app.session_show_rx = Some(("sess".into(), rx));
    app.poll_session_show();
    assert!(app.session_show_rx.is_some());
    assert!(app.messages.is_empty());
    assert!(!app.running);
    assert!(app.chat_job_thread.is_none());

    drop(tx);
    app.poll_session_show();
    assert!(app.session_show_rx.is_none());
    assert!(app.messages.is_empty());
    assert!(!app.running);
    assert!(app.chat_job_thread.is_none());
}

// Folded from PR #279.
#[test]
fn imagine_save_poll_reports_the_path() {
    let mut app = Cabin::quiet_for_test();
    app.status = "Harbor".into();
    app.poll_imagine_save();
    assert_eq!(app.status, "Harbor");
    assert!(app.imagine_save_rx.is_none());
    assert!(!app.running);
    assert!(app.chat_job_thread.is_none());

    let (tx, rx) = std::sync::mpsc::channel::<Result<String, String>>();
    app.imagine_save_rx = Some(rx);
    app.poll_imagine_save();
    assert!(app.imagine_save_rx.is_some());
    assert_eq!(app.status, "Harbor");
    tx.send(Ok("/tmp/harbor.png".into())).unwrap();
    app.poll_imagine_save();
    assert!(app.imagine_save_rx.is_none());
    assert_eq!(app.status, "Saved /tmp/harbor.png");
    assert!(!app.running);
    assert!(app.chat_job_thread.is_none());

    let (tx, rx) = std::sync::mpsc::channel::<Result<String, String>>();
    app.imagine_save_rx = Some(rx);
    tx.send(Err("disk full".into())).unwrap();
    app.poll_imagine_save();
    assert!(app.imagine_save_rx.is_none());
    assert_eq!(app.status, "disk full");
    assert!(!app.running);
    assert!(app.chat_job_thread.is_none());
}

// Folded from PR #281.
#[test]
fn chip_poll_drops_a_secret_label() {
    fn chip(id: &str, label: &str, value: &str) -> QuickChip {
        QuickChip {
            id: id.into(),
            label: label.into(),
            value: value.into(),
            kind: ChipKind::Chat,
            score: 1.0,
            hint: String::new(),
            primary: false,
        }
    }

    let mut cabin = Cabin::quiet_for_test();
    cabin.chip_busy = true;
    cabin.poll_chips();
    assert!(cabin.chip_rx.is_none());
    assert!(cabin.chip_busy);

    let (tx, rx) = std::sync::mpsc::channel();
    cabin.chip_rx = Some(rx);
    cabin.poll_chips();
    assert!(cabin.chip_rx.is_some());
    assert!(cabin.chip_busy);
    assert!(cabin.llm_chips.is_empty());

    let secret_label = format!("sk-{}", "a".repeat(12));
    let secret_value = format!("sk-{}", "b".repeat(12));
    tx.send(vec![
        chip("harbor", "Harbor", "/sh"),
        chip("secret-label", &secret_label, "ok"),
        chip("secret-value", "Night", &secret_value),
    ])
    .expect("chip send");
    cabin.poll_chips();
    assert!(!cabin.chip_busy);
    assert!(cabin.chip_rx.is_none());
    assert_eq!(cabin.llm_chips.len(), 1);
    assert_eq!(cabin.llm_chips[0].label, "Harbor");
    assert_eq!(cabin.llm_chips[0].value, "/sh");

    cabin.chip_busy = true;
    let (gone, rx) = std::sync::mpsc::channel();
    drop(gone);
    cabin.chip_rx = Some(rx);
    cabin.poll_chips();
    assert!(!cabin.chip_busy);
    assert!(cabin.chip_rx.is_none());
    assert_eq!(cabin.llm_chips.len(), 1);
    assert_eq!(cabin.llm_chips[0].label, "Harbor");
    assert_eq!(cabin.llm_chips[0].value, "/sh");
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
}

// Folded from PR #283.
#[test]
fn greeting_poll_keeps_a_fresh_line() {
    let mut cabin = Cabin::quiet_for_test();
    cabin.greeting = "Harbor is quiet tonight.".into();
    cabin.greeting_busy = true;

    cabin.poll_greeting();
    assert!(cabin.greeting_files_rx.is_none());
    assert!(cabin.greeting_rx.is_none());
    assert!(cabin.greeting_busy);
    assert_eq!(cabin.greeting, "Harbor is quiet tonight.");

    let (files_tx, files_rx) = std::sync::mpsc::channel();
    cabin.greeting_files_rx = Some(files_rx);
    cabin.poll_greeting();
    assert!(cabin.greeting_files_rx.is_some());
    assert!(cabin.greeting_rx.is_none());
    assert!(cabin.greeting_busy);
    assert_eq!(cabin.greeting, "Harbor is quiet tonight.");

    files_tx
        .send((3, "user page".into(), 4, "memory page".into()))
        .expect("send greeting files");
    cabin.poll_greeting();
    assert!(cabin.greeting_files_rx.is_none());
    assert_eq!(cabin.greeting_user_at, 3);
    assert_eq!(cabin.greeting_user_md, "user page");
    assert_eq!(cabin.greeting_memory_at, 4);
    assert_eq!(cabin.greeting_memory_md, "memory page");
    assert!(cabin.greeting_rx.is_none());
    assert!(cabin.greeting_busy);
    assert_eq!(cabin.greeting, "Harbor is quiet tonight.");

    let (greet_tx, greet_rx) = std::sync::mpsc::channel();
    cabin.greeting_rx = Some(greet_rx);
    cabin.poll_greeting();
    assert!(cabin.greeting_rx.is_some());
    assert!(cabin.greeting_busy);
    assert_eq!(cabin.greeting, "Harbor is quiet tonight.");

    greet_tx
        .send("Evening, User. The dock light is on.".into())
        .expect("send greeting");
    cabin.poll_greeting();
    assert!(cabin.greeting_rx.is_none());
    assert!(!cabin.greeting_busy);
    assert_eq!(cabin.greeting, "Evening, User. The dock light is on.");

    cabin.greeting = "Harbor is quiet tonight.".into();
    cabin.greeting_user_md = "the wall still wants a second coat tonight".into();
    let (echo_tx, echo_rx) = std::sync::mpsc::channel();
    cabin.greeting_rx = Some(echo_rx);
    echo_tx
        .send("Evening, User. the wall still wants a second coat tonight.".into())
        .expect("send echoed greeting");
    cabin.poll_greeting();
    assert!(cabin.greeting_rx.is_none());
    assert!(!cabin.greeting_busy);
    assert_eq!(cabin.greeting, "Harbor is quiet tonight.");

    cabin.greeting_busy = true;
    let (drop_tx, drop_rx) = std::sync::mpsc::channel::<String>();
    cabin.greeting_rx = Some(drop_rx);
    drop(drop_tx);
    cabin.poll_greeting();
    assert!(cabin.greeting_rx.is_none());
    assert!(cabin.greeting_files_rx.is_none());
    assert!(!cabin.greeting_busy);
    assert_eq!(cabin.greeting, "Harbor is quiet tonight.");
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
}

// Folded from PR #285.
#[test]
fn wall_cover_poll_stays_off_a_spawn() {
    let mut app = Cabin::quiet_for_test();
    app.status = "Harbor".into();
    app.wall_busy = true;
    app.poll_wall();
    assert!(app.wall_rx.is_none());
    assert!(app.wall_busy);
    assert_eq!(app.status, "Harbor");

    let (tx, rx) = std::sync::mpsc::channel::<Result<grokhub_core::WallGif, String>>();
    app.wall_rx = Some(rx);
    app.poll_wall();
    assert!(app.wall_rx.is_some());
    assert!(app.wall_busy);
    assert_eq!(app.status, "Harbor");

    drop(tx);
    app.poll_wall();
    assert!(app.wall_rx.is_none());
    assert!(!app.wall_busy);
    assert_eq!(app.status, "Harbor");
    assert!(!app.running);
    assert!(app.chat_job_thread.is_none());
}

// Folded from PR #294.
#[test]
fn history_search_poll_keeps_matching_hits() {
    let mut cabin = Cabin::quiet_for_test();
    cabin.status = "Harbor".into();

    cabin.poll_history_search();
    assert_eq!(cabin.status, "Harbor");
    assert!(cabin.history_rx.is_none());
    assert!(cabin.history_hits.is_empty());

    let (tx, rx) = std::sync::mpsc::channel();
    cabin.history_rx = Some(rx);
    cabin.poll_history_search();
    assert!(cabin.history_rx.is_some());
    assert_eq!(cabin.status, "Harbor");

    cabin.history_q = "harbor".into();
    tx.send(("other".into(), vec![("a".into(), "b".into())]))
        .expect("send other");
    cabin.poll_history_search();
    assert!(cabin.history_hits.is_empty());
    assert_eq!(cabin.status, "Harbor");
    assert!(cabin.history_rx.is_none());

    let (tx, rx) = std::sync::mpsc::channel();
    cabin.history_rx = Some(rx);
    tx.send(("harbor".into(), vec![("Chat".into(), "sess-1".into())]))
        .expect("send harbor");
    cabin.poll_history_search();
    assert_eq!(cabin.history_hits, vec![("Chat".into(), "sess-1".into())]);
    assert_eq!(cabin.status, "1 hits");
    assert!(cabin.history_rx.is_none());

    let (tx, rx) = std::sync::mpsc::channel();
    drop(tx);
    cabin.history_rx = Some(rx);
    cabin.poll_history_search();
    assert!(cabin.history_rx.is_none());
    assert_eq!(cabin.history_hits, vec![("Chat".into(), "sess-1".into())]);
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
}

// Folded from PR #295.
#[test]
fn memory_restore_poll_puts_the_body_back() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("mem-restore-poll");
    cabin.status = "Harbor".into();
    cabin.mem_name = "USER.md".into();
    cabin.mem_body = "old".into();
    cabin.poll_mem_restore();
    assert_eq!(cabin.status, "Harbor");
    assert_eq!(cabin.mem_name, "USER.md");
    assert_eq!(cabin.mem_body, "old");
    assert!(cabin.mem_restore_rx.is_none());

    let (tx, rx) = std::sync::mpsc::channel();
    cabin.mem_restore_rx = Some(rx);
    cabin.poll_mem_restore();
    assert!(cabin.mem_restore_rx.is_some());
    assert_eq!(cabin.status, "Harbor");
    assert_eq!(cabin.mem_body, "old");

    tx.send(("USER.md".into(), Ok("Night brief".into())))
        .unwrap();
    cabin.poll_mem_restore();
    assert_eq!(cabin.mem_body, "Night brief");
    assert_eq!(cabin.status, "Restored USER.md.prev");
    assert!(cabin.mem_restore_rx.is_none());
    // USER.md is cache slot 1. The body is the channel payload; the mtime slot
    // is whatever `memory_updated_at` reads, so this test leaves that field alone.
    assert_eq!(cabin.mem_cache_body[1], "Night brief");
    assert!(cabin.mem_cache_body[0].is_empty());
    assert!(cabin.mem_cache_body[2].is_empty());

    let (tx, rx) = std::sync::mpsc::channel();
    cabin.mem_restore_rx = Some(rx);
    tx.send(("USER.md".into(), Err("missing prev".into())))
        .unwrap();
    cabin.poll_mem_restore();
    assert_eq!(cabin.status, "missing prev");
    assert_eq!(cabin.mem_body, "Night brief");
    assert_eq!(cabin.mem_cache_body[1], "Night brief");

    let (tx, rx) = std::sync::mpsc::channel::<(String, Result<String, String>)>();
    drop(tx);
    cabin.mem_restore_rx = Some(rx);
    cabin.poll_mem_restore();
    assert!(cabin.mem_restore_rx.is_none());
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
    release_isolated(&root, cabin);
}

// Folded from PR #296.
#[test]
fn memory_file_poll_fills_the_open_page() {
    let mut cabin = Cabin::quiet_for_test();
    cabin.mem_name = "USER.md".into();
    cabin.mem_body = "old".into();
    cabin.status = "Harbor".into();

    cabin.poll_mem_file();
    assert_eq!(cabin.mem_name, "USER.md");
    assert_eq!(cabin.mem_body, "old");
    assert_eq!(cabin.status, "Harbor");
    assert!(cabin.mem_file_rx.is_none());
    assert_eq!(cabin.mem_cache_at, [0, 0, 0]);
    assert_eq!(
        cabin.mem_cache_body,
        [String::new(), String::new(), String::new()]
    );

    let (tx, rx) = std::sync::mpsc::channel();
    cabin.mem_file_rx = Some(("USER.md".into(), rx));
    cabin.poll_mem_file();
    assert!(cabin
        .mem_file_rx
        .as_ref()
        .is_some_and(|(name, _)| name == "USER.md"));
    assert_eq!(cabin.mem_body, "old");
    assert_eq!(cabin.status, "Harbor");

    tx.send((9, "Night brief".into())).expect("user file");
    cabin.poll_mem_file();
    assert_eq!(cabin.mem_body, "Night brief");
    assert_eq!(cabin.mem_cache_at[1], 9);
    assert_eq!(cabin.mem_cache_body[1], "Night brief");
    assert_eq!(cabin.status, "Harbor");
    assert!(cabin.mem_file_rx.is_none());

    let (soul_tx, soul_rx) = std::sync::mpsc::channel();
    cabin.mem_file_rx = Some(("SOUL.md".into(), soul_rx));
    soul_tx.send((3, "soul text".into())).expect("soul file");
    assert_eq!(cabin.mem_name, "USER.md");
    cabin.poll_mem_file();
    assert_eq!(cabin.mem_body, "Night brief");
    assert_eq!(cabin.mem_cache_at[0], 3);
    assert_eq!(cabin.mem_cache_body[0], "soul text");
    assert_eq!(cabin.mem_cache_at[1], 9);
    assert_eq!(cabin.mem_cache_body[1], "Night brief");
    assert_eq!(cabin.status, "Harbor");
    assert!(cabin.mem_file_rx.is_none());

    let (gone_tx, gone_rx) = std::sync::mpsc::channel();
    cabin.mem_file_rx = Some(("MEMORY.md".into(), gone_rx));
    drop(gone_tx);
    cabin.poll_mem_file();
    assert!(cabin.mem_file_rx.is_none());
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
    assert_eq!(cabin.mem_body, "Night brief");
    assert_eq!(cabin.status, "Harbor");
}

// Folded from PR #297.
#[test]
fn reflect_poll_reports_a_drop() {
    let mut cabin = Cabin::quiet_for_test();
    cabin.status = "Harbor".into();
    cabin.poll_reflect();
    assert_eq!(cabin.status, "Harbor");
    assert!(cabin.reflect_rx.is_none());

    let (tx, rx) = std::sync::mpsc::channel::<super::learn_notes_ui::LearnWrites>();
    cabin.reflect_rx = Some(rx);
    cabin.poll_reflect();
    assert!(cabin.reflect_rx.is_some());
    assert_eq!(cabin.status, "Harbor");

    drop(tx);
    cabin.poll_reflect();
    assert!(cabin.reflect_rx.is_none());
    assert_eq!(cabin.status, "Reflect failed");
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
}

// Folded from PR #298.
#[test]
fn hub_sync_poll_reports_a_drop() {
    // Pin a fresh config root: poll_sync on disconnect consults read_key, and a
    // leftover learned-key.id / sealed file under the shared home config would
    // turn a plain drop into "Hub sync not sent: … key missing".
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("hub-sync-drop");
    let _ = std::fs::create_dir_all(&root);
    let _pin = crate::config::TestConfigDir::set(root);
    let mut cabin = Cabin::quiet_for_test();
    cabin.status = "Harbor".into();
    let nav = cabin.nav;

    cabin.poll_sync();
    assert_eq!(cabin.status, "Harbor");
    assert!(cabin.sync_rx.is_none());

    let (tx, rx) = std::sync::mpsc::channel::<(String, Vec<grokhub_core::HubMemoryFile>)>();
    cabin.sync_rx = Some(rx);
    cabin.poll_sync();
    assert!(cabin.sync_rx.is_some());
    assert_eq!(cabin.status, "Harbor");

    drop(tx);
    cabin.poll_sync();
    assert!(cabin.sync_rx.is_none());
    assert_eq!(cabin.status, "Hub sync failed");
    assert_eq!(cabin.nav, nav);
    assert!(!matches!(cabin.nav, Nav::Devices));
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
}

// Folded from PR #299.
#[test]
fn verify_poll_stays_off_a_skill_save() {
    let mut cabin = Cabin::quiet_for_test();
    cabin.status = "Harbor".into();
    assert!(cabin.verify_chip.is_empty());
    assert!(!cabin.verify_ok_turn);
    assert!(cabin.verify_rx.is_none());
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());

    cabin.poll_verify();
    assert_eq!(cabin.status, "Harbor");
    assert!(cabin.verify_chip.is_empty());
    assert!(!cabin.verify_ok_turn);
    assert!(cabin.verify_rx.is_none());

    let (tx, rx) = std::sync::mpsc::channel();
    cabin.verify_rx = Some(rx);
    cabin.poll_verify();
    assert!(
        cabin.verify_rx.is_some(),
        "an empty poll keeps the receiver"
    );
    assert_eq!(cabin.status, "Harbor");
    assert!(cabin.verify_chip.is_empty());
    assert!(!cabin.verify_ok_turn);

    tx.send(None).expect("send None");
    cabin.poll_verify();
    assert!(cabin.verify_rx.is_none());
    assert_eq!(cabin.status, "Harbor");
    assert!(cabin.verify_chip.is_empty());
    assert!(!cabin.verify_ok_turn);
    assert!(
        cabin.messages.is_empty(),
        "None must not apply a verify result"
    );

    let (gone, rx) = std::sync::mpsc::channel();
    drop(gone);
    cabin.verify_rx = Some(rx);
    cabin.poll_verify();
    assert!(cabin.verify_rx.is_none());
    assert_eq!(cabin.status, "Harbor");
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
    assert!(cabin.verify_chip.is_empty());
    assert!(!cabin.verify_ok_turn);
}

// Folded from PR #300.
#[test]
fn eyes_cap_poll_stays_off_a_store() {
    let mut cabin = Cabin::quiet_for_test();
    cabin.status = "Harbor".into();

    assert!(cabin.poll_eyes_cap().is_none());
    assert!(cabin.eyes_cap_rx.is_none());
    assert_eq!(cabin.status, "Harbor");

    let (tx, rx) = std::sync::mpsc::channel::<Result<String, String>>();
    cabin.eyes_cap_rx = Some(rx);
    assert!(cabin.poll_eyes_cap().is_none());
    assert!(cabin.eyes_cap_rx.is_some());
    assert_eq!(cabin.status, "Harbor");

    drop(tx);
    assert!(cabin.poll_eyes_cap().is_none());
    assert!(cabin.eyes_cap_rx.is_none());
    assert_eq!(cabin.status, "Harbor");
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
}

// Folded from PR #301.
#[test]
fn recipe_cap_poll_stays_off_a_store() {
    let mut app = Cabin::quiet_for_test();
    app.status = "Harbor".into();
    app.eyes_text = "frame: capturing…\n".into();
    app.poll_recipe_cap();
    assert_eq!(app.status, "Harbor");
    assert_eq!(app.eyes_text, "frame: capturing…\n");
    assert!(app.recipe_cap_rx.is_none());

    let (tx, rx) = std::sync::mpsc::channel::<Result<String, String>>();
    app.recipe_cap_rx = Some(rx);
    app.poll_recipe_cap();
    assert!(app.recipe_cap_rx.is_some());
    assert_eq!(app.status, "Harbor");
    assert_eq!(app.eyes_text, "frame: capturing…\n");

    drop(tx);
    app.poll_recipe_cap();
    assert!(app.recipe_cap_rx.is_none());
    assert_eq!(app.status, "Harbor");
    assert_eq!(app.eyes_text, "frame: capturing…\n");
    assert!(!app.running);
    assert!(app.chat_job_thread.is_none());
}

// Folded from PR #302.
#[test]
fn inhabit_poll_reports_a_drop() {
    let mut cabin = Cabin::quiet_for_test();
    cabin.status = "Harbor".into();
    cabin.poll_inhabit();
    assert_eq!(cabin.status, "Harbor");
    assert!(cabin.inhabit_rx.is_none());

    let (tx, rx) = std::sync::mpsc::channel::<grokhub_core::InhabitBundle>();
    cabin.inhabit_rx = Some(rx);
    cabin.poll_inhabit();
    assert!(cabin.inhabit_rx.is_some());
    assert_eq!(cabin.status, "Harbor");

    drop(tx);
    cabin.poll_inhabit();
    assert!(cabin.inhabit_rx.is_none());
    assert_eq!(cabin.status, "Inhabit failed");
    assert!(!matches!(cabin.nav, Nav::Devices));
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
}

// Folded from PR #303.
#[test]
fn openclaw_import_poll_reports_a_drop() {
    let mut cabin = Cabin::quiet_for_test();
    cabin.status = "Harbor".into();
    cabin.poll_import_openclaw();
    assert_eq!(cabin.status, "Harbor");
    assert!(cabin.import_rx.is_none());

    let (tx, rx) = std::sync::mpsc::channel();
    cabin.import_rx = Some(rx);
    cabin.poll_import_openclaw();
    assert!(cabin.import_rx.is_some());
    assert_eq!(cabin.status, "Harbor");

    drop(tx);
    cabin.poll_import_openclaw();
    assert!(cabin.import_rx.is_none());
    assert_eq!(cabin.status, "OpenClaw import failed");
    assert!(!matches!(cabin.nav, Nav::Memory));
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
}

// Folded from PR #304.
#[test]
fn recipe_replay_poll_reports_a_drop() {
    let mut cabin = Cabin::quiet_for_test();
    cabin.status = "Harbor".into();
    cabin.poll_replay_desk();
    assert_eq!(cabin.status, "Harbor");
    assert!(cabin.recipe_desk_rx.is_none());

    let (tx, rx) = std::sync::mpsc::channel::<super::ReplayDeskOut>();
    cabin.recipe_desk_rx = Some(rx);
    cabin.poll_replay_desk();
    assert!(cabin.recipe_desk_rx.is_some());
    assert_eq!(cabin.status, "Harbor");

    drop(tx);
    cabin.poll_replay_desk();
    assert!(cabin.recipe_desk_rx.is_none());
    assert_eq!(cabin.status, "Recipe replay failed");
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
}

// Folded from PR #308.
#[test]
fn night_check_poll_reports_a_drop() {
    let mut cabin = Cabin::quiet_for_test();
    cabin.status = "Harbor".into();

    assert!(!cabin.poll_night_check(0));
    assert_eq!(cabin.status, "Harbor");
    assert!(cabin.night_check_rx.is_none());

    let (tx, rx) = std::sync::mpsc::channel::<(String, i32)>();
    cabin.night_check_rx = Some(("night-check".into(), rx));
    assert!(cabin.poll_night_check(0));
    assert!(cabin.night_check_rx.is_some());
    assert_eq!(cabin.status, "Harbor");

    drop(tx);
    assert!(!cabin.poll_night_check(0));
    assert!(cabin.night_check_rx.is_none());
    assert_eq!(cabin.status, "Harbor");
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
}

// Folded from PR #309.
#[test]
fn persist_poll_clears_on_drop() {
    let mut cabin = Cabin::quiet_for_test();
    cabin.status = "Harbor".into();
    cabin.poll_persist();
    assert_eq!(cabin.status, "Harbor");
    assert!(cabin.persist_rx.is_none());

    let (tx, rx) = std::sync::mpsc::channel::<()>();
    cabin.persist_rx = Some(rx);
    cabin.poll_persist();
    assert!(cabin.persist_rx.is_some());
    assert_eq!(cabin.status, "Harbor");

    drop(tx);
    cabin.poll_persist();
    assert!(cabin.persist_rx.is_none());
    assert_eq!(cabin.status, "Harbor");
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
}

// Folded from PR #310.
#[test]
fn goals_poll_clears_busy_on_drop() {
    let mut cabin = Cabin::quiet_for_test();
    cabin.status = "Harbor".into();
    cabin.goal_stale = false;
    cabin.poll_goals();
    assert_eq!(cabin.status, "Harbor");
    assert!(cabin.goal_rx.is_none());
    assert!(!cabin.goal_busy);

    cabin.goal_busy = true;
    let (tx, rx) = std::sync::mpsc::channel::<(String, String)>();
    cabin.goal_rx = Some(rx);
    cabin.poll_goals();
    assert!(cabin.goal_rx.is_some());
    assert!(cabin.goal_busy);
    assert_eq!(cabin.status, "Harbor");

    drop(tx);
    cabin.poll_goals();
    assert!(cabin.goal_rx.is_none());
    assert!(!cabin.goal_busy);
    assert!(!cabin.goal_stale);
    assert_eq!(cabin.status, "Harbor");
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
}

// Folded from PR #311.
#[test]
fn review_poll_clears_busy_on_drop() {
    let mut cabin = Cabin::quiet_for_test();
    cabin.status = "Harbor".into();
    cabin.poll_review();
    assert_eq!(cabin.status, "Harbor");
    assert!(cabin.review_rx.is_none());
    assert!(!cabin.review_busy);

    cabin.review_busy = true;
    let (tx, rx) = std::sync::mpsc::channel::<Result<String, String>>();
    cabin.review_rx = Some(rx);
    cabin.poll_review();
    assert!(cabin.review_rx.is_some());
    assert!(cabin.review_busy);
    assert_eq!(cabin.status, "Harbor");

    drop(tx);
    cabin.poll_review();
    assert!(cabin.review_rx.is_none());
    assert!(!cabin.review_busy);
    assert_eq!(cabin.status, "Harbor");
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
}

// Folded from PR #313.
#[test]
fn cabin_frame_poll_reports_skip_on_drop() {
    let mut cabin = Cabin::quiet_for_test();
    cabin.status = "Harbor".into();

    let (tx, rx) = std::sync::mpsc::channel::<Result<String, String>>();
    cabin.kick_cap_rx = Some(rx);
    assert!(matches!(cabin.poll_cabin_frame(), CabinFrame::Pending));
    assert!(cabin.kick_cap_rx.is_some());
    assert_eq!(cabin.status, "Harbor");
    assert!(!cabin.kick_skip);

    drop(tx);
    assert!(matches!(cabin.poll_cabin_frame(), CabinFrame::Skip));
    assert!(cabin.kick_cap_rx.is_none());
    assert!(cabin.kick_skip);
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
}

// Folded from PR #314.
#[test]
fn host_diff_poll_finishes_on_drop() {
    let mut cabin = Cabin::quiet_for_test();
    cabin.status = "Harbor".into();
    cabin.poll_host_diff();
    assert_eq!(cabin.status, "Harbor");
    assert!(!cabin.host_diff_kick);

    let (tx, rx) = std::sync::mpsc::channel::<Option<String>>();
    cabin.host_diff_rx = Some(rx);
    cabin.poll_host_diff();
    assert!(cabin.host_diff_rx.is_some());
    assert_eq!(cabin.status, "Harbor");
    assert!(!cabin.host_diff_kick);

    drop(tx);
    cabin.poll_host_diff();
    assert!(cabin.host_diff_rx.is_none());
    assert!(!cabin.host_diff_kick);
    assert_eq!(cabin.status, "Harbor");
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
}

// Folded from PR #315.
#[test]
fn profile_pick_poll_clears_on_drop() {
    let mut cabin = Cabin::quiet_for_test();
    cabin.status = "Harbor".into();
    cabin.poll_profile_pick();
    assert_eq!(cabin.status, "Harbor");
    assert!(cabin.profile_pick_rx.is_none());

    let (tx, rx) = std::sync::mpsc::channel::<(u64, super::ProfilePick)>();
    cabin.profile_pick_rx = Some(rx);
    cabin.poll_profile_pick();
    assert!(cabin.profile_pick_rx.is_some());
    assert_eq!(cabin.status, "Harbor");

    drop(tx);
    cabin.poll_profile_pick();
    assert!(cabin.profile_pick_rx.is_none());
    assert_eq!(cabin.status, "Harbor");
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
}

// Folded from PR #316.
#[test]
fn oauth_start_poll_reports_a_drop() {
    let mut cabin = Cabin::quiet_for_test();
    cabin.status = "Harbor".into();
    cabin.poll_oauth();
    assert_eq!(cabin.status, "Harbor");
    assert!(cabin.oauth_start_rx.is_none());

    let (tx, rx) = std::sync::mpsc::channel::<Result<DeviceCodeStart, String>>();
    cabin.oauth_start_rx = Some(rx);
    cabin.poll_oauth();
    assert!(cabin.oauth_start_rx.is_some());
    assert_eq!(cabin.status, "Harbor");

    drop(tx);
    cabin.poll_oauth();
    assert!(cabin.oauth_start_rx.is_none());
    assert_eq!(cabin.status, "Grok OAuth failed to start");
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
}

// Folded from PR #317.
#[test]
fn ideas_poll_clears_on_drop() {
    let mut cabin = Cabin::quiet_for_test();
    cabin.status = "Harbor".into();
    cabin.poll_ideas();
    assert_eq!(cabin.status, "Harbor");
    assert!(cabin.ideas_rx.is_none());

    let (tx, rx) = std::sync::mpsc::channel::<String>();
    cabin.ideas_rx = Some((rx, super::feed_ui::IdeaInputs::default()));
    cabin.cfg.feed_pulse.last_ideas_ms = grokhub_core::now_ms();
    cabin.poll_ideas();
    assert!(cabin.ideas_rx.is_some());
    assert_eq!(cabin.status, "Harbor");

    drop(tx);
    cabin.poll_ideas();
    assert!(cabin.ideas_rx.is_none());
    assert_eq!(cabin.status, "Harbor");
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
    assert!(!matches!(cabin.nav, Nav::Pulse));
}

// Folded from PR #318.
#[test]
fn grok_loop_poll_reports_a_drop() {
    let mut cabin = Cabin::quiet_for_test();
    cabin.status = "Harbor".into();
    assert!(!cabin.poll_grok_loop());
    assert_eq!(cabin.status, "Harbor");

    let (tx, rx) = std::sync::mpsc::channel::<String>();
    cabin.grok_loop_rx = Some(("loop-1".into(), rx));
    assert!(cabin.poll_grok_loop());
    assert!(cabin.grok_loop_rx.is_some());
    assert_eq!(cabin.status, "Harbor");

    drop(tx);
    assert!(!cabin.poll_grok_loop());
    assert!(cabin.grok_loop_rx.is_none());
    assert_eq!(cabin.status, "Harbor");
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
}

// Folded from PR #320.
#[test]
fn oauth_poll_rx_reports_a_drop() {
    let mut cabin = Cabin::quiet_for_test();
    cabin.status = "Harbor".into();
    assert!(cabin.oauth_start_rx.is_none());
    assert!(cabin.oauth_pending.is_none());
    cabin.poll_oauth();
    assert_eq!(cabin.status, "Harbor");
    assert!(cabin.oauth_poll_rx.is_none());

    let (tx, rx) = std::sync::mpsc::channel::<Result<grokhub_core::PollResult, String>>();
    cabin.oauth_poll_rx = Some(rx);
    cabin.poll_oauth();
    assert!(cabin.oauth_poll_rx.is_some());
    assert_eq!(cabin.status, "Harbor");
    assert!(cabin.oauth_start_rx.is_none());
    assert!(cabin.oauth_pending.is_none());

    drop(tx);
    cabin.poll_oauth();
    assert!(cabin.oauth_poll_rx.is_none());
    assert_eq!(cabin.status, "Harbor");
    assert!(cabin.oauth_start_rx.is_none());
    assert!(cabin.oauth_pending.is_none());
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
}

// Folded from PR #321.
#[test]
fn job_poll_reports_worker_gone_on_drop() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("job-worker-gone");
    cabin.status = "Harbor".into();
    cabin.poll_job();
    assert_eq!(cabin.status, "Harbor");
    assert!(cabin.rx.is_none());

    cabin.running = true;
    let (tx, rx) = std::sync::mpsc::channel::<JobOut>();
    cabin.rx = Some(rx);
    cabin.poll_job();
    assert!(cabin.rx.is_some());
    assert!(cabin.running);
    assert_eq!(cabin.status, "Harbor");

    drop(tx);
    cabin.poll_job();
    assert!(cabin.rx.is_none());
    assert!(!cabin.running);
    assert_eq!(cabin.status, worker_gone_status());
    assert!(cabin.chat_job_thread.is_none());
    release_isolated(&root, cabin);
}

// Folded from PR #324.
#[test]
fn greeting_files_poll_clears_on_drop() {
    let mut cabin = Cabin::quiet_for_test();
    cabin.status = "Harbor".into();
    assert!(cabin.greeting_files_rx.is_none());
    assert!(cabin.greeting_rx.is_none());
    cabin.poll_greeting();
    assert_eq!(cabin.status, "Harbor");
    assert!(cabin.greeting_files_rx.is_none());
    assert!(cabin.greeting_rx.is_none());

    let (tx, rx) = std::sync::mpsc::channel::<(u64, String, u64, String)>();
    cabin.greeting_files_rx = Some(rx);
    cabin.poll_greeting();
    assert!(cabin.greeting_files_rx.is_some());
    assert!(cabin.greeting_rx.is_none());
    assert_eq!(cabin.status, "Harbor");

    drop(tx);
    cabin.poll_greeting();
    assert!(cabin.greeting_files_rx.is_none());
    assert!(cabin.greeting_rx.is_none());
    assert_eq!(cabin.status, "Harbor");
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
}

// Folded from PR #325.
#[test]
fn chips_poll_clears_busy_on_drop() {
    let mut cabin = Cabin::quiet_for_test();
    cabin.status = "Harbor".into();
    cabin.poll_chips();
    assert_eq!(cabin.status, "Harbor");
    assert!(!cabin.chip_busy);

    cabin.chip_busy = true;
    let (tx, rx) = std::sync::mpsc::channel::<Vec<QuickChip>>();
    cabin.chip_rx = Some(rx);
    cabin.poll_chips();
    assert!(cabin.chip_rx.is_some());
    assert!(cabin.chip_busy);
    assert_eq!(cabin.status, "Harbor");

    drop(tx);
    cabin.poll_chips();
    assert!(cabin.chip_rx.is_none());
    assert!(!cabin.chip_busy);
    assert_eq!(cabin.status, "Harbor");
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
}

// LO-07: background writes report what actually happened.
#[test]
fn poll_reports_success_and_failure() {
    let mut cabin = Cabin::quiet_for_test();
    cabin.status = "Harbor".into();
    cabin.spawn_file_write("/x/export.md".into(), false, || Ok("/x/export-1.md".into()));
    wait_for("write result", || {
        cabin.poll_file_writes();
        cabin.file_writes.is_empty()
    });
    assert_eq!(cabin.status, "Wrote /x/export-1.md");

    cabin.spawn_file_write("SOUL.md".into(), true, || Err("disk full".into()));
    wait_for("write error", || {
        cabin.poll_file_writes();
        cabin.file_writes.is_empty()
    });
    assert_eq!(cabin.status, "Couldn't write SOUL.md: disk full");

    cabin.status = "Harbor".into();
    cabin.spawn_file_write("MEMORY.md".into(), true, || Ok("MEMORY.md".into()));
    wait_for("quiet autosave", || {
        cabin.poll_file_writes();
        cabin.file_writes.is_empty()
    });
    assert_eq!(
        cabin.status, "Harbor",
        "a quiet autosave says nothing on success"
    );
}
