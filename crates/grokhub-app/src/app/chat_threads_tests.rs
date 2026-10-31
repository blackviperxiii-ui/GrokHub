//! Chat threads, history rail and chat find tests (rearch stage 5).
//!
//! Mechanical move from `tests.rs`. Wired via `#[path]` so
//! `app/mod.rs` stays untouched.

use super::*;
use eframe::egui;

#[test]
fn rename_focus_selects_the_placeholder() {
    egui::__run_test_ui(|ui| {
        let mut buf = String::from("Project");
        let edit = ui.add(egui::TextEdit::singleline(&mut buf));
        select_all_edit(ui, edit.id, &buf);
        let state = egui::TextEdit::load_state(ui.ctx(), edit.id).expect("edit state");
        let range = state.cursor.char_range().expect("selection");
        let [a, b] = range.sorted_cursors();
        assert_eq!(a.index, egui::text::CharIndex(0));
        assert_eq!(b.index, egui::text::CharIndex(7));
    });
}

#[test]
fn selecting_plan_does_not_change_the_thread_title() {
    assert_eq!(
        grokhub_core::cli_history::title_after_selecting_plan("Night watch"),
        "Night watch"
    );
    assert_eq!(
        grokhub_core::cli_history::preferred_history_title(
            "Night watch",
            false,
            Some("Plan"),
            Some("abc")
        ),
        "Night watch"
    );
    assert_eq!(
        grokhub_core::cli_history::history_label_after_plan("Night watch", "fix the dock"),
        "Night watch"
    );
}

#[test]
fn history_rail_uses_session_names_and_can_delete() {
    let src = cabin_src();
    let tabs = include_str!("chat_tabs.rs");
    assert!(
        tabs.contains("chat_section_indices") && tabs.contains("TabAct::Switch"),
        "chat tabs list cabin chats, not grok sessions: {tabs}"
    );
    assert!(
        !tabs.contains("discover_session_files") && !tabs.contains("reload_grok_sessions"),
        "chat tabs must not walk session dirs or relist the CLI: {tabs}"
    );
    assert!(
        tabs.contains("TabAct::Delete") && tabs.contains("\"Delete\""),
        "chat tabs must offer Delete: {tabs}"
    );
    assert!(
        tabs.contains("session_list_order")
            && tabs.contains("\"Unpin\"")
            && tabs.contains("\"Pin\"")
            && tabs.contains("\"Rename\""),
        "chat tabs must pin and rename: {tabs}"
    );
    assert!(
        tabs.contains("is_background_history_title"),
        "chat tabs must drop workboard summarize and similar: {tabs}"
    );
    let side = include_str!("sidebar.rs");
    assert!(
        !side.contains("ui_rail_chat_drawer")
            && !side.contains("rail_chat_drawer_open")
            && !side.contains("rail-archived"),
        "sidebar must not keep the chat drawer or Archived section: {side}"
    );
    let page = src
        .split("crate::cards::section_label(ui, \"Chats\")")
        .nth(1)
        .and_then(|s| s.split("fn ui_board(").next())
        .expect("history chats section");
    assert!(
        page.contains("thread_rail_title") && page.contains("chat_section_indices"),
        "History page must paint cabin chats: {page}"
    );
    assert!(
        page.contains("Delete") && page.contains("delete_thread_at"),
        "History page must delete the cabin chat: {page}"
    );
    assert!(
        page.contains("is_background_history_title"),
        "History page must not list workboard summarize: {page}"
    );
    assert!(
        !page.contains("Archived chats")
            && !page.contains("No archived chats")
            && !page.contains("\"Unarchive\""),
        "History page must not paint an Archived drawer: {page}"
    );
    let dta = fn_src(&src, "delete_thread_at");
    assert!(
        dta.contains("soft_delete_thread")
            && dta.contains("hard_purge_thread_sessions")
            && !dta.contains("grok_user_stdout"),
        "delete soft-trashes when possible; ResetLast hard-purges sessions; never runs the CLI: {dta}"
    );
    let purge = fn_src(&src, "hard_purge_thread_sessions");
    assert!(
        purge.contains("delete_session"),
        "expired trash / ResetLast must drop native sessions: {purge}"
    );
    assert!(
        page.contains("self.nav = Nav::History"),
        "deleting a History chat must keep the See all pane: {page}"
    );
    let hist = src
        .split("fn ui_history(")
        .nth(1)
        .and_then(|s| s.split("fn ui_board(").next())
        .expect("ui_history");
    assert!(
        hist.contains("self.history_header(ui)"),
        "History See all must draw the header with Delete all: {hist}"
    );
    let header = src
        .split("fn history_header(")
        .nth(1)
        .and_then(|s| s.split("fn ui_history(").next())
        .expect("history_header");
    assert!(
        header.contains("Delete all")
            && header.contains("click_delete_all_history")
            && header.contains("danger_pill"),
        "Delete all is a red two-click button, never a one-click primary: {header}"
    );
    let poll = src
        .split("fn poll_single(")
        .nth(1)
        .and_then(|s| s.split("fn upsert_stream_assistant(").next())
        .expect("poll_single");
    assert!(
        poll.contains("apply_auto_title"),
        "a finished turn must name the tab from the session, not leave Chat: {poll}"
    );
    assert!(
        poll.contains("GrokPEvent::Usage")
            && poll.contains("GrokPEvent::Compact")
            && poll.contains("thinking_status")
            && poll.contains("turn_footer"),
        "1.0.13 stream must paint usage, compact, and a turn footer: {poll}"
    );
    assert!(
        poll.contains("GrokPEvent::Recovering") && poll.contains("apply_compact_status"),
        "1.0.13 truncation/5xx recovery and compact errors must not kill the turn: {poll}"
    );
    assert!(
        poll.contains("retry_status_line"),
        "1.0.14 retry status must show a short reason: {poll}"
    );
    assert!(
        poll.contains("scheduled_perm = false"),
        "a finished or failed grok -p turn must drop scheduled_perm: {poll}"
    );
    let deleted = fn_src(&src, "delete_thread_at");
    assert!(
        deleted.contains("soft_delete_thread") && deleted.contains("hard_purge_thread_sessions"),
        "delete soft-trashes live chats and hard-purges the ResetLast / expired path: {deleted}"
    );
    let purge = fn_src(&src, "hard_purge_thread_sessions");
    assert!(
        purge.contains("grokhub_agent::delete_session"),
        "hard purge must drop native sessions: {purge}"
    );
    let title = fn_src(&src, "thread_rail_title");
    assert!(
        title.contains("preferred_history_title"),
        "rail titles must prefer the Grok Build session name: {title}"
    );
}

#[test]
#[allow(clippy::assertions_on_constants)] // pins design constants
fn rail_footer_is_reserved() {
    assert_eq!(super::sidebar::RAIL_FOOTER_H, 40.0);
    assert!(super::PALETTE_LIST_H < 400.0);
}

#[test]
fn rail_chat_title_stays_short() {
    assert_eq!(
        grokhub_core::display_tab_title("chowder and food interest and cho"),
        "chowder food interest cho"
    );
    with_fonts_ui(|ui| {
        let painted = super::fit_rail_label(ui, "chowder and food interest and cho", 72.0);
        assert!(
            painted.chars().count() <= grokhub_core::AUTO_TITLE_MAX,
            "rail label must not run past the tab title cap: {painted}"
        );
        assert!(
            painted.contains('…') || painted.starts_with("chowder"),
            "{painted}"
        );
    });
}

#[test]
fn chat_rail_restores_last_used_tab() {
    let src = cabin_src();
    let theme = include_str!("../theme.rs");
    let loadout = theme
        .split("pub const LOADOUT_NAV")
        .nth(1)
        .and_then(|s| s.split("pub const CABIN_MENU").next())
        .expect("LOADOUT_NAV");
    let home = loadout.find("(\"home\", \"Home\")").expect("home rail");
    let chat = loadout.find("(\"chat\", \"Chat\")").expect("chat rail");
    assert!(home < chat, "Home sits above Chat on the Loadout rail");
    let set_nav = fn_src(&src, "set_nav_id");
    let chat_arm = set_nav
        .split("\"chat\" =>")
        .nth(1)
        .and_then(|s| s.split("_ =>").next())
        .expect("chat arm");
    assert!(
        chat_arm.contains("self.open_last_chat_tab()"),
        "Chat rail restores the last-used tab: {chat_arm}"
    );
    assert!(
        !chat_arm.contains("new_thread(false)"),
        "Chat rail must not force a fresh draft over last-used: {chat_arm}"
    );
    let open_last = fn_src(&src, "open_last_chat_tab");
    assert!(
        open_last.contains("last_chat_tab")
            && open_last.contains("last_used_live_chat_index")
            && open_last.contains("new_thread(false)"),
        "restore prefers remembered, then next live, else fresh: {open_last}"
    );
    let fresh = fn_src(&src, "open_fresh_home");
    assert!(
        fresh.contains("last_chat_tab") && fresh.contains("keep"),
        "Home parking must not wipe last_chat_tab: {fresh}"
    );
    let created = src
        .split("fn new_thread")
        .nth(1)
        .and_then(|s| s.split("fn begin_chat_rename").next())
        .expect("new_thread");
    assert!(
        created.contains("reuse_empty_thread_idx") && created.contains("has_session"),
        "tab + still reuses an empty no-session draft instead of stacking Chats: {created}"
    );
    assert!(
        created.contains("composer_want_focus = true"),
        "new chat must put the cursor in the composer: {created}"
    );
    let side = fn_src(&src, "ui_sidebar");
    assert!(
        !side.contains("\"New chat\"") && !side.contains("RailIcon::Compose"),
        "sidebar must not keep a separate New chat button: {side}"
    );
    assert!(
        !side.contains("ui_rail_chat_drawer") && !side.contains("rail_chat_drawer_open"),
        "Chat rail must not open a drawer: {side}"
    );
    let tabs = include_str!("chat_tabs.rs");
    assert!(
        tabs.contains("ui_chat_tabs")
            && tabs.contains("apply_tab_act")
            && tabs.contains("CHAT_TABS_H"),
        "chat tabs live under the title bar: {tabs}"
    );
    let tab_act = fn_src(&src, "apply_tab_act");
    assert!(
        tab_act.contains("composer_want_focus = true"),
        "tab clicks must focus the composer: {tab_act}"
    );
    let palette = src
        .split("fn run_palette(")
        .nth(1)
        .and_then(|s| s.split("fn run_slash_line(").next())
        .expect("run_palette");
    assert!(
        palette.contains("\"nav:chat\"") && palette.contains("self.open_last_chat_tab()"),
        "palette Chat uses the same last-used restore as the rail: {palette}"
    );
    assert!(
        palette.contains("file:")
            && palette.contains("palette_file_shown")
            && palette.contains("desktop::open_path"),
        "picking a palette file must open it, not only write status: {palette}"
    );
    let land = src
        .split("fn land_on_real_chat(")
        .nth(1)
        .and_then(|s| s.split("fn new_thread(").next())
        .expect("land_on_real_chat");
    assert!(
        land.contains("scratch()") && land.contains("apply_switch_thread"),
        "background chat must leave Scratch for the last real thread: {land}"
    );
    assert!(
        land.contains("most_recently_accessed_index") && !land.contains("self.persist()"),
        "leaving Scratch for a night/inbox job must not clone every thread twice: {land}"
    );
    let house = src
        .split("HeartbeatAct::Housekeep =>")
        .nth(1)
        .and_then(|s| s.split("HeartbeatAct::Inbox =>").next())
        .expect("housekeep");
    assert!(
        !house.contains("stamp_current_access"),
        "sitting on a chat is not activity and must not move that History row: {house}"
    );
    let idle = src
        .split("JobKind::Reflect =>")
        .nth(1)
        .and_then(|s| s.split("JobKind::Indexers =>").next())
        .expect("idle reflect");
    assert!(
        idle.contains("scratch()"),
        "idle reflect must not consume the slot on Scratch: {idle}"
    );
    let mut older = crate::threads::ChatThread::new("Older", false);
    older.accessed_ms = 1_000;
    let mut newer = crate::threads::ChatThread::new("Night cabin", false);
    newer.accessed_ms = 8_000;
    let mut scratch = crate::threads::ChatThread::new("Scratch", true);
    scratch.accessed_ms = 9_000;
    assert_eq!(
        crate::threads::most_recently_accessed_index(&[older, newer, scratch]),
        Some(1)
    );
}

/// Sidebar History order: pins last-pinned-first, then last used.
fn history_row_titles(cabin: &Cabin) -> Vec<String> {
    let listed = crate::threads::chat_section_indices(
        &cabin.threads,
        Some(cabin.thread_idx),
        cabin.messages.is_empty(),
    );
    let keys: Vec<crate::threads::SessionSortKey> = listed
        .iter()
        .map(|&i| crate::threads::sort_key(&cabin.threads[i]))
        .collect();
    crate::threads::session_list_order(&keys)
        .into_iter()
        .map(|pos| cabin.threads[listed[pos]].title.clone())
        .collect()
}

fn spoke(title: &str, accessed_ms: u64) -> crate::threads::ChatThread {
    let mut thread = crate::threads::ChatThread::new(title, false);
    thread.accessed_ms = accessed_ms;
    thread
        .messages_mut()
        .push(("user".into(), format!("keep {title}")));
    thread
}

#[test]
fn opening_a_history_row_keeps_its_place() {
    let _lock = crate::config::hold_test_config();
    let root = crate::config::test_config_root("history-click");
    let _ = std::fs::remove_dir_all(&root);
    let _cfg = RestoreEnv::set("GROKHUB_CONFIG", &root);
    let _tray = RestoreEnv::set("GROKHUB_TRAY", "0");

    let mut last_pin = spoke("Last pin", 1);
    last_pin.pinned = true;
    last_pin.pinned_ms = 80;
    let mut earlier_pin = spoke("Earlier pin", 9_000);
    earlier_pin.pinned = true;
    earlier_pin.pinned_ms = 20;
    let newer = spoke("Newer chat", 500);
    let older = spoke("Older chat", 100);
    let earlier_idx = 1;
    let newer_idx = 2;
    let older_idx = 3;
    let mut cabin = Cabin::quiet_cabin(vec![last_pin, earlier_pin, newer, older], newer_idx);
    assert!(!cabin.running, "the cabin is quiet before the click");

    let before = history_row_titles(&cabin);
    assert_eq!(
        before,
        ["Last pin", "Earlier pin", "Newer chat", "Older chat"],
        "pins stay last-pinned-first, then last used: {before:?}"
    );
    let opened_at = before.iter().position(|t| t == "Older chat").unwrap();
    assert_ne!(opened_at, 0, "the chat we open is not already first");
    let older_accessed = cabin.threads[older_idx].accessed_ms;
    let earlier_pin_ms = cabin.threads[earlier_idx].pinned_ms;
    let earlier_accessed = cabin.threads[earlier_idx].accessed_ms;

    cabin.switch_thread(older_idx);
    let after_click = history_row_titles(&cabin);
    assert_eq!(
        after_click, before,
        "switch_thread is the History click and must not move the row: {after_click:?}"
    );
    assert_eq!(cabin.thread_idx, older_idx);
    assert_eq!(
        cabin.threads[older_idx].accessed_ms, older_accessed,
        "the click must not bump accessed_ms"
    );

    // send_chat stamps the open thread with stamp_current_access. That is the
    // sent-turn bump. The click above did not call it.
    cabin.stamp_current_access();
    let after_stamp = history_row_titles(&cabin);
    assert_eq!(
        after_stamp,
        ["Last pin", "Earlier pin", "Older chat", "Newer chat"],
        "a sent turn moves that chat up, under the pins: {after_stamp:?}"
    );
    let stamped_at = after_stamp.iter().position(|t| t == "Older chat").unwrap();
    assert!(stamped_at < opened_at, "the messaged chat moved up");
    assert!(cabin.threads[older_idx].accessed_ms > older_accessed);

    let pins_before: Vec<_> = after_stamp
        .iter()
        .filter(|title| cabin.threads.iter().any(|t| t.title == **title && t.pinned))
        .cloned()
        .collect();
    assert_eq!(pins_before, ["Last pin", "Earlier pin"]);
    cabin.switch_thread(earlier_idx);
    let after_pin_click = history_row_titles(&cabin);
    let pins_after: Vec<_> = after_pin_click.iter().take(2).cloned().collect::<Vec<_>>();
    assert_eq!(
        pins_after, pins_before,
        "clicking a pinned chat leaves the pin group last-pinned-first: {after_pin_click:?}"
    );
    assert!(cabin.threads[earlier_idx].pinned);
    assert_eq!(cabin.threads[earlier_idx].pinned_ms, earlier_pin_ms);
    assert_eq!(
        cabin.threads[earlier_idx].accessed_ms, earlier_accessed,
        "clicking a pin is not activity"
    );
    assert_eq!(
        after_pin_click, after_stamp,
        "the pin click does not reorder History"
    );

    let start = std::time::Instant::now();
    while cabin.persist_rx.is_some() && start.elapsed() < std::time::Duration::from_secs(5) {
        cabin.poll_persist();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn park_fresh_chat_keeps_the_running_reply_and_opens_an_empty_chat() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("park-fresh");
    let _ = std::fs::remove_dir_all(&root);
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut live = crate::threads::ChatThread::new("Night watch", false);
    live.pinned = true;
    live.messages = std::sync::Arc::new(vec![
        ("user".into(), "keep this reply".into()),
        ("assistant".into(), "still writing".into()),
    ]);
    let live_id = live.id.clone();
    let mut cabin = super::Cabin::quiet_for_test();
    cabin.threads = vec![live];
    cabin.thread_idx = 0;
    cabin.messages = cabin.threads[0].messages.clone();
    cabin.running = true;
    cabin.chat_job_thread = Some(live_id.clone());
    cabin.park_fresh_chat();

    assert!(cabin.running, "parking must not halt the reply");
    assert_eq!(cabin.chat_job_thread.as_deref(), Some(live_id.as_str()));
    assert!(
        cabin.messages.is_empty(),
        "the open pane must be an empty chat"
    );
    assert_ne!(
        cabin.threads[cabin.thread_idx].id, live_id,
        "the empty chat must not be the thread that is still replying"
    );
    let old = cabin
        .threads
        .iter()
        .find(|t| t.id == live_id)
        .expect("the running thread stays in the list");
    assert!(old.pinned);
    assert_eq!(old.title, "Night watch");
    assert_eq!(
        old.messages.as_slice(),
        [
            ("user".into(), "keep this reply".into()),
            ("assistant".into(), "still writing".into()),
        ]
    );

    let io = cabin.persist_io.clone();
    drop(cabin);
    let start = std::time::Instant::now();
    while start.elapsed() < std::time::Duration::from_secs(3) {
        if io.try_lock().is_ok() {
            std::thread::sleep(std::time::Duration::from_millis(30));
            if io.try_lock().is_ok() {
                break;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let _ = std::fs::remove_dir_all(&root);
    std::env::remove_var("GROKHUB_CONFIG");
}

#[test]
fn selecting_plan_leaves_the_title_and_sets_plan() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("plan-title");
    let mut thread = crate::threads::ChatThread::new("Night watch", false);
    thread.messages = std::sync::Arc::new(vec![("user".into(), "dock".into())]);
    cabin.threads = vec![thread];
    cabin.thread_idx = 0;
    cabin.messages = cabin.threads[0].messages.clone();
    cabin.session_mode = SessionMode::Chat;

    cabin.select_plan_without_rename();

    assert_eq!(cabin.threads[0].title, "Night watch");
    assert_eq!(cabin.session_mode, SessionMode::Plan);
    release_isolated(&root, cabin);
}

#[test]
fn session_menu_sits_left_of_minimize() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("menu-place");
    let mut thread = crate::threads::ChatThread::new("Menu", false);
    thread.grok_session = Some("sess".into());
    cabin.threads = vec![thread];
    cabin.thread_idx = 0;
    let ctx = egui::Context::default();
    let raw = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(900.0, 80.0),
        )),
        ..Default::default()
    };
    let mut menu = egui::Rect::NOTHING;
    let mut mini = egui::Rect::NOTHING;
    let _ = crate::theme::test_pass(&ctx, raw, |ui| {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                mini = titlebar_chrome_btn(ui, ChromeBtn::Minimize).rect;
                menu = cabin.paint_session_actions_menu(ui);
            });
        });
    });
    assert!(
        menu.right() <= mini.left() + 0.5,
        "the session menu must sit left of Minimize: menu {menu:?} minimize {mini:?}"
    );
    release_isolated(&root, cabin);
}

#[test]
fn pin_thread_flips_the_pin() {
    let mut quiet = QuietCabin::boot("pin-thread");
    let cabin = &mut quiet.cabin;
    let idx = cabin.thread_idx;
    let title = cabin.threads[idx].title.clone();
    assert!(!cabin.threads[idx].pinned);
    cabin.pin_thread(idx);
    assert!(
        cabin.threads[idx].pinned,
        "the pin control must pin this chat"
    );
    assert!(cabin.threads[idx].pinned_ms > 0);
    assert_eq!(cabin.status, format!("Pinned {title}"));
    quiet.settle();
}

#[test]
fn composer_and_history_omit_archived_idle_chrome() {
    let src = cabin_src();
    let toast = fn_src(&src, "paint_archive_toast");
    assert!(
        !toast.contains("idle chat")
            && !toast.contains("idle chats")
            && !toast.contains("auto_archive_note")
            && !toast.contains("ghost_pill(ui, \"Archived\")")
            && !toast.contains("Nav::History"),
        "composer archive toast must not show idle-archive chrome: {toast}"
    );
    assert!(
        toast.contains("ghost_pill(ui, \"Undo\")"),
        "manual archive Undo toast stays: {toast}"
    );
    let live = src
        .split("ComposerStackSlot::LiveWork =>")
        .nth(1)
        .and_then(|s| s.split("ComposerStackSlot::SlashPalette =>").next())
        .expect("LiveWork slot");
    assert!(
        live.contains("paint_archive_toast") && !live.contains("idle chat"),
        "LiveWork must not embed idle-archive labels: {live}"
    );
    let hist = src
        .split("fn ui_history(")
        .nth(1)
        .and_then(|s| s.split("fn ui_board(").next())
        .expect("ui_history");
    assert!(
        !hist.contains("history_archived")
            && !hist.contains("\"Archived\"")
            && !hist.contains("Archived chats"),
        "History must not offer an Archived view: {hist}"
    );
    let side = include_str!("sidebar.rs");
    assert!(
        !side.contains("rail-archived")
            && !side.contains("RichText::new(\"Archived\")")
            && !side.contains("No archived chats"),
        "sidebar must not list Archived chats: {side}"
    );
    let kick = fn_src(&src, "kick_history_search");
    assert!(
        kick.contains("is_archived") && kick.contains("continue"),
        "History search must skip archived chats: {kick}"
    );
    let archive_row = fn_src(&src, "ui_chat_archive_row");
    assert!(
        !archive_row.contains("move to Archived")
            && archive_row.contains("leave History and search")
            && archive_row.contains("still on disk")
            && archive_row.contains("no Archived drawer")
            && !archive_row.contains("open the chat or Unarchive from its menu"),
        "Settings auto-archive must not promise an Archived drawer or post-leave recovery: {archive_row}"
    );
    let recall = src
        .split("Slash::Recall(q)")
        .nth(1)
        .and_then(|s| s.split("fn kick_model_retry").next())
        .expect("Recall");
    assert!(
        recall.contains("is_archived") && recall.contains("continue"),
        "/recall must skip archived chats like History search: {recall}"
    );
}

#[test]
fn archive_hides_from_main_list_and_reply_restores() {
    let mut quiet = QuietCabin::boot("chat-archive");
    let cabin = &mut quiet.cabin;
    cabin.threads.clear();
    let mut keep = crate::threads::ChatThread::new("Keep", false);
    keep.messages_mut().push(("user".into(), "stay".into()));
    keep.accessed_ms = 100;
    let mut done = crate::threads::ChatThread::new("Done chat", false);
    done.messages_mut().push(("user".into(), "old".into()));
    done.accessed_ms = 50;
    cabin.threads = vec![keep, done];
    cabin.thread_idx = 1;
    cabin.messages = cabin.threads[1].messages.clone();
    cabin.archive_thread_at(1);
    assert!(crate::threads::is_archived(&cabin.threads[1]));
    assert_eq!(
        crate::threads::chat_section_indices(&cabin.threads, Some(1), false),
        vec![0]
    );
    assert_eq!(cabin.status, "Archived Done chat");
    assert_eq!(
        cabin.archive_undo.as_deref(),
        Some(cabin.threads[1].id.as_str())
    );
    cabin.unarchive_on_reply();
    assert!(!crate::threads::is_archived(&cabin.threads[1]));
    assert_eq!(cabin.status, "Restored Done chat to chats");
    assert_eq!(
        crate::threads::chat_section_indices(&cabin.threads, Some(1), false),
        vec![0, 1]
    );
    quiet.settle();
}

#[test]
fn auto_archive_15_day_idle_skips_13_day_pinned_and_busy() {
    let mut quiet = QuietCabin::boot("auto-archive");
    let cabin = &mut quiet.cabin;
    let now = 30 * crate::threads::DAY_MS;
    cabin.cfg.chat_archive_days = 14;
    cabin.threads.clear();
    let mut idle15 = crate::threads::ChatThread::new("Fifteen", false);
    idle15.messages_mut().push(("user".into(), "hi".into()));
    idle15.accessed_ms = now - 15 * crate::threads::DAY_MS;
    let mut idle13 = crate::threads::ChatThread::new("Thirteen", false);
    idle13.messages_mut().push(("user".into(), "hi".into()));
    idle13.accessed_ms = now - 13 * crate::threads::DAY_MS;
    let mut pinned = crate::threads::ChatThread::new("Pinned", false);
    pinned.messages_mut().push(("user".into(), "hi".into()));
    pinned.accessed_ms = now - 20 * crate::threads::DAY_MS;
    pinned.pinned = true;
    let mut busy = crate::threads::ChatThread::new("Busy", false);
    busy.messages_mut().push(("user".into(), "hi".into()));
    busy.accessed_ms = now - 20 * crate::threads::DAY_MS;
    let busy_id = busy.id.clone();
    cabin.threads = vec![idle15, idle13, pinned, busy];
    cabin.thread_idx = 0;
    cabin.messages = cabin.threads[0].messages.clone();
    cabin.running = true;
    cabin.chat_job_thread = Some(busy_id);
    let n = cabin.run_auto_archive(now);
    assert_eq!(n, 1, "only the 15-day idle chat archives");
    assert!(crate::threads::is_archived(&cabin.threads[0]));
    assert!(!crate::threads::is_archived(&cabin.threads[1]));
    assert!(!crate::threads::is_archived(&cabin.threads[2]));
    assert!(!crate::threads::is_archived(&cabin.threads[3]));
    assert_eq!(cabin.status, "Archived 1 idle chat");
    quiet.settle();
}

#[test]
fn delete_thread_at_removes_that_row() {
    let mut quiet = QuietCabin::boot("delete-thread");
    let cabin = &mut quiet.cabin;
    let keep = cabin.threads[cabin.thread_idx].title.clone();
    cabin
        .threads
        .push(crate::threads::ChatThread::new("Harbor", false));
    let idx = cabin.threads.len() - 1;
    cabin.threads[idx].messages = std::sync::Arc::new(vec![("user".into(), "harbor row".into())]);
    cabin.delete_thread_at(idx);
    let harbor = cabin
        .threads
        .iter()
        .find(|t| t.title == "Harbor")
        .expect("soft-delete keeps Harbor on disk");
    assert!(
        crate::threads::is_deleted(harbor),
        "deleting that chat must soft-trash it"
    );
    let listed = crate::threads::chat_section_indices(&cabin.threads, None, false);
    assert!(
        listed.iter().all(|&i| cabin.threads[i].title != "Harbor"),
        "Harbor must leave the tab strip / History list"
    );
    assert!(cabin
        .threads
        .iter()
        .any(|t| t.title == keep && !crate::threads::is_deleted(t)));
    assert_eq!(cabin.status, "Deleted Harbor");
    quiet.settle();
}

#[test]
fn delete_all_history_clears_seeded_chats() {
    let _lock = crate::config::hold_test_config();
    let cfg = IsolatedConfig::arm("delete-all");
    let mut cabin = Cabin::quiet_for_test();
    let mut pier = crate::threads::ChatThread::new("Pier light", false);
    pier.messages_mut()
        .push(("user".into(), "bring the lamp".into()));
    let mut salt = crate::threads::ChatThread::new("Salt lane", false);
    salt.messages_mut()
        .push(("user".into(), "walk the lane".into()));
    cabin.threads = vec![pier, salt];
    cabin.thread_idx = 0;
    cabin.messages = cabin.threads[0].messages.clone();
    cabin.delete_all_history();
    // Drain the ordered writer before reading: polling the folder on a timer
    // raced slow Windows runners.
    join_persist(&cabin.persist_svc);
    assert_eq!(*cabin.persist_err.lock().unwrap(), None, "persist failed");
    let saved = std::fs::read_to_string(cfg.root.join("threads.json")).expect("threads.json");
    assert!(
        saved.contains("\"Main\""),
        "the fresh Main chat is saved: {saved}"
    );
    assert!(
        !saved.contains("Pier light") && !saved.contains("Salt lane"),
        "the pre-delete snapshot from halt_in_flight must not land last: {saved}"
    );
    assert!(
        cabin.threads.iter().all(|t| t.title != "Pier light"),
        "Pier light must be gone"
    );
    assert!(
        cabin.threads.iter().all(|t| t.title != "Salt lane"),
        "Salt lane must be gone"
    );
    assert_eq!(cabin.status, "Deleted all chats");
    assert_eq!(
        cabin.threads.len(),
        1,
        "the fresh Main chat is the only row"
    );
    assert_eq!(cabin.threads[0].title, "Main");
    assert!(cabin.threads[0].main);
    assert!(
        cabin.threads[0].messages.is_empty(),
        "the fresh chat has no transcript"
    );
    drop(cfg);
}

// Landed from PR #94.
#[test]
fn rename_thread_sets_locked_title() {
    let _g = config::hold_test_config();
    let root = config::test_config_root("rename-thread");
    let _ = std::fs::remove_dir_all(&root);
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    cabin.threads.push(ChatThread::new("Chat", false));
    cabin.rename_thread(0, "Harbor watch");
    assert_eq!(cabin.threads[0].title, "Harbor watch");
    assert!(cabin.threads[0].title_locked);
    assert_eq!(cabin.status, "Renamed Harbor watch");
    let _io = cabin.persist_io.lock().unwrap_or_else(|e| e.into_inner());
    std::env::remove_var("GROKHUB_CONFIG");
}

// Landed from PR #101.
#[test]
fn new_thread_opens_another_chat_when_the_current_one_has_a_message() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("new-chat");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    if cabin.threads.is_empty() {
        cabin
            .threads
            .push(crate::threads::ChatThread::new("Harbor", false));
        cabin.thread_idx = 0;
    }
    let line = std::sync::Arc::new(vec![("user".into(), "paint the harbor".into())]);
    for t in &mut cabin.threads {
        t.messages = line.clone();
    }
    cabin.messages = line;
    let before = cabin.threads.len();
    cabin.new_thread(false);
    assert_eq!(cabin.threads.len(), before + 1);
    assert_eq!(cabin.status, "New chat");
    assert!(cabin.messages.is_empty());
    assert_eq!(cabin.threads[cabin.thread_idx].title, "Chat");
    std::env::remove_var("GROKHUB_CONFIG");
}

// Landed from PR #102.
#[test]
fn open_history_hit_opens_pier_and_a_memory_file() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("history-open");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    cabin.threads.clear();
    let mut harbor = crate::threads::ChatThread::new("Harbor", false);
    let mut pier = crate::threads::ChatThread::new("Pier", false);
    let line = std::sync::Arc::new(vec![("user".into(), "paint the harbor".into())]);
    harbor.messages = line.clone();
    pier.messages = line.clone();
    let pier_id = pier.id.clone();
    cabin.threads.push(harbor);
    cabin.threads.push(pier);
    cabin.thread_idx = 0;
    cabin.messages = line;
    cabin.open_history_hit(&format!("thread:{pier_id}"));
    assert_eq!(cabin.threads[cabin.thread_idx].title, "Pier");
    assert!(matches!(cabin.nav, Nav::Chat));
    cabin.open_history_hit("thread:missing");
    assert_eq!(cabin.status, "That chat is gone");
    assert_eq!(cabin.threads[cabin.thread_idx].title, "Pier");
    cabin.open_history_hit("mem:USER.md");
    assert!(matches!(cabin.nav, Nav::Memory));
    assert_eq!(cabin.status, "USER.md");
    std::env::remove_var("GROKHUB_CONFIG");
}

// Landed from PR #146.
#[test]
fn grok_history_open_stays_off_a_run() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("grok-hist");
    let _ = std::fs::remove_dir_all(&root);
    std::env::set_var("GROKHUB_CONFIG", &root);

    let id = "sess-quiet-known";
    let title = "Known cabin title";
    let mut cabin = super::Cabin::quiet_for_test();
    cabin
        .grok_sessions
        .push(grokhub_core::cli_history::GrokSession {
            id: id.to_string(),
            title: title.to_string(),
            path: None,
            cwd: None,
            cabin: false,
        });

    cabin.open_grok_session(id);
    assert_eq!(cabin.threads.len(), 1);
    assert_eq!(cabin.threads[0].grok_session.as_deref(), Some(id));
    assert_eq!(cabin.threads[0].title, title);
    assert!(cabin.nav == super::Nav::Chat);
    assert_eq!(cabin.status, format!("Opened {title}"));
    assert!(!cabin.running);

    cabin.open_grok_session(id);
    assert_eq!(cabin.threads.len(), 1);
    assert_eq!(cabin.threads[0].grok_session.as_deref(), Some(id));
    assert!(!cabin.running);
}

// Landed from PR #171.
#[test]
fn history_hit_opens_memory_or_chat() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("history-hit");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = Cabin::quiet_for_test();
    assert!(!cabin.running);

    cabin.open_history_hit("mem:MEMORY.md");
    assert!(matches!(cabin.nav, Nav::Memory));
    assert_eq!(cabin.status, "MEMORY.md");
    assert_eq!(cabin.mem_name, "MEMORY.md");
    assert!(!cabin.running);

    cabin.open_history_hit("thread:nope");
    assert_eq!(cabin.status, "That chat is gone");
    assert!(!cabin.running);

    cabin.new_thread(false);
    cabin.new_thread(false);
    let id = cabin
        .threads
        .get(cabin.thread_idx)
        .map(|t| t.id.clone())
        .expect("seeded thread");
    cabin.open_history_hit(&format!("thread:{id}"));
    assert_eq!(cabin.threads[cabin.thread_idx].id, id);
    assert!(matches!(cabin.nav, Nav::Chat));
    assert!(!cabin.running);
}

// Landed from PR #172.
#[test]
fn delete_all_history_needs_a_second_click() {
    let _lock = crate::config::hold_test_config();
    let _cfg = IsolatedConfig::arm("delete-all-confirm");
    let mut cabin = Cabin::quiet_for_test();
    let mut pier = crate::threads::ChatThread::new("Pier light", false);
    pier.messages_mut()
        .push(("user".into(), "bring the lamp".into()));
    cabin.threads = vec![pier];
    cabin.thread_idx = 0;
    cabin.messages = cabin.threads[0].messages.clone();
    let t0 = std::time::Instant::now();

    assert!(!cabin.click_delete_all_history(t0), "first click only arms");
    assert!(cabin.history_delete_armed(t0));
    assert!(
        cabin.threads.iter().any(|t| t.title == "Pier light"),
        "one click must leave every chat in place"
    );

    // A click after the window lapsed re-arms instead of deleting.
    let late = t0 + super::DELETE_ALL_ARM + std::time::Duration::from_secs(1);
    assert!(
        !cabin.history_delete_armed(late),
        "the arm lapses on its own"
    );
    assert!(
        !cabin.click_delete_all_history(late),
        "a stale arm never deletes"
    );
    assert!(cabin.threads.iter().any(|t| t.title == "Pier light"));

    // Confirming inside the window deletes.
    let confirm = late + std::time::Duration::from_secs(2);
    assert!(
        cabin.click_delete_all_history(confirm),
        "second click deletes"
    );
    assert!(
        !cabin.threads.iter().any(|t| t.title == "Pier light"),
        "confirmed Delete all wipes the chats"
    );
    assert!(!cabin.history_delete_armed(confirm), "deleting disarms");
    join_persist(&cabin.persist_svc);
}

#[test]
fn delete_all_history_leaves_one_empty_chat() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("delete-all-history");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = super::Cabin::quiet_for_test();
    cabin.new_thread(false);
    cabin.messages = std::sync::Arc::new(vec![("user".into(), "note".into())]);
    cabin.new_thread(false);
    cabin.cfg.goal_pin = "harbor".into();
    let seeded = cabin
        .threads
        .iter()
        .position(|t| !t.messages.is_empty())
        .expect("seeded message");
    cabin.threads[seeded].grok_session = Some("sess".into());
    assert!(cabin.threads.len() >= 2);
    assert!(cabin
        .threads
        .iter()
        .any(|t| t.grok_session.as_deref() == Some("sess")));
    assert_eq!(cabin.cfg.goal_pin, "harbor");
    assert!(!cabin.running);

    cabin.delete_all_history();

    assert_eq!(cabin.status, "Deleted all chats");
    assert_eq!(cabin.threads.len(), 1);
    assert_eq!(cabin.threads[0].title, "Main");
    assert!(cabin.threads[0].main);
    assert!(cabin.threads[0].messages.is_empty());
    assert!(cabin.messages.is_empty());
    assert!(cabin.cfg.goal_pin.is_empty());
    assert!(cabin.grok_sessions.is_empty());
    assert!(!cabin.running);

    let _ = std::fs::remove_dir_all(&root);
    std::env::remove_var("GROKHUB_CONFIG");
}

// Landed from PR #214.
#[test]
fn rename_chat_stays_off_a_run() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("rename-chat");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    let prev = std::env::var("GROKHUB_CONFIG").ok();
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = Cabin::quiet_for_test();
    if cabin.threads.is_empty() {
        cabin.new_thread(false);
    }
    let thread_idx = cabin.thread_idx;
    assert_eq!(cabin.threads[thread_idx].title, "Chat");
    assert!(!cabin.running);

    cabin.rename_thread(thread_idx, "");
    assert_eq!(cabin.status, "Kept Chat");
    assert_eq!(cabin.threads[thread_idx].title, "Chat");
    assert!(!cabin.threads[thread_idx].title_locked);

    cabin.rename_thread(thread_idx, "Harbor");
    assert_eq!(cabin.status, "Renamed Harbor");
    assert_eq!(cabin.threads[thread_idx].title, "Harbor");
    assert!(cabin.threads[thread_idx].title_locked);
    assert!(!cabin.running);

    match prev {
        Some(v) => std::env::set_var("GROKHUB_CONFIG", v),
        None => std::env::remove_var("GROKHUB_CONFIG"),
    }
}

// Landed from PR #215.
#[test]
fn pin_chat_stays_off_a_run() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("pin-chat");
    let _ = std::fs::remove_dir_all(&root);
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = Cabin::quiet_for_test();
    if cabin.threads.is_empty() {
        cabin.new_thread(false);
    }
    assert_eq!(cabin.threads[cabin.thread_idx].title, "Chat");
    assert!(!cabin.running);

    cabin.pin_thread(cabin.thread_idx);
    assert_eq!(cabin.status, "Pinned Chat");
    assert!(cabin.threads[cabin.thread_idx].pinned);
    assert!(!cabin.running);

    cabin.pin_thread(cabin.thread_idx);
    assert_eq!(cabin.status, "Unpinned Chat");
    assert!(!cabin.threads[cabin.thread_idx].pinned);
    assert!(!cabin.running);
}

// Landed from PR #216.
#[test]
fn fresh_home_keeps_the_old_chat() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("fresh-home");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = Cabin::quiet_for_test();
    if cabin.threads.is_empty() {
        cabin.new_thread(false);
    }
    cabin.messages = std::sync::Arc::new(vec![("user".into(), "hello".into())]);

    cabin.open_fresh_home();

    assert_eq!(cabin.status, "New chat");
    assert!(cabin.messages.is_empty());
    let kept = cabin.threads.iter().enumerate().any(|(i, thread)| {
        i != cabin.thread_idx
            && thread
                .messages
                .iter()
                .any(|(role, line)| role == "user" && line == "hello")
    });
    assert!(kept);
    assert!(!cabin.running);
    assert!(matches!(cabin.nav, Nav::Chat));
}

// Landed from PR #217.
#[test]
fn delete_one_chat_stays_off_a_run() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("delete-one-chat");
    let _ = std::fs::remove_dir_all(&root);
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = super::Cabin::quiet_for_test();
    if cabin.threads.is_empty() {
        cabin.new_thread(false);
    }
    cabin.threads[cabin.thread_idx].title = "Harbor".into();
    cabin.messages = Arc::new(vec![("user".into(), "hello".into())]);
    cabin.new_thread(false);

    let harbor = cabin
        .threads
        .iter()
        .position(|t| t.title == "Harbor")
        .expect("Harbor");
    cabin.delete_thread_at(harbor);

    assert_eq!(cabin.status, "Deleted Harbor");
    let harbor_row = cabin
        .threads
        .iter()
        .find(|t| t.title == "Harbor")
        .expect("soft-delete keeps Harbor on disk");
    assert!(crate::threads::is_deleted(harbor_row));
    assert!(!crate::threads::is_deleted(
        &cabin.threads[cabin.thread_idx]
    ));
    assert_eq!(cabin.threads[cabin.thread_idx].title, "Chat");
    assert!(!cabin.running);
    assert!(cabin.messages.is_empty());

    std::env::remove_var("GROKHUB_CONFIG");
}

// Landed from PR #218.
#[test]
fn scratch_chat_stays_off_a_run() {
    let _guard = crate::config::hold_test_config();
    let root = crate::config::test_config_root("scratch-chat");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    assert!(!cabin.running);
    cabin.new_thread(true);
    assert_eq!(cabin.status, "Scratch — no memory writes");
    let thread = cabin.threads.get(cabin.thread_idx).expect("current thread");
    assert_eq!(thread.title, "Scratch");
    assert!(thread.scratch);
    assert!(!cabin.running);
}

// Landed from PR #219.
#[test]
fn delete_last_chat_stays_off_a_run() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("delete-last");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    if cabin.threads.is_empty() {
        cabin.new_thread(false);
    }
    cabin.delete_thread_at(0);
    assert_eq!(cabin.status, "Chat deleted");
    assert_eq!(cabin.threads.len(), 1);
    assert_eq!(cabin.threads[0].title, "Chat");
    assert!(!cabin.running);
    std::env::remove_var("GROKHUB_CONFIG");
}

// Landed from PR #220.
#[test]
fn land_on_real_chat_leaves_scratch() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("land-chat");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    if cabin.threads.is_empty() {
        cabin.new_thread(false);
    }
    cabin.new_thread(true);
    let current = &cabin.threads[cabin.thread_idx];
    assert_eq!(current.title, "Scratch");
    assert!(current.scratch);
    assert!(cabin
        .threads
        .iter()
        .enumerate()
        .any(|(i, t)| { i != cabin.thread_idx && t.title == "Chat" && !t.scratch }));
    assert_eq!(cabin.status, "Scratch — no memory writes");
    cabin.land_on_real_chat();
    let current = &cabin.threads[cabin.thread_idx];
    assert_eq!(current.title, "Chat");
    assert!(!current.scratch);
    assert!(matches!(cabin.nav, Nav::Chat));
    assert!(cabin
        .threads
        .iter()
        .enumerate()
        .any(|(i, t)| { i != cabin.thread_idx && t.title == "Scratch" && t.scratch }));
    assert_eq!(cabin.status, "Scratch — no memory writes");
    assert!(!cabin.running);
}

#[test]
fn chat_rail_home_round_trip_restores_last_tab() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("chat-rail-last-used");
    let _ = std::fs::remove_dir_all(&root);
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    cabin.threads.clear();

    let mut older = crate::threads::ChatThread::new("Older", false);
    older.accessed_ms = 1_000;
    older.messages = std::sync::Arc::new(vec![("user".into(), "old".into())]);
    let older_id = older.id.clone();
    let mut newer = crate::threads::ChatThread::new("Harbor", false);
    newer.accessed_ms = 5_000;
    newer.messages = std::sync::Arc::new(vec![("user".into(), "dock".into())]);
    let newer_id = newer.id.clone();
    cabin.threads.push(older);
    cabin.threads.push(newer);
    cabin.thread_idx = 1;
    cabin.messages = cabin.threads[1].messages.clone();
    cabin.remember_last_chat_tab();
    assert_eq!(cabin.cfg.last_chat_tab, newer_id);

    cabin.open_fresh_home();
    assert!(cabin.messages.is_empty(), "Home parks on an empty draft");
    assert_eq!(cabin.cfg.last_chat_tab, newer_id);
    assert_ne!(cabin.threads[cabin.thread_idx].id, newer_id);

    cabin.set_nav_id("chat");
    assert_eq!(cabin.threads[cabin.thread_idx].id, newer_id);
    assert_eq!(cabin.messages[0].1, "dock");

    // Closed remembered tab → next live (Older), not the Home draft.
    let harbor_idx = cabin.thread_idx;
    cabin.delete_thread_at(harbor_idx);
    assert!(crate::threads::is_deleted(
        cabin
            .threads
            .iter()
            .find(|t| t.id == newer_id)
            .expect("soft-deleted Harbor stays on disk")
    ));
    cabin.open_fresh_home();
    cabin.set_nav_id("chat");
    assert_eq!(cabin.threads[cabin.thread_idx].id, older_id);
    assert_eq!(cabin.messages[0].1, "old");

    // None remain → fresh draft.
    let older_idx = cabin.thread_idx;
    cabin.delete_thread_at(older_idx);
    cabin.cfg.last_chat_tab.clear();
    // Drop leftover parking drafts so restore has nothing live.
    for t in cabin.threads.iter_mut() {
        if crate::threads::is_parking_chat_draft(t) || t.messages.is_empty() {
            let _ = crate::threads::soft_delete_thread(t, 99_000);
        }
    }
    cabin.set_nav_id("chat");
    assert!(
        cabin.threads[cabin.thread_idx].title == "Chat"
            && cabin.messages.is_empty()
            && !crate::threads::is_deleted(&cabin.threads[cabin.thread_idx]),
        "no live tabs opens a fresh chat"
    );
    assert!(!cabin.running);
    std::env::remove_var("GROKHUB_CONFIG");
}

// Landed from PR #222.
#[test]
fn recent_chat_stays_off_a_run() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("recent-chat");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    if cabin.threads.is_empty() {
        cabin.new_thread(false);
    }
    let idx = cabin.thread_idx;
    assert_eq!(cabin.threads[idx].title, "Chat");
    assert_eq!(cabin.status, "New chat");
    cabin.open_recent_chat();
    assert_eq!(cabin.threads[cabin.thread_idx].title, "Chat");
    assert_eq!(cabin.thread_idx, idx);
    assert!(cabin.composer_want_focus);
    assert_eq!(cabin.status, "New chat");
    assert!(!cabin.running);
    assert_eq!(cabin.threads.len(), 1);
    std::env::remove_var("GROKHUB_CONFIG");
}

// Landed from PR #223.
#[test]
fn begin_rename_stays_off_a_run() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("begin-rename");
    let _ = std::fs::remove_dir_all(&root);
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    if cabin.threads.is_empty() {
        cabin.new_thread(false);
    }
    let thread_idx = cabin.thread_idx;
    assert_eq!(cabin.threads[thread_idx].title, "Chat");
    assert!(!cabin.threads[thread_idx].title_locked);
    assert_eq!(cabin.status, "New chat");
    assert!(!cabin.running);
    cabin.begin_chat_rename(thread_idx);
    assert_eq!(cabin.rename_idx, Some(thread_idx));
    assert!(cabin.rename_focus);
    assert_eq!(cabin.rename_buf, "Chat");
    assert_eq!(cabin.rename_lock, Some("Chat".into()));
    assert_eq!(cabin.threads[thread_idx].title, "Chat");
    assert!(!cabin.threads[thread_idx].title_locked);
    assert_eq!(cabin.status, "New chat");
    assert!(!cabin.running);
}

// Landed from PR #226.
#[test]
fn switch_chat_keeps_the_line() {
    let _cfg = crate::config::hold_test_config();
    let root = crate::config::test_config_root("switch-chat-keeps");
    let _ = std::fs::remove_dir_all(&root);
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut app = Cabin::quiet_for_test();
    let mut held = crate::threads::ChatThread::new("Chat", false);
    held.messages = std::sync::Arc::new(vec![("user".into(), "older line".into())]);
    app.threads.push(held);
    app.thread_idx = 0;
    app.messages = app.threads[0].messages.clone();
    app.new_thread(false);
    let left = app.thread_idx;
    assert!(left > 0);
    app.live_mut().push(("user".into(), "harbor line".into()));
    app.apply_switch_thread(0);
    assert_eq!(app.thread_idx, 0);
    assert!(app.messages.iter().all(|(_, c)| c != "harbor line"));
    assert!(app.threads[left]
        .messages
        .iter()
        .any(|(_, c)| c == "harbor line"));
    assert!(app.composer_want_focus);
    assert!(app.rename_idx.is_none());
    assert!(!app.running);
    assert!(app.chat_job_thread.is_none());
    assert_eq!(app.status, "New chat");

    std::env::remove_var("GROKHUB_CONFIG");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn edit_puts_your_message_back_without_dropping_a_draft() {
    assert_eq!(
        super::chat_ui::edit_into_composer("", "fix the tests"),
        "fix the tests"
    );
    assert_eq!(
        super::chat_ui::edit_into_composer("  \n", "fix the tests"),
        "fix the tests"
    );
    assert_eq!(
        super::chat_ui::edit_into_composer("half typed\n", "fix the tests"),
        "half typed\n\nfix the tests"
    );
    assert_eq!(
        super::chat_ui::msg_act_labels(true),
        &["Copy", "Edit", "Reply"]
    );
    assert_eq!(super::chat_ui::msg_act_labels(false), &["Copy", "Reply"]);
    let src = cabin_src();
    let chat = fn_src(&src, "ui_chat");
    assert!(chat.contains("ChatBlockAct::Edit(body)"), "{chat}");
    assert!(chat.contains("edit_into_composer"), "{chat}");
}

#[test]
fn user_act_row_with_edit_stays_inside_a_narrow_pane() {
    for width in [300.0_f32, 360.0, 800.0] {
        let ctx = egui::Context::default();
        let _ = crate::theme::test_pass(&ctx, Default::default(), |ui| {
            let ctx = ui.ctx().clone();
            crate::theme::apply(&ctx, true);
            egui::CentralPanel::default().show(ui, |ui| {
                ui.allocate_ui(egui::vec2(width, 200.0), |ui| {
                    ui.set_max_width(width);
                    let row = ui.max_rect();
                    let bubble = super::chat_ui::paint_speech_bubble(ui, "hey", true, false);
                    let acts = super::chat_ui::paint_msg_acts(
                        ui,
                        true,
                        "hey",
                        row.width(),
                        bubble.rect.width(),
                    );
                    assert!(
                        acts.row.min.x >= row.min.x - 0.5,
                        "width {width}: {:?}",
                        acts.row
                    );
                    assert!(
                        acts.row.max.x <= row.max.x + 1.0,
                        "width {width}: {:?}",
                        acts.row
                    );
                    let want = super::chat_ui::msg_acts_row_width(ui, true);
                    assert!(
                        acts.row.width() + 1.0 >= want,
                        "width {width}: Edit row clipped {} < {want}",
                        acts.row.width()
                    );
                });
            });
        });
    }
}

fn find_views(bodies: &[(&str, &str)]) -> Vec<ChatView> {
    grokhub_core::visible_chat_refs(bodies.iter().copied())
}

#[test]
fn chat_find_starts_on_the_newest_hit_and_wraps() {
    let views = find_views(&[
        ("user", "build the cabin"),
        ("assistant", "Building it."),
        ("user", "now test"),
        ("assistant", "Tests pass. Build is green."),
    ]);
    let mut find = super::chat_ui::ChatFind::default();
    find.toggle();
    assert!(find.open && find.want_focus);
    find.query = "BUILD".into();
    find.refresh("t1", &views);
    let hits = find.hits().to_vec();
    assert_eq!(hits.len(), 3, "{hits:?}");
    assert_eq!(find.current_row(), hits.last().copied());
    assert!(find.jump, "a new query scrolls to its hit");
    assert_eq!(find.label(), "3 of 3");
    find.jump = false;
    find.step(true);
    assert_eq!(find.current_row(), Some(hits[0]));
    assert!(find.jump);
    find.step(false);
    assert_eq!(find.label(), "3 of 3");
    // Same query, same transcript: no recount, pick stays.
    find.step(false);
    find.refresh("t1", &views);
    assert_eq!(find.label(), "2 of 3");
    find.query = "zzz".into();
    find.refresh("t1", &views);
    assert_eq!(find.label(), "No matches");
    assert!(find.current_row().is_none());
    find.close();
    assert!(!find.open && !find.focused);
    assert!(find.hits().is_empty(), "a closed find marks nothing");
}

#[test]
fn chat_find_is_ctrl_f_on_chat_and_owns_enter_over_a_permission_card() {
    let src = cabin_src();
    assert!(src.contains("egui::Key::F"), "Ctrl+F must open find");
    assert!(src.contains("self.find.toggle()"));
    let chat = fn_src(&src, "ui_chat");
    assert!(chat.contains("paint_find_bar"), "{chat}");
    assert!(chat.contains("find_jump && find_row == Some(i)"), "{chat}");
    assert!(
        src.contains("self.palette_open || self.nav == Nav::Settings || self.find.focused"),
        "Enter in the find box must not allow a tool"
    );
    let confirm = include_str!("confirm.rs");
    assert!(
        confirm.contains("self.find.focused"),
        "Enter in the find box must not confirm a sheet"
    );
    assert!(grokhub_core::shortcut_help().contains("Ctrl+F"));
}

#[test]
fn chat_find_bar_paints_and_steps_with_the_arrows() {
    let mut app = Cabin::quiet_for_test();
    app.find.toggle();
    app.find.query = "x".into();
    let views = find_views(&[("user", "x one"), ("assistant", "x two")]);
    app.find.refresh("t", &views);
    let ctx = egui::Context::default();
    let _ = crate::theme::test_pass(&ctx, Default::default(), |ui| {
        let ctx = ui.ctx().clone();
        crate::theme::apply(&ctx, true);
        app.paint_find_bar(
            &ctx,
            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0)),
        );
    });
    assert!(app.find.open);
    assert_eq!(app.find.label(), "2 of 2");
}

#[test]
fn tab_plus_shows_the_new_chat_as_a_selected_tab() {
    let _lock = crate::config::hold_test_config();
    let root = crate::config::test_config_root("tab-plus");
    let _ = std::fs::remove_dir_all(&root);
    let _cfg = RestoreEnv::set("GROKHUB_CONFIG", &root);
    let _tray = RestoreEnv::set("GROKHUB_TRAY", "0");

    let mut cabin = Cabin::quiet_cabin(vec![spoke("Harbor plan", 100)], 0);
    assert_eq!(cabin.chat_tab_indices(), vec![0]);

    cabin.new_thread(false);
    let fresh = cabin.thread_idx;
    assert_ne!(fresh, 0, "+ opens a new draft beside the spoken chat");
    assert!(cabin.messages.is_empty(), "the new draft starts empty");
    let tabs = cabin.chat_tab_indices();
    assert_eq!(tabs.len(), 2, "the empty draft is a tab: {tabs:?}");
    assert!(tabs.contains(&0) && tabs.contains(&fresh), "{tabs:?}");
    assert_eq!(cabin.chat_tab_title(fresh), "New chat");
    assert_eq!(cabin.chat_tab_title(0), "Harbor plan");

    // History still hides the empty draft; only the strip shows it.
    assert_eq!(history_row_titles(&cabin), ["Harbor plan"]);

    // A second + reuses the same draft instead of stacking tabs.
    cabin.new_thread(false);
    assert_eq!(cabin.thread_idx, fresh);
    assert_eq!(cabin.chat_tab_indices().len(), 2);

    // An empty draft that is not open stays off the strip.
    cabin.switch_thread(0);
    assert_eq!(cabin.chat_tab_indices(), vec![0]);

    // An open project chat with messages stays in its project, off the strip.
    let mut harbor = spoke("Project harbor", 50);
    harbor.project_id = Some("proj-harbor".into());
    cabin.threads.push(harbor);
    let project_chat = cabin.threads.len() - 1;
    cabin.switch_thread(project_chat);
    assert!(!cabin.messages.is_empty(), "the project chat has messages");
    assert_eq!(cabin.chat_tab_indices(), vec![0]);
    let _ = std::fs::remove_dir_all(&root);
}

fn finished_scheduled_run(thread_id: &str, say: &str) -> super::background::BgRun {
    super::background::BgRun {
        id: 1,
        thread_id: thread_id.to_string(),
        title: "Tidy downloads".into(),
        origin: grokhub_core::BgOrigin::Scheduled,
        rx: None,
        say: say.to_string(),
        action: String::new(),
        started: std::time::Instant::now(),
        end: Some(grokhub_core::BgEnd::Done),
        session: String::new(),
        resumed: None,
        fork_hold: false,
        native_session: None,
        automation: None,
    }
}

#[test]
fn main_chat_is_listed_first_and_cannot_be_archived_deleted_or_pinned() {
    let _lock = crate::config::hold_test_config();
    let root = crate::config::test_config_root("main-chat");
    let _ = std::fs::remove_dir_all(&root);
    let _cfg = RestoreEnv::set("GROKHUB_CONFIG", &root);
    let _tray = RestoreEnv::set("GROKHUB_TRAY", "0");

    let mut pinned = spoke("Night watch", 900);
    pinned.pinned = true;
    pinned.pinned_ms = 900;
    let mut list = vec![spoke("Harbor plan", 500), pinned];
    let main_idx = crate::threads::ensure_main_thread(&mut list);
    assert_eq!(main_idx, 2);
    assert_eq!(crate::threads::ensure_main_thread(&mut list), 2, "one Main");
    let mut cabin = Cabin::quiet_cabin(list, 0);

    // Empty, it still lists, above the pinned chat.
    assert_eq!(
        history_row_titles(&cabin),
        ["Main", "Night watch", "Harbor plan"]
    );
    let order = cabin.chat_tab_order(&cabin.chat_tab_indices());
    assert_eq!(
        cabin.chat_tab_indices()[order[0]],
        2,
        "Main leads the strip"
    );

    cabin.archive_thread_at(2);
    assert_eq!(cabin.threads[2].archived_at, None);
    assert_eq!(
        cabin.status,
        "Main is your permanent chat and always stays put"
    );
    cabin.delete_thread_at(2);
    assert_eq!(cabin.threads[2].deleted_at, None);
    cabin.hard_drop_thread_at(2);
    assert_eq!(cabin.threads.len(), 3);
    cabin.pin_thread(2);
    assert!(!cabin.threads[2].pinned);
    cabin.begin_chat_rename(2);
    assert_eq!(cabin.rename_idx, None);

    // + never hands Main out as the fresh draft.
    cabin.switch_thread(2);
    cabin.new_thread(false);
    assert_ne!(cabin.thread_idx, 2);
    assert!(cabin.threads[2].main);

    // Delete all keeps a fresh, empty Main and opens it.
    cabin.delete_all_history();
    assert_eq!(cabin.threads.len(), 1);
    assert!(cabin.threads[0].main);
    assert_eq!(cabin.threads[0].title, "Main");
    assert_eq!(cabin.thread_idx, 0);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn main_chat_repairs_a_bad_saved_row() {
    let mut first = crate::threads::ChatThread::new("Main", false);
    first.main = true;
    first.archived_at = Some(5);
    first.deleted_at = Some(6);
    first.project_id = Some("proj".into());
    let mut second = crate::threads::ChatThread::new("Main copy", false);
    second.main = true;
    let mut list = vec![first, second];
    assert_eq!(crate::threads::ensure_main_thread(&mut list), 0);
    assert_eq!(list[0].archived_at, None);
    assert_eq!(list[0].deleted_at, None);
    assert_eq!(list[0].project_id, None);
    assert!(list[0].title_locked);
    assert!(!list[1].main, "only one Main");
    assert!(!crate::threads::should_auto_archive(
        &list[0],
        u64::MAX,
        1,
        false
    ));
}

#[test]
fn scheduled_and_orphan_background_reports_post_in_main() {
    let _lock = crate::config::hold_test_config();
    let root = crate::config::test_config_root("main-report");
    let _ = std::fs::remove_dir_all(&root);
    let _cfg = RestoreEnv::set("GROKHUB_CONFIG", &root);
    let _tray = RestoreEnv::set("GROKHUB_TRAY", "0");

    let mut list = vec![spoke("Harbor plan", 500)];
    let main_idx = crate::threads::ensure_main_thread(&mut list);
    let mut cabin = Cabin::quiet_cabin(list, 0);
    let bg_idx = cabin.ensure_background_history_thread();
    let bg_id = cabin.threads[bg_idx].id.clone();

    cabin
        .bg
        .runs
        .push(finished_scheduled_run(&bg_id, "Moved 4 files."));
    cabin.post_finished_bg_runs();
    let main = &cabin.threads[main_idx];
    assert_eq!(
        main.messages.last().map(|m| m.1.as_str()),
        Some("**Background task done** · Tidy downloads\n\nMoved 4 files.")
    );
    assert_eq!(
        cabin.bg.unread,
        vec![(
            main.id.clone(),
            "- Tidy downloads (done): Moved 4 files.".to_string()
        )]
    );
    assert_eq!(cabin.threads[bg_idx].messages.len(), 1, "hidden copy kept");

    // A /bg run whose chat is gone reports in Main instead of vanishing.
    let mut orphan = finished_scheduled_run("thr-gone", "All clear.");
    orphan.origin = grokhub_core::BgOrigin::User;
    orphan.title = "Check the logs".into();
    cabin.bg.runs.push(orphan);
    cabin.post_finished_bg_runs();
    assert_eq!(
        cabin.threads[main_idx]
            .messages
            .last()
            .map(|m| m.1.as_str()),
        Some("**Background task done** · Check the logs\n\nAll clear.")
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// Stop used to drop the half-streamed reply: the text lived only in the stream
/// buffers and halt_in_flight cleared them, then popped the assistant row.
#[test]
fn stop_keeps_the_reply_that_already_streamed() {
    let _lock = crate::config::hold_test_config();
    let _cfg = IsolatedConfig::arm("stop-keeps-reply");
    let mut cabin = Cabin::quiet_for_test();
    let mut chat = crate::threads::ChatThread::new("Stutter", false);
    chat.messages_mut()
        .push(("user".into(), "why does my game stutter".into()));
    let tid = chat.id.clone();
    cabin.threads = vec![chat];
    cabin.thread_idx = 0;
    cabin.messages = cabin.threads[0].messages.clone();
    cabin.running = true;
    cabin.chat_job_thread = Some(tid);
    cabin.ingest_stream_chunk(LiveKind::Say, "Shader cache rebuilds on every launch, so");
    cabin.halt_in_flight();
    let last = cabin.messages.last().expect("a row");
    assert_eq!(last.0, "assistant", "{:?}", cabin.messages);
    assert!(
        last.1.contains("Shader cache rebuilds") && last.1.ends_with("*Stopped*"),
        "the partial reply stays, marked stopped: {:?}",
        last.1
    );
    assert!(cabin.stream_buf.is_empty() && cabin.turn_log.is_empty());
    assert!(!cabin.running);
}

#[test]
fn stop_keeps_the_tool_calls_of_the_run() {
    let _lock = crate::config::hold_test_config();
    let _cfg = IsolatedConfig::arm("stop-keeps-tools");
    let mut cabin = Cabin::quiet_for_test();
    let mut chat = crate::threads::ChatThread::new("Disk", false);
    chat.messages_mut()
        .push(("user".into(), "what is eating my disk".into()));
    let tid = chat.id.clone();
    cabin.threads = vec![chat];
    cabin.thread_idx = 0;
    cabin.messages = cabin.threads[0].messages.clone();
    cabin.running = true;
    cabin.chat_job_thread = Some(tid);
    cabin.ingest_stream_chunk(LiveKind::Say, "Checking the big folders.");
    append_tool(&mut cabin.turn_log, "t1", "Scan Downloads", "completed", "");
    cabin.ingest_stream_chunk(LiveKind::Say, "Downloads holds 40 GB of");
    let expected = {
        let mut log = cabin.turn_log.clone();
        append_say(&mut log, STOPPED_TURN_SEAM);
        encode_turn(&log)
    };
    cabin.halt_in_flight();
    let last = cabin.messages.last().expect("a row");
    assert_eq!(last.0, "assistant");
    assert_eq!(
        last.1, expected,
        "the whole timeline, tools in place, is kept"
    );
    assert!(last.1.contains("Scan Downloads") && last.1.contains("*Stopped*"));
}

#[test]
fn stop_before_anything_streamed_leaves_no_empty_bubble() {
    let _lock = crate::config::hold_test_config();
    let _cfg = IsolatedConfig::arm("stop-nothing-yet");
    let mut cabin = Cabin::quiet_for_test();
    let mut chat = crate::threads::ChatThread::new("Duo", false);
    chat.messages_mut()
        .push(("user".into(), "find me a duo".into()));
    let tid = chat.id.clone();
    cabin.threads = vec![chat];
    cabin.thread_idx = 0;
    cabin.messages = cabin.threads[0].messages.clone();
    cabin.running = true;
    cabin.chat_job_thread = Some(tid);
    cabin.halt_in_flight();
    assert_eq!(
        cabin.messages.last().map(|m| m.0.as_str()),
        Some("user"),
        "{:?}",
        cabin.messages
    );
}

#[test]
fn stop_keeps_a_run_streaming_on_another_tab_in_that_tab() {
    let _lock = crate::config::hold_test_config();
    let _cfg = IsolatedConfig::arm("stop-other-tab");
    let mut cabin = Cabin::quiet_for_test();
    let here = crate::threads::ChatThread::new("Here", false);
    let mut there = crate::threads::ChatThread::new("There", false);
    there
        .messages_mut()
        .push(("user".into(), "plan my weekend".into()));
    let there_id = there.id.clone();
    cabin.threads = vec![here, there];
    cabin.thread_idx = 0;
    cabin.messages = cabin.threads[0].messages.clone();
    cabin.running = true;
    cabin.chat_job_thread = Some(there_id.clone());
    cabin.ingest_stream_chunk(LiveKind::Say, "Saturday: ranked grind, then");
    cabin.halt_in_flight();
    assert!(
        cabin.messages.is_empty(),
        "nothing lands on the visible tab"
    );
    let there = cabin
        .threads
        .iter()
        .find(|t| t.id == there_id)
        .expect("there");
    let last = there.messages.last().expect("a row");
    assert_eq!(last.0, "assistant");
    assert!(last.1.contains("ranked grind") && last.1.ends_with("*Stopped*"));
}
