//! Chat state on an idle cabin tests (rearch stage 5).
//!
//! Mechanical move from `tests.rs`. Wired via `#[path]` so
//! `app/mod.rs` stays untouched.

use super::*;
use eframe::egui;

// ---- Cursor fold: Chat state on an idle cabin ----

// Folded from PR #273.
#[test]
fn offscreen_chat_row_stays_outside_the_clip() {
    let clip = egui::Rect::from_min_max(egui::pos2(0.0, 100.0), egui::pos2(200.0, 300.0));
    assert!(chat_row_outside_clip(
        egui::pos2(0.0, 0.0),
        100.0,
        50.0,
        clip
    ));
    assert!(chat_row_outside_clip(
        egui::pos2(0.0, 300.0),
        100.0,
        40.0,
        clip
    ));
    assert!(chat_row_outside_clip(
        egui::pos2(-80.0, 120.0),
        40.0,
        40.0,
        clip
    ));
    assert!(chat_row_outside_clip(
        egui::pos2(200.0, 120.0),
        40.0,
        40.0,
        clip
    ));
    assert!(!chat_row_outside_clip(
        egui::pos2(10.0, 150.0),
        100.0,
        40.0,
        clip
    ));
}

// Folded from PR #282.
#[test]
fn chip_pairs_cap_a_long_reply() {
    let mut cabin = Cabin::quiet_for_test();
    let long = "a".repeat(8000);
    cabin.messages = Arc::new(vec![
        ("user".into(), "Harbor".into()),
        ("assistant".into(), long.clone()),
    ]);

    let chips = cabin.chip_chat_pairs();
    let scanned = grokhub_core::chip_scan(&long);
    assert_eq!(chips.len(), 2);
    assert_eq!(chips[0].0, "user");
    assert_eq!(chips[0].1, "Harbor");
    assert_eq!(chips[1].0, "assistant");
    assert_eq!(chips[1].1.len(), scanned.len());
    assert_eq!(scanned.len(), 4096);
    assert!(chips[1].1.chars().all(|c| c == 'a'));
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
}

// Folded from PR #284.
/// A painted greeting leaves as soon as the chat has text, or the open thread is scratch.
/// Both paths return before the memory flush and the USER/MEMORY file read.
#[test]
fn greeting_hides_once_the_chat_has_text() {
    let mut cabin = Cabin::quiet_for_test();
    cabin.greeting = "Evening, User.".into();
    cabin.messages = Arc::new(vec![("user".into(), "Harbor".into())]);
    cabin.refresh_greeting();
    assert!(
        cabin.greeting.is_empty(),
        "a chat that already has text must clear the greeting"
    );
    assert!(cabin.greeting_files_rx.is_none());
    assert!(cabin.greeting_rx.is_none());
    assert!(!cabin.greeting_busy);

    cabin.greeting = "Evening, User.".into();
    cabin.messages = Arc::new(Vec::new());
    cabin.threads.push(ChatThread::new("Scratch", true));
    cabin.thread_idx = cabin.threads.len() - 1;
    cabin.refresh_greeting();
    assert!(
        cabin.greeting.is_empty(),
        "a scratch thread must clear the greeting"
    );
    assert!(cabin.greeting_files_rx.is_none());
    assert!(cabin.greeting_rx.is_none());
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
}

// Folded from PR #287.
#[test]
fn project_flush_stays_off_when_clean() {
    let mut app = Cabin::quiet_for_test();
    app.status = "Harbor".into();
    assert!(!app.projects_dirty);
    app.flush_projects();
    assert!(!app.projects_dirty);
    assert_eq!(app.status, "Harbor");

    app.touch_projects();
    assert!(app.projects_dirty);
    assert_eq!(app.status, "Harbor");
    // Leave dirty. Do not flush while dirty.
    assert!(!app.running);
    assert!(app.chat_job_thread.is_none());
}

// Folded from PR #329.
#[test]
fn abandon_turn_card_stays_off_when_idle() {
    let mut cabin = Cabin::quiet_for_test();
    cabin.status = "Harbor".into();
    assert!(!cabin.inflight_open);
    assert!(cabin.board.is_empty());
    cabin.abandon_turn_card();
    assert!(!cabin.inflight_open);
    assert_eq!(cabin.status, "Harbor");
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
    assert!(cabin.board.is_empty());
}

// Folded from PR #332.
#[test]
fn ensure_background_history_thread_reuses_one_row() {
    let mut cabin = Cabin::quiet_for_test();
    cabin.status = "Harbor".into();
    let first = cabin.ensure_background_history_thread();
    let second = cabin.ensure_background_history_thread();
    assert_eq!(first, second);
    assert_eq!(
        cabin.threads.iter().filter(|t| t.background).count(),
        1,
        "ensure must reuse one Background row"
    );
    let thread = &cabin.threads[first];
    assert!(thread.background);
    assert_eq!(thread.title, "Background");
    assert!(!cabin.running);
    assert_ne!(cabin.status, "Thinking…");
    assert_eq!(cabin.status, "Harbor");
    assert!(cabin.chat_job_thread.is_none());
}

// Folded from PR #335.
#[test]
fn stamp_current_access_updates_open_thread() {
    let mut cabin = Cabin::quiet_for_test();
    if cabin.threads.is_empty() {
        cabin
            .threads
            .push(crate::threads::ChatThread::new("Chat", false));
        cabin.thread_idx = 0;
    }
    cabin.status = "Harbor".into();
    let idx = cabin.thread_idx;
    cabin.threads[idx].accessed_ms = 1;
    let before = cabin.threads[idx].accessed_ms;
    cabin.stamp_current_access();
    assert!(
        cabin.threads[idx].accessed_ms > before,
        "stamp_current_access must bump accessed_ms past {before}, got {}",
        cabin.threads[idx].accessed_ms
    );
    assert_eq!(cabin.status, "Harbor");
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
    assert_eq!(cabin.thread_idx, idx);
}

// Folded from PR #336.
#[test]
fn pin_chat_tail_keeps_scroll_intent() {
    let mut cabin = Cabin::quiet_for_test();
    cabin.status = "Harbor".into();
    assert_eq!(cabin.chat_tail_frames, 0);
    cabin.pin_chat_tail();
    assert_eq!(cabin.chat_tail_frames, grokhub_core::CHAT_TAIL_FRAMES);
    assert_eq!(cabin.status, "Harbor");
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
}

// Folded from PR #337.
#[test]
fn job_on_background_thread_false_when_idle() {
    let mut cabin = Cabin::quiet_for_test();
    cabin.status = "Harbor".into();
    // quiet_for_test: chat_job_thread None → idle false.
    assert!(cabin.chat_job_thread.is_none());
    assert!(!cabin.job_on_background_thread());
    assert_eq!(cabin.status, "Harbor");
    assert!(!cabin.running);

    // chat_job_thread pointing at a background=true thread → true.
    let idx = cabin.ensure_background_history_thread();
    let tid = cabin.threads[idx].id.clone();
    assert!(cabin.threads[idx].background);
    cabin.chat_job_thread = Some(tid);
    assert!(cabin.job_on_background_thread());
    assert!(!cabin.running);
}

// Folded from PR #338.
#[test]
fn background_tasks_open_false_when_idle() {
    let cabin = Cabin::quiet_for_test();
    assert!(!cabin.background_tasks_open());
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
    assert!(cabin.grok_tasks.is_empty());
}

// Folded from PR #339.
#[test]
fn stream_here_false_when_job_elsewhere() {
    let mut cabin = Cabin::quiet_for_test();
    cabin.status = "Harbor".into();
    // quiet_for_test: chat_job_thread None → stream_here false
    // (chat_stream_is_visible: None => false).
    assert!(cabin.chat_job_thread.is_none());
    assert!(!cabin.stream_here());
    assert_eq!(cabin.status, "Harbor");
    assert!(!cabin.running);

    // Job bound to another thread while viewing the first → false.
    cabin
        .threads
        .push(crate::threads::ChatThread::new("Here", false));
    cabin
        .threads
        .push(crate::threads::ChatThread::new("Elsewhere", false));
    cabin.thread_idx = 0;
    let other = cabin.threads[1].id.clone();
    cabin.chat_job_thread = Some(other);
    assert!(!cabin.stream_here());
    assert!(!cabin.running);
    assert_eq!(cabin.status, "Harbor");
}

// Folded from PR #340.
#[test]
fn leave_should_halt_false_when_idle() {
    let mut cabin = Cabin::quiet_for_test();
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
    // drop_leaving_thread_chrome gates on `running && leave_should_halt()`.
    assert!(
        !(cabin.running && cabin.leave_should_halt()),
        "leaving while idle must not halt"
    );

    // Cabin-wide unbound work (no chat_job_thread) still stops on leave.
    cabin.running = true;
    assert!(
        cabin.leave_should_halt(),
        "cabin-wide work with no thread still stops"
    );
}

// Folded from PR #341.
#[test]
fn chrome_here_true_on_unbound_idle() {
    let mut cabin = Cabin::quiet_for_test();
    cabin.status = "Harbor".into();
    // quiet_for_test: chat_job_thread None → chrome_here true (unbound cabin-wide work).
    assert!(cabin.chat_job_thread.is_none());
    assert!(cabin.chrome_here());
    assert_eq!(cabin.status, "Harbor");
    assert!(!cabin.running);

    // Bound to a different thread than visible → false; match visible → true.
    cabin
        .threads
        .push(crate::threads::ChatThread::new("Chat", false));
    cabin.thread_idx = 0;
    let vis = cabin.visible_thread_id();
    cabin.chat_job_thread = Some("elsewhere".into());
    assert!(!cabin.chrome_here());
    cabin.chat_job_thread = Some(vis);
    assert!(cabin.chrome_here());
    assert!(!cabin.running);
    assert_eq!(cabin.status, "Harbor");
}

// Folded from PR #343.
#[test]
fn run_phase_here_idle_when_not_thinking() {
    let cabin = Cabin::quiet_for_test();
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
    assert_eq!(
        cabin.run_phase_here(),
        ChatRunPhase::Idle,
        "quiet idle cabin must report Idle"
    );
}

// Folded from PR #344.
#[test]
fn has_real_history_false_when_idle() {
    let mut cabin = Cabin::quiet_for_test();
    cabin.status = "Harbor".into();

    // Empty / blank title, Scratch, and default Chat are not real history — even with messages.
    let mut blank = crate::threads::ChatThread::new("", false);
    blank.messages = Arc::new(vec![("user".into(), "hi".into())]);
    let mut scratch = crate::threads::ChatThread::new("Scratch", true);
    scratch.messages = Arc::new(vec![("user".into(), "hi".into())]);
    let mut chat = crate::threads::ChatThread::new("Chat", false);
    chat.messages = Arc::new(vec![("user".into(), "hi".into())]);
    cabin.threads = vec![blank, scratch, chat];
    cabin.thread_idx = 2;

    assert!(!cabin.has_real_history());
    assert_eq!(cabin.status, "Harbor");
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
}

// Folded from PR #345.
#[test]
fn run_action_here_empty_when_idle() {
    let cabin = Cabin::quiet_for_test();
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
    assert!(cabin.perm_ask.is_none());
    assert!(cabin.elicit_ask.is_none());
    assert!(cabin.tool_cards.is_empty());
    assert!(cabin.live_blocks.is_empty());
    // Quiet idle: no wait title, no tool title → chat_run_action → "".
    assert_eq!(cabin.run_action_here(), "");
}

// Folded from PR #346.
#[test]
fn thinking_here_false_when_idle() {
    let cabin = Cabin::quiet_for_test();
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
    assert!(!cabin.thinking_here());
}

// Folded from PR #349.
#[test]
fn last_night_hint_empty_when_idle() {
    let cabin = Cabin::quiet_for_test();
    assert!(cabin.continue_hint.is_empty());
    assert!(cabin.messages.is_empty());
    assert!(cabin.rewind_rows.is_empty());
    assert!(cabin.chat_job_thread.is_none());
    assert!(!cabin.running);
    assert!(cabin.rx.is_none());
    assert!(cabin.acp.is_none());
    assert!(cabin.live_cap_rx.is_none());

    let hint = cabin.last_night_hint();
    assert!(
        hint.is_empty(),
        "idle cabin with empty continue_hint and no receipts stays on the continue_hint-only path: {hint:?}"
    );
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
    assert!(cabin.rx.is_none());
    assert!(cabin.acp.is_none());
    assert!(cabin.live_cap_rx.is_none());
}

// Folded from PR #351.
#[test]
fn visible_host_receipts_empty_when_idle() {
    let cabin = Cabin::quiet_for_test();
    assert!(cabin.messages.is_empty());
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
    assert!(cabin.visible_host_receipts().is_empty());
}

// Folded from PR #353.
#[test]
fn visible_thread_id_matches_current_when_idle() {
    let mut cabin = Cabin::quiet_for_test();
    cabin
        .threads
        .push(crate::threads::ChatThread::new("Chat", false));
    cabin.thread_idx = 0;
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
    let id = cabin.visible_thread_id();
    assert!(!id.is_empty());
    assert_eq!(id, cabin.threads[cabin.thread_idx].id);
}

// Folded from PR #355.
#[test]
fn scratch_false_on_quiet_chat() {
    let mut cabin = Cabin::quiet_for_test();
    cabin
        .threads
        .push(crate::threads::ChatThread::new("Chat", false));
    cabin.thread_idx = 0;
    assert!(cabin.chat_job_thread.is_none());
    assert!(!cabin.running);
    assert!(!cabin.scratch());
}

// Folded from PR #356.
#[test]
fn touch_clears_reflected_idle() {
    let mut cabin = Cabin::quiet_for_test();
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
    assert!(cabin.reflect_rx.is_none());
    assert!(cabin.rx.is_none());
    cabin.reflected_idle = true;
    cabin.touch();
    assert!(!cabin.reflected_idle);
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
    assert!(cabin.reflect_rx.is_none());
    assert!(cabin.rx.is_none());
}

// Folded from PR #359.
#[test]
fn last_user_on_job_empty_when_idle() {
    let cabin = Cabin::quiet_for_test();
    assert!(cabin.chat_job_thread.is_none());
    assert!(cabin.messages.is_empty());
    // Quiet empty transcript → empty string. No spawn/network.
    assert_eq!(cabin.last_user_on_job(), "");
}

// Folded from PR #365.
#[test]
fn turn_transcript_returns_merged_when_idle() {
    let cabin = Cabin::quiet_for_test();
    assert!(cabin.turn_log.is_empty());
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
    assert!(cabin.rx.is_none());
    let merged = "idle merged reply".to_string();
    // Empty turn_log → turn_needs_timeline false → merged unchanged. No spawn/network.
    assert_eq!(
        cabin.turn_transcript(merged.clone(), IMAGE_FILE_CAP),
        merged
    );
}

// Folded from PR #372.
#[test]
fn tick_mid_thought_sets_continue_hint() {
    let mut cabin = Cabin::quiet_for_test();
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
    cabin.tick_mid_thought();
    assert_eq!(
        cabin.continue_hint,
        crate::threads::continue_thread_hint(&cabin.threads)
    );
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
}

// Folded from PR #374.
#[test]
fn last_project_title_from_prefers_continue_hint() {
    let cabin = Cabin::quiet_for_test();
    assert!(cabin.chat_job_thread.is_none());
    assert!(!cabin.running);
    assert!(cabin.rx.is_none());
    assert!(cabin.acp.is_none());

    assert_eq!(
        Cabin::last_project_title_from("", "", ""),
        "",
        "empty inputs stay empty"
    );
    assert_eq!(
        Cabin::last_project_title_from("Night Board", "Auth Gate", "Short Pin"),
        "Auth Gate",
        "continue_hint wins over last_night and short goal_pin"
    );
    assert_eq!(
        Cabin::last_project_title_from("Night Board", "", "Short Pin"),
        "Night Board",
        "last_night wins when continue_hint is empty"
    );
    assert_eq!(
        Cabin::last_project_title_from("", "", "Short Pin"),
        "Short Pin",
        "short goal_pin is the last fallback"
    );

    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
    assert!(cabin.rx.is_none());
    assert!(cabin.acp.is_none());
}

// Folded from PR #380.
#[test]
fn absorb_turn_learning_noop_when_idle() {
    let mut cabin = Cabin::quiet_for_test();
    assert!(cabin.messages.is_empty());
    assert!(cabin.chat_job_thread.is_none());
    assert!(!cabin.running);
    assert!(cabin.rx.is_none());
    let before = cabin.learning.clone();
    cabin.absorb_turn_learning("Ready.");
    assert_eq!(cabin.learning, before);
    assert!(cabin.messages.is_empty());
    assert!(cabin.chat_job_thread.is_none());
    assert!(!cabin.running);
    assert!(cabin.rx.is_none());
    assert!(cabin.persist_rx.is_none());
}

// Folded from PR #381.
#[test]
fn apply_assistant_snapshot_empty_and_sets() {
    let mut cabin = Cabin::quiet_for_test();
    assert!(cabin.chat_job_thread.is_none());
    assert!(!cabin.running);
    assert!(cabin.rx.is_none());
    assert!(cabin.messages.is_empty());

    // Empty content is a no-op.
    cabin.apply_assistant_snapshot(String::new());
    assert!(
        cabin.messages.is_empty(),
        "empty snapshot must not touch the visible thread: {:?}",
        cabin.messages
    );
    assert!(cabin.chat_job_thread.is_none());
    assert!(!cabin.running);

    // Non-empty lands as assistant on the visible thread.
    cabin.apply_assistant_snapshot("first reply".into());
    assert_eq!(
        cabin.messages.as_slice(),
        [("assistant".into(), "first reply".into())]
    );

    // Another non-empty updates that assistant in place.
    cabin.apply_assistant_snapshot("updated reply".into());
    assert_eq!(
        cabin.messages.as_slice(),
        [("assistant".into(), "updated reply".into())]
    );
    assert!(cabin.chat_job_thread.is_none());
    assert!(!cabin.running);
    assert!(cabin.rx.is_none());
}

// Folded from PR #382.
#[test]
fn trim_job_result_dumps_noop_when_idle() {
    let mut cabin = Cabin::quiet_for_test();
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
    assert!(cabin.messages.is_empty());
    assert!(cabin.rx.is_none());
    cabin.trim_job_result_dumps();
    assert!(cabin.messages.is_empty(), "empty transcript stays empty");
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
    assert!(cabin.rx.is_none());
}

// Folded from PR #383.
#[test]
fn push_bound_msg_appends_on_visible() {
    let mut cabin = Cabin::quiet_for_test();
    assert!(cabin.chat_job_thread.is_none());
    assert!(cabin.messages.is_empty());
    assert!(!cabin.running);
    // Quiet cabin (no chat_job_thread) → lines land on the visible transcript. No spawn/network.
    cabin.push_bound_msg("user", "harbor bound user".into());
    cabin.push_bound_msg("assistant", "harbor bound reply".into());
    assert_eq!(
        cabin.messages.as_ref(),
        &[
            ("user".into(), "harbor bound user".into()),
            ("assistant".into(), "harbor bound reply".into()),
        ]
    );
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
    assert!(cabin.rx.is_none());
}

// Folded from PR #384.
#[test]
fn apply_live_assistant_empty_when_idle() {
    let mut cabin = Cabin::quiet_for_test();
    assert!(cabin.thought_buf.is_empty());
    assert!(cabin.stream_buf.is_empty());
    assert!(cabin.turn_log.is_empty());
    assert!(cabin.messages.is_empty());
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
    assert!(cabin.rx.is_none());
    // Empty buffers → empty snapshot → apply_assistant_snapshot early-outs. No spawn/network.
    cabin.apply_live_assistant();
    assert!(
        cabin.messages.is_empty(),
        "idle empty snapshot must not add an assistant line"
    );
    assert!(cabin.chat_job_thread.is_none());
    assert!(!cabin.running);
    assert!(cabin.rx.is_none());
}

// Folded from PR #388.
#[test]
fn paint_text_delta_skips_when_stream_elsewhere() {
    let mut cabin = Cabin::quiet_for_test();
    // quiet_for_test: chat_job_thread None → stream_here false → paint early-returns.
    assert!(cabin.chat_job_thread.is_none());
    assert!(cabin.live_blocks.is_empty());
    cabin.paint_text_delta(LiveKind::Say, "hello elsewhere");
    assert!(
        cabin.live_blocks.is_empty(),
        "unbound stream must not paint say: {:?}",
        cabin.live_blocks
    );
    cabin.paint_text_delta(LiveKind::Thought, "thinking elsewhere");
    assert!(
        cabin.live_blocks.is_empty(),
        "unbound stream must not paint thought: {:?}",
        cabin.live_blocks
    );
    assert!(!cabin.running);

    // Job on another thread → same early return; live_blocks stay empty.
    cabin
        .threads
        .push(crate::threads::ChatThread::new("Chat", false));
    cabin.thread_idx = 0;
    cabin.chat_job_thread = Some("elsewhere".into());
    cabin.paint_text_delta(LiveKind::Say, "parked say");
    cabin.paint_text_delta(LiveKind::Thought, "parked thought");
    assert!(
        cabin.live_blocks.is_empty(),
        "stream on another thread must not paint: {:?}",
        cabin.live_blocks
    );
    assert!(!cabin.running);
}

// Folded from PR #389.
#[test]
fn ingest_tool_card_skips_live_when_stream_elsewhere() {
    let mut cabin = Cabin::quiet_for_test();
    cabin
        .threads
        .push(crate::threads::ChatThread::new("Here", false));
    cabin
        .threads
        .push(crate::threads::ChatThread::new("Elsewhere", false));
    cabin.thread_idx = 0;
    let other = cabin.threads[1].id.clone();
    cabin.chat_job_thread = Some(other);
    assert!(!cabin.stream_here());
    assert!(!cabin.running);
    assert!(cabin.live_blocks.is_empty());

    let card = grokhub_core::wire::ToolCard {
        id: "t1".into(),
        title: "grep".into(),
        kind: String::new(),
        status: "completed".into(),
        detail: String::new(),
        diff: String::new(),
        image_data_url: None,
        raw_input: String::new(),
    };
    cabin.ingest_tool_card(&card);
    assert!(cabin.live_blocks.is_empty());
    assert!(cabin.turn_log.is_empty());
    assert!(!cabin.running);
    assert!(cabin.rx.is_none());
}

// Folded from PR #391.
#[test]
fn apply_job_fail_returns_error_when_idle() {
    let mut cabin = Cabin::quiet_for_test();
    assert!(cabin.chat_job_thread.is_none());
    assert!(!cabin.running);
    assert!(cabin.rx.is_none());
    assert!(!cabin.try_again);
    // No chat_job_thread → job_error_goes_to_chat is false; non-sigterm / non-credit
    // err returns the string without chat paint or grok spawn.
    let err = "host failed";
    assert_eq!(cabin.apply_job_fail(err), err);
    assert!(cabin.chat_job_thread.is_none());
    assert!(cabin.messages.is_empty());
    assert!(!cabin.running);
    assert!(cabin.rx.is_none());
    assert!(!cabin.try_again);
    assert!(cabin.persist_rx.is_none());
}

// Folded from PR #398.
#[test]
fn withdraw_perm_asks_noop_when_idle() {
    let mut cabin = Cabin::quiet_for_test();
    assert!(cabin.perm_ask.is_none());
    assert!(cabin.perm_queue.is_empty());
    assert!(cabin.acp.is_none());
    assert!(cabin.chat_job_thread.is_none());
    assert!(!cabin.running);
    cabin.withdraw_perm_asks();
    assert!(cabin.perm_ask.is_none());
    assert!(cabin.perm_queue.is_empty());
    assert!(cabin.acp.is_none());
    assert!(cabin.chat_job_thread.is_none());
    assert!(!cabin.running);
    assert!(cabin.rx.is_none());
    assert!(cabin.persist_rx.is_none());
}

// Folded from PR #399.
#[test]
fn job_is_idea_talk_false_when_idle() {
    let cabin = Cabin::quiet_for_test();
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
    assert!(!cabin.job_is_idea_talk());
}

// Folded from PR #400.
#[test]
fn settle_scheduled_run_noop_without_an_automation() {
    let mut cabin = Cabin::quiet_for_test();
    cabin.status = "Harbor".into();
    assert!(cabin.chat_job_thread.is_none());
    assert!(!cabin.running);
    assert!(cabin.rx.is_none());
    let cards = cabin.updates.len();
    // No automation id → early return; status/Harbor unchanged, no feed card.
    cabin.settle_scheduled_run(None, &grokhub_core::BgEnd::Failed("dummy".into()), "");
    assert_eq!(cabin.updates.len(), cards);
    assert_eq!(cabin.status, "Harbor");
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
    assert!(cabin.rx.is_none());
}

// Folded from PR #404.
#[test]
fn hold_chat_name_for_plan_freezes_visible_history_label() {
    let mut cabin = Cabin::quiet_for_test();
    assert!(cabin.chat_job_thread.is_none());
    assert!(!cabin.running);
    assert!(cabin.rx.is_none());
    assert!(cabin.acp.is_none());
    assert!(!cabin.host_diff_kick);

    let sid = "sess-plan-hold";
    let mut thread = crate::threads::ChatThread::new("Night watch", false);
    thread.title_locked = true;
    thread.grok_session = Some(sid.into());
    cabin.threads = vec![thread];
    cabin.thread_idx = 0;
    cabin
        .grok_sessions
        .push(grokhub_core::cli_history::GrokSession {
            id: sid.into(),
            title: "fix the dock".into(),
            path: None,
            cwd: None,
            cabin: false,
        });

    cabin.hold_chat_name_for_plan();
    assert_eq!(cabin.threads[0].title, "Night watch");
    assert_eq!(
        cabin
            .grok_sessions
            .iter()
            .find(|s| s.id == sid)
            .map(|s| s.title.as_str()),
        Some("Night watch"),
        "Plan freezes the History label to the visible rail title"
    );

    // Already held: second call is a noop.
    cabin.hold_chat_name_for_plan();
    assert_eq!(
        cabin
            .grok_sessions
            .iter()
            .find(|s| s.id == sid)
            .map(|s| s.title.as_str()),
        Some("Night watch")
    );
    assert_eq!(cabin.threads[0].title, "Night watch");
    assert!(cabin.chat_job_thread.is_none());
    assert!(!cabin.running);
    assert!(cabin.rx.is_none());
    assert!(cabin.acp.is_none());
    assert!(!cabin.host_diff_kick);
}

// Folded from PR #405.
#[test]
fn apply_thread_goal_empty_topics_stays_idle() {
    let mut cabin = Cabin::quiet_for_test();
    cabin
        .threads
        .push(crate::threads::ChatThread::new("Chat", false));
    cabin.thread_idx = 0;
    let tid = cabin.threads[0].id.clone();
    assert!(!tid.is_empty());
    assert!(!cabin.host_diff_kick);
    assert!(!cabin.goal_busy);
    assert!(!cabin.goal_stale);
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
    assert!(cabin.goal_rx.is_none());
    assert!(cabin.rx.is_none());

    // Empty reply → empty topics → early return; no spawn/network/persist path.
    cabin.apply_thread_goal(&tid, "");
    assert!(
        cabin.threads[0].goal.label.is_empty(),
        "empty topics must leave the goal label alone"
    );
    assert!(
        cabin.threads[0].goal.topics.is_empty(),
        "empty topics must leave goal topics alone"
    );
    assert!(!cabin.host_diff_kick);
    assert!(!cabin.goal_busy);
    assert!(!cabin.goal_stale);
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
    assert!(cabin.goal_rx.is_none());
    assert!(cabin.rx.is_none());
}

#[test]
fn spawn_thread_goal_titles_from_user_words_without_a_model() {
    let (root, mut cabin) = isolated_cabin("local-thread-title");
    cabin
        .threads
        .push(crate::threads::ChatThread::new("Chat", false));
    cabin.thread_idx = cabin.threads.len() - 1;
    let tid = cabin.threads[cabin.thread_idx].id.clone();
    cabin.messages = vec![("user".into(), "draw porn comics please".into())].into();
    cabin.spawn_thread_goal_on(Some(&tid));
    assert!(
        cabin.goal_rx.is_none() && !cabin.goal_busy,
        "titles must not open a model channel"
    );
    assert_eq!(cabin.threads[cabin.thread_idx].goal.label, "draw");
    assert!(
        cabin.threads[cabin.thread_idx]
            .goal
            .topics
            .iter()
            .any(|t| t == "porn"),
        "content words from the user turn become topics: {:?}",
        cabin.threads[cabin.thread_idx].goal.topics
    );
    assert_ne!(
        cabin.threads[cabin.thread_idx].title, "Chat",
        "auto-title must rename the rail from local topics"
    );
    release_isolated(&root, cabin);
}

// Folded from PR #425.
#[test]
fn chip_chat_pairs_empty_when_idle() {
    let cabin = Cabin::quiet_for_test();
    assert!(cabin.messages.is_empty());
    assert!(!cabin.running);
    assert!(cabin.chat_job_thread.is_none());
    // Quiet cabin / empty messages → empty chip pairs. No spawn/network.
    let pairs = cabin.chip_chat_pairs();
    assert!(pairs.is_empty());
    assert!(cabin.chat_job_thread.is_none());
    assert!(!cabin.running);
}
