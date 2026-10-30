//! Background runs, steering and scheduled run tests (rearch stage 5).
//!
//! Mechanical move from `tests.rs`. Wired via `#[path]` so
//! `app/mod.rs` stays untouched.

use super::*;
use eframe::egui;

// Background runs and steering.

#[cfg(unix)]
#[test]
fn a_message_typed_during_a_reply_steers_it() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = bg_cabin("steer-live");
    let _tx = live_reply(&mut cabin, "slow-turn please", "Working on it.");
    assert!(cabin.can_steer_live_turn());

    cabin.send_chat("actually make it blue".into());
    assert!(
        cabin.followup_queue.is_empty(),
        "Enter steers, it does not queue"
    );
    assert!(
        cabin.running,
        "the steered turn keeps running: {}",
        cabin.status
    );
    assert_eq!(cabin.status, "Steering…");
    let roles: Vec<(&str, &str)> = cabin
        .messages
        .iter()
        .map(|m| (m.0.as_str(), m.1.as_str()))
        .collect();
    assert_eq!(
        roles,
        vec![
            ("user", "slow-turn please"),
            ("assistant", "Working on it."),
            ("user", "actually make it blue"),
        ],
        "what it said stays and your message shows clean"
    );
    end_bg_test(root, cabin);
}

#[cfg(unix)]
#[test]
fn alt_enter_and_slash_queue_wait_for_the_reply() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = bg_cabin("steer-queue");
    let _tx = live_reply(&mut cabin, "slow-turn please", "Working on it.");
    cabin.bg.queue_next = true;
    cabin.send_chat("after that, deploy".into());
    cabin.send_chat("/queue and then tidy".into());
    assert_eq!(
        cabin.followup_queue,
        vec![
            "after that, deploy".to_string(),
            "and then tidy".to_string()
        ]
    );
    assert!(
        cabin.running && cabin.grok_p_rx.is_some(),
        "queuing leaves the turn alone"
    );
    assert_eq!(
        cabin.messages.len(),
        2,
        "queued messages wait off the transcript"
    );
    assert!(!cabin.bg.queue_next, "Alt+Enter is one send");
    end_bg_test(root, cabin);
}

#[cfg(unix)]
#[test]
fn slash_bg_runs_beside_the_chat_on_a_forked_session() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = bg_cabin("bg-slash");
    cabin.threads[0].grok_session = Some("native-main".into());

    cabin.send_chat("/bg run the checks".into());
    assert!(!cabin.running, "the composer stays free");
    assert_eq!(cabin.bg.runs.len(), 1, "{}", cabin.status);
    assert_eq!(cabin.status, "Background · run the checks");
    assert_eq!(cabin.bg.runs[0].resumed.as_deref(), Some("native-main"));
    let child = cabin.bg.runs[0]
        .native_session
        .clone()
        .expect("native child session");
    assert_ne!(
        child, "native-main",
        "a background run never writes the chat's session"
    );
    assert!(
        poll_until(&mut cabin, 8, |c| c.bg.runs.is_empty()),
        "{}",
        cabin.status
    );
    assert_eq!(
        cabin.messages.last().map(|m| m.1.as_str()),
        Some("**Background task done** · run the checks\n\nbg finished")
    );
    assert_eq!(
        cabin.threads[0].grok_session.as_deref(),
        Some("native-main")
    );

    cabin.send_chat("/bg".into());
    assert_eq!(cabin.status, "No background tasks — /bg <task> starts one");
    cabin.send_chat("/bg stop".into());
    assert_eq!(cabin.status, "No background tasks running");
    end_bg_test(root, cabin);
}

#[cfg(unix)]
#[test]
fn grok_can_start_background_tasks_from_its_reply() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = bg_cabin("bg-agent");
    let id = cabin.threads[0].id.clone();
    cabin.start_agent_bg_tasks("On it.\nBACKGROUND_TASK: index the docs", &id);
    assert_eq!(cabin.bg.runs.len(), 1, "{}", cabin.status);
    assert_eq!(cabin.bg.runs[0].origin, grokhub_core::BgOrigin::Agent);
    assert_eq!(cabin.bg.runs[0].title, "index the docs");
    assert!(
        poll_until(&mut cabin, 8, |c| c.bg.runs.is_empty()),
        "{}",
        cabin.status
    );
    assert!(
        cabin
            .messages
            .last()
            .is_some_and(|m| m.1.starts_with("**Background task done** · index the docs")),
        "{:?}",
        cabin.messages
    );
    end_bg_test(root, cabin);
}

#[test]
fn a_background_result_waits_while_its_chat_is_mid_turn() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("bg-hold");
    // Focused on this chat: the post needs no desktop notification.
    cabin.window_focused = true;
    cabin.threads = vec![crate::threads::ChatThread::new("Chat", false)];
    cabin.thread_idx = 0;
    cabin.messages = std::sync::Arc::new(vec![("user".into(), "hi".into())]);
    let id = cabin.threads[0].id.clone();
    cabin.bg.runs.push(super::background::BgRun {
        id: 1,
        thread_id: id.clone(),
        title: "checks".into(),
        origin: grokhub_core::BgOrigin::User,
        rx: None,
        say: "all green".into(),
        action: String::new(),
        started: std::time::Instant::now(),
        end: Some(grokhub_core::BgEnd::Done),
        session: String::new(),
        resumed: None,
        fork_hold: false,
        native_session: None,
        automation: None,
    });
    cabin.running = true;
    cabin.chat_job_thread = Some(id.clone());
    cabin.post_finished_bg_runs();
    assert_eq!(
        cabin.bg.runs.len(),
        1,
        "a live reply would overwrite the post"
    );
    assert_eq!(cabin.messages.len(), 1);
    cabin.running = false;
    cabin.chat_job_thread = None;
    cabin.post_finished_bg_runs();
    assert!(cabin.bg.runs.is_empty());
    assert_eq!(
        cabin.messages.last().map(|m| m.1.as_str()),
        Some("**Background task done** · checks\n\nall green")
    );
    let block = cabin
        .take_bg_results_follow(&id)
        .expect("next turn hears it");
    assert!(block.contains("- checks (done): all green"), "{block}");
    assert!(cabin.take_bg_results_follow(&id).is_none(), "told once");
    release_isolated(&root, cabin);
}

#[test]
fn a_scheduled_run_ends_on_follow_up_and_never_touches_your_chat() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("bg-scheduled");
    cabin.board.clear();
    cabin.automations = vec![test_automation(
        "a-snap",
        "Host snapshot",
        "every day at 9, run a read-only host snapshot and report what changed",
    )];
    cabin.threads = vec![crate::threads::ChatThread::new("Chat", false)];
    cabin.thread_idx = 0;
    cabin.messages = std::sync::Arc::new(vec![("user".into(), "hi".into())]);
    cabin.nav = Nav::Workboard;
    let hidden = cabin.ensure_background_history_thread();
    let bg_id = cabin.threads[hidden].id.clone();
    let fork = "01a01b0f-7e06-74b1-8f22-5236c9d57d60".to_string();
    cabin.bg.runs.push(super::background::BgRun {
        id: 1,
        thread_id: bg_id.clone(),
        title: "Host snapshot".into(),
        origin: grokhub_core::BgOrigin::Scheduled,
        rx: None,
        say: "## Host snapshot\nDisk at 91% on /home.".into(),
        action: String::new(),
        started: std::time::Instant::now(),
        end: None,
        session: fork.clone(),
        resumed: None,
        fork_hold: false,
        native_session: None,
        automation: Some("a-snap".into()),
    });
    assert!(cabin.bg.scheduled_live());
    assert_eq!(
        cabin.bg.live_count(),
        0,
        "a scheduled run takes none of your /bg slots"
    );
    // You keep chatting while it works: your turn does not hold its post.
    cabin.running = true;
    cabin.chat_job_thread = Some(cabin.threads[0].id.clone());
    cabin.bg.runs[0].end = Some(grokhub_core::BgEnd::Done);
    cabin.post_finished_bg_runs();
    assert!(cabin.bg.runs.is_empty());
    assert_eq!(cabin.messages.len(), 1, "nothing lands in your chat");
    assert!(cabin.nav == Nav::Workboard, "the page you are on stays");
    let card = cabin
        .board
        .iter()
        .find(|c| c.automation.as_deref() == Some("a-snap"))
        .expect("follow up card")
        .clone();
    assert_eq!(card.status, grokhub_core::BoardStatus::FollowUp);
    let chat = card.thread_id.expect("its chat");
    let thread = cabin.threads.iter().find(|t| t.id == chat).expect("thread");
    assert!(thread.background && thread.title == "Follow up · Host snapshot");
    assert!(thread
        .messages
        .last()
        .unwrap()
        .1
        .contains("Disk at 91% on /home."));
    assert_eq!(
        crate::threads::chat_section_indices(&cabin.threads, Some(0), false),
        vec![0],
        "History lists only your chat"
    );
    let bg = cabin.threads.iter().find(|t| t.id == bg_id).unwrap();
    assert!(
        bg.retired_sessions.contains(&fork),
        "its session is filed off History"
    );
    assert_eq!(
        cabin.automations[0].health.outcome,
        grokhub_core::AutoOutcome::Ok
    );
    cabin.running = false;
    cabin.chat_job_thread = None;
    release_isolated(&root, cabin);
}

/// 2.10.89 review of #485: Halt still reaches a scheduled run in its own process.
#[test]
fn halt_stops_a_scheduled_run_and_your_stop_does_not() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("bg-scheduled-halt");
    std::fs::create_dir_all(&root).expect("config root");
    cabin.board.clear();
    cabin.automations = vec![test_automation(
        "a-snap",
        "Host snapshot",
        "every day at 9, run a read-only host snapshot and report what changed",
    )];
    let hidden = cabin.ensure_background_history_thread();
    let bg_id = cabin.threads[hidden].id.clone();
    let (_tx, rx) = std::sync::mpsc::channel();
    cabin.bg.runs.push(super::background::BgRun {
        id: 1,
        thread_id: bg_id,
        title: "Host snapshot".into(),
        origin: grokhub_core::BgOrigin::Scheduled,
        rx: Some(rx),
        say: "## Host snapshot\nHalf way.".into(),
        action: String::new(),
        started: std::time::Instant::now(),
        end: None,
        session: String::new(),
        resumed: None,
        fork_hold: false,
        native_session: None,
        automation: Some("a-snap".into()),
    });
    // Stop on your own turn (composer, /stop, the board) is for your chat only.
    cabin.halt_work("Stopped");
    assert!(
        cabin.bg.scheduled_live(),
        "your Stop leaves the scheduled run alone"
    );
    // Halt (hotkey, tray, Ctrl+Alt+H) stops everything, the scheduled run too.
    cabin.halt_everything("Stopped");
    assert!(!cabin.bg.scheduled_live());
    assert!(cabin.bg.runs.is_empty());
    assert_eq!(
        cabin.automations[0].health.outcome,
        grokhub_core::AutoOutcome::Stopped
    );
    assert_eq!(cabin.status, "Stopped");
    assert!(
        !cabin
            .board
            .iter()
            .any(|c| c.automation.as_deref() == Some("a-snap")),
        "a stopped run files no Follow up card"
    );
    release_isolated(&root, cabin);
}

/// 2.10.89 review of #485: scheduled runs stay at low effort, like every unattended run.
#[test]
fn a_scheduled_run_uses_low_effort_and_your_bg_starts_at_the_chat_start() {
    let cabin = Cabin::quiet_for_test();
    assert_eq!(
        cabin
            .bg_effort(grokhub_core::BgOrigin::Scheduled)
            .as_deref(),
        Some("low")
    );
    // R1: your own /bg work starts at everyday chat's start; the router moves it per step.
    assert_eq!(
        cabin.bg_effort(grokhub_core::BgOrigin::Agent).as_deref(),
        Some("medium")
    );
    assert_eq!(
        cabin.bg_effort(grokhub_core::BgOrigin::Detached).as_deref(),
        Some("medium")
    );
    let src = include_str!("background.rs");
    let start = fn_src(src, "start_bg_task");
    assert!(start.contains("start_native_bg"), "{start}");
    let native = fn_src(src, "start_native_bg");
    assert!(native.contains("self.bg_effort(origin)"), "{native}");
}

/// 2.10.89 review of #485: an automation that names a skill still gets its steps.
#[test]
fn a_scheduled_run_keeps_a_matching_skills_steps() {
    let mut cabin = Cabin::quiet_for_test();
    cabin.skill_list = vec![SkillMd {
        name: "host-snapshot".into(),
        description: "Read-only host report".into(),
        slash: "/snapshot".into(),
        trigger: "host snapshot".into(),
        instructions: "1. `df -h`".into(),
        pitfalls: "Never write.".into(),
        verify: "Report disk use.".into(),
        runs: 0,
    }];
    assert_eq!(
        cabin.scheduled_task_text("/snapshot then report what changed"),
        "Active skill host-snapshot — follow these steps:\n## Steps\n1. `df -h`\n\n## Pitfalls\nNever write.\n\n## Verify\nReport disk use.\n\n/snapshot then report what changed"
    );
    assert_eq!(
        cabin.scheduled_task_text("summarize my open GitHub issues"),
        "summarize my open GitHub issues"
    );
    let src = include_str!("night.rs");
    let start = fn_src(src, "start_scheduled_run");
    assert!(
        start.contains("scheduled_task_text(&a.instructions)"),
        "{start}"
    );
    assert!(start.contains("bg_task_title(&a.instructions)"), "{start}");
}

/// 2.10.89 review of #485: deleting a Follow up card puts its hidden chat back in
/// History, as the status line promises, so a report or your reply is never lost.
#[test]
fn deleting_a_follow_up_card_brings_its_chat_back_to_history() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("follow-up-delete");
    std::fs::create_dir_all(&root).expect("config root");
    cabin.board.clear();
    cabin.automations = vec![test_automation(
        "a-issues",
        "Morning issues",
        "every day at 8, summarize my open GitHub issues and report",
    )];
    cabin.threads = vec![crate::threads::ChatThread::new("Chat", false)];
    cabin.thread_idx = 0;
    cabin.settle_scheduled_run(
        Some("a-issues"),
        &grokhub_core::BgEnd::Done,
        "## Open issues\nThree opened overnight: crash on resume, tray icon, docs.",
    );
    let card = cabin
        .board
        .iter()
        .find(|c| c.automation.as_deref() == Some("a-issues"))
        .expect("follow up card")
        .clone();
    let chat = card.thread_id.clone().expect("its chat");
    let at = cabin
        .threads
        .iter()
        .position(|t| t.id == chat)
        .expect("thread");
    assert!(cabin.threads[at].background);
    assert_eq!(
        crate::threads::chat_section_indices(&cabin.threads, Some(0), false),
        vec![0]
    );
    assert!(cabin.apply_board_act(Some(super::pages::BoardAct::Delete(card.id.clone()))));
    assert_eq!(cabin.status, "Card deleted. Its chat stays in History.");
    assert!(!cabin.threads[at].background);
    assert_eq!(cabin.threads[at].title, "Follow up · Morning issues");
    assert_eq!(
        crate::threads::chat_section_indices(&cabin.threads, Some(0), false),
        vec![0, at]
    );
    // The hidden Background chat stays hidden: only Follow up chats come back.
    let bg = cabin.ensure_background_history_thread();
    assert!(!cabin.unpark_follow_up_chat(Some(cabin.threads[bg].id.clone().as_str())));
    assert!(cabin.threads[bg].background);
    release_isolated(&root, cabin);
}

#[test]
fn a_cabin_wide_grok_command_still_queues_instead_of_steering() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("steer-cabin-wide");
    let (_tx, rx) = mpsc::channel();
    cabin.grok_p_rx = Some(rx);
    cabin.running = true;
    cabin.chat_job_thread = None;
    assert!(
        !cabin.can_steer_live_turn(),
        "/compact has no chat turn to steer"
    );
    cabin.send_chat("then this".into());
    assert_eq!(cabin.followup_queue, vec!["then this".to_string()]);
    assert!(cabin.running);
    cabin.grok_p_rx = None;
    cabin.running = false;
    release_isolated(&root, cabin);
}

#[test]
fn the_live_work_strip_offers_steer_queue_and_background_stop() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("bg-strip");
    cabin.threads = vec![crate::threads::ChatThread::new("Chat", false)];
    cabin.thread_idx = 0;
    cabin.messages = std::sync::Arc::new(vec![("user".into(), "hi".into())]);
    let id = cabin.threads[0].id.clone();
    let paint = |cabin: &mut super::Cabin| {
        let mut texts = Vec::new();
        let ctx = egui::Context::default();
        let _ = crate::theme::test_pass(&ctx, Default::default(), |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                cabin.paint_live_work(ui);
                let layer = ui.layer_id();
                ui.ctx().graphics(|layers| {
                    if let Some(list) = layers.get(layer) {
                        for clipped in list.all_entries() {
                            collect_shape_text(&clipped.shape, &mut texts);
                        }
                    }
                });
            });
        });
        texts.join("|")
    };
    assert_eq!(paint(&mut cabin), "", "nothing to act on paints nothing");

    let (_tx, rx) = mpsc::channel();
    cabin.grok_p_rx = Some(rx);
    cabin.running = true;
    cabin.chat_job_thread = Some(id.clone());
    cabin.composer = "make it blue".into();
    cabin.followup_queue = vec!["then deploy".into()];
    cabin.bg.runs.push(super::background::BgRun {
        id: 7,
        thread_id: id.clone(),
        title: "run the checks".into(),
        origin: grokhub_core::BgOrigin::User,
        rx: None,
        say: String::new(),
        action: "cargo test".into(),
        started: std::time::Instant::now(),
        end: None,
        session: String::new(),
        resumed: None,
        fork_hold: false,
        native_session: None,
        automation: None,
    });
    let shown = paint(&mut cabin);
    for want in [
        "Steer",
        "Queue",
        "Queued",
        "then deploy",
        "Steer now",
        "Remove",
        "Background · run the checks",
        "0s · cargo test",
        "Stop",
    ] {
        assert!(shown.contains(want), "missing {want:?}: {shown}");
    }
    cabin.grok_p_rx = None;
    cabin.running = false;
    cabin.bg.runs.clear();
    release_isolated(&root, cabin);
}

#[cfg(unix)]
#[test]
fn bg_under_ask_starts_and_never_moves_the_live_reply() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = bg_cabin("bg-ask-refuse");
    cabin.permission_mode = PermissionMode::Ask;
    // Under Ask a native run starts; its gate denies writes (see
    // `native_bg_ask_refuses_a_write_without_prompting`).
    cabin.send_chat("/bg run the checks".into());
    assert_eq!(cabin.status, "Background · run the checks");
    assert_eq!(cabin.bg.runs.len(), 1);
    assert!(
        poll_until(&mut cabin, 8, |c| c.bg.runs.is_empty()),
        "{}",
        cabin.status
    );

    let _tx = live_reply(&mut cabin, "slow-turn please", "Working on it.");
    cabin.send_chat("/bg".into());
    assert!(
        cabin.running && cabin.grok_p_rx.is_some(),
        "/bg must not move the live reply"
    );
    assert!(cabin.bg.runs.is_empty(), "{}", cabin.status);
    assert_eq!(
        cabin.status,
        "This reply can't move to the background — /stop it or let it finish"
    );
    cabin.send_chat("/bg stop".into());
    assert_eq!(cabin.status, "No background tasks running");

    cabin.halt_in_flight();
    cabin.send_chat("/bg".into());
    assert_eq!(cabin.status, "No background tasks — /bg <task> starts one");
    end_bg_test(root, cabin);
}

/// Native `/bg` under Ask runs the engine and denies a write. It never opens a card.
#[cfg(unix)]
#[test]
fn native_bg_ask_refuses_a_write_without_prompting() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("native-bg-ask");
    let _ = std::fs::create_dir_all(&root);
    cabin.permission_mode = PermissionMode::Ask;
    cabin.session_mode = SessionMode::Chat;
    cabin.threads = vec![crate::threads::ChatThread::new("Chat", false)];
    cabin.thread_idx = 0;
    cabin.messages = cabin.threads[0].messages.clone();
    cabin.threads[0].native = true;
    cabin.threads[0].grok_cwd = Some(root.display().to_string());
    cabin.send_chat("/bg write a file".into());
    assert_ne!(
        cabin.status,
        super::background::BG_ASK_OFF,
        "{}",
        cabin.status
    );
    let child = cabin
        .bg
        .runs
        .first()
        .and_then(|run| run.native_session.clone())
        .unwrap_or_else(|| panic!("{}", cabin.status));
    assert!(cabin.perm_ask.is_none());
    let child_for_poll = child.clone();
    assert!(
        poll_until(&mut cabin, 8, move |_| {
            grokhub_agent::session_file(&child_for_poll)
                .ok()
                .and_then(|path| std::fs::read_to_string(path).ok())
                .is_some_and(|body| body.contains("deny rule on edit"))
        }),
        "native /bg should record the Ask denial"
    );
    assert!(cabin.perm_ask.is_none());
    assert!(!root.join("written-by-bg.txt").exists());
    cabin.stop_all_bg_runs();
    release_isolated(&root, cabin);
}

#[cfg(unix)]
#[test]
fn bg_ask_does_not_start_agent_background_tasks() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = bg_cabin("bg-ask-agent");
    cabin.permission_mode = PermissionMode::Ask;
    let id = cabin.threads[0].id.clone();
    cabin.start_agent_bg_tasks("On it.\nBACKGROUND_TASK: index the docs", &id);
    assert!(cabin.bg.runs.is_empty(), "{}", cabin.status);
    assert_eq!(cabin.status, super::background::BG_ASK_OFF);
    assert!(
        cabin.bg.unread.iter().any(|(tid, note)| {
            tid == &id && note.contains("index the docs") && note.contains("not started")
        }),
        "{:?}",
        cabin.bg.unread
    );
    end_bg_test(root, cabin);
}

#[test]
fn bg_gate_follows_the_pill_the_mode_and_the_desktop_switch() {
    let mut cabin = Cabin::quiet_for_test();
    cabin.session_mode = SessionMode::Chat;
    cabin.permission_mode = PermissionMode::Ask;
    cabin.cfg.desktop_control = false;
    let gate = cabin.native_bg_gate();
    assert_eq!(gate.mode, grokhub_agent::PermMode::Ask);
    assert!(!gate.attended, "a background run has nobody to ask");
    assert!(!gate.readonly_session);
    assert!(!gate.desktop);
    cabin.permission_mode = PermissionMode::Auto;
    cabin.cfg.desktop_control = true;
    let gate = cabin.native_bg_gate();
    assert_eq!(gate.mode, grokhub_agent::PermMode::Auto);
    assert!(gate.desktop);
    cabin.session_mode = SessionMode::Plan;
    cabin.permission_mode = PermissionMode::AlwaysApprove;
    let gate = cabin.native_bg_gate();
    assert_eq!(gate.mode, grokhub_agent::PermMode::Always);
    assert!(
        gate.readonly_session,
        "Plan stays read-only in the background"
    );
}

#[cfg(unix)]
#[test]
fn bg_auto_status_names_the_task() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = bg_cabin("bg-auto-status");
    cabin.send_chat("/bg run the checks".into());
    assert_eq!(cabin.status, "Background · run the checks");
    assert!(!cabin.status.contains("denied"), "{}", cabin.status);
    end_bg_test(root, cabin);
}

#[test]
fn bg_copy_no_longer_says_approval_is_denied() {
    let src = include_str!("background.rs");
    assert!(
        !src.contains("tools that need approval are denied"),
        "{src}"
    );
    let night = include_str!("night.rs");
    let fire = night
        .split("fn fire_loop(")
        .nth(1)
        .and_then(|s| s.split("\n    fn ").next())
        .expect("fire_loop");
    assert!(
        fire.contains("spawn_native_loop"),
        "loops run on the native engine: {fire}"
    );
}

#[cfg(unix)]
#[test]
fn bg_delete_chat_stops_its_runs() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = bg_cabin("bg-delete");
    cabin
        .threads
        .push(crate::threads::ChatThread::new("Other", false));
    let other = cabin.threads[1].id.clone();
    let started = cabin.start_bg_task("slow-bg", &other, grokhub_core::BgOrigin::User);
    assert!(started.is_ok(), "{started:?}");
    assert_eq!(cabin.bg.runs.len(), 1);
    cabin
        .bg
        .unread
        .push((other.clone(), "- slow-bg (done): still going".into()));
    cabin.delete_thread_at(1);
    assert!(
        cabin.bg.runs.is_empty(),
        "runs left: {} ({})",
        cabin.bg.runs.len(),
        cabin.status
    );
    assert!(
        cabin.bg.unread.iter().all(|(id, _)| id != &other),
        "{:?}",
        cabin.bg.unread
    );

    let only = cabin.threads[0].id.clone();
    let started = cabin.start_bg_task("slow-bg", &only, grokhub_core::BgOrigin::User);
    assert!(started.is_ok(), "{started:?}");
    assert_eq!(cabin.bg.runs.len(), 1);
    cabin
        .bg
        .unread
        .push((only.clone(), "- slow-bg (done): still going".into()));
    cabin.delete_thread_at(0);
    assert!(
        cabin.bg.runs.is_empty(),
        "ResetLast left runs: {} ({})",
        cabin.bg.runs.len(),
        cabin.status
    );
    assert!(
        cabin.bg.unread.iter().all(|(id, _)| id != &only),
        "{:?}",
        cabin.bg.unread
    );
    end_bg_test(root, cabin);
}

#[test]
fn hidden_logic_acts_on_input_once_per_pass() {
    // eframe 0.36 hands a hidden window's `logic` the last shown frame's input
    // every tick; drops, shortcuts, and activity must not replay from it.
    let src = cabin_src();
    let logic = fn_src(&src, "logic");
    let gate = logic
        .find("fresh_input = ")
        .expect("logic tracks the input pass");
    let drop = logic
        .find("take_dropped_attach")
        .expect("logic attaches drops");
    let keys = logic
        .find("logic_input_actions")
        .expect("logic runs shortcuts");
    assert!(gate < drop && gate < keys, "{logic}");
    assert!(logic.contains("cumulative_pass_nr"), "{logic}");
    assert!(
        !logic.contains("Key::N"),
        "shortcuts live behind the gate: {logic}"
    );
    let actions = fn_src(&src, "logic_input_actions");
    assert!(
        actions.contains("self.touch()") && actions.contains("Key::K"),
        "{actions}"
    );
}

#[test]
fn x_on_an_untouched_card_is_a_rejection_and_on_an_opened_one_is_not() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("feed-reject");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let _pin = crate::config::TestConfigDir::set(root.clone());

    let mut cabin = Cabin::quiet_for_test();
    cabin.cfg.quiet_start = "00:00".into();
    cabin.cfg.quiet_end = "00:00".into();
    cabin.note_automation_done("seen-src", "Seen report", "ok");
    cabin.note_automation_done("cold-src", "Cold report", "ok");
    let id_of = |cabin: &Cabin, src: &str| {
        cabin
            .updates
            .iter()
            .find(|card| card.source_id == src)
            .map(|card| card.id.clone())
            .expect("card")
    };
    let seen = id_of(&cabin, "seen-src");
    let cold = id_of(&cabin, "cold-src");
    cabin.open_feed_card(&seen);
    assert_eq!(cabin.card_prefs.uses["run:seen-src"].depth, 1);
    assert_eq!(
        cabin.card_prefs.uses["run:seen-src"].did,
        grokhub_core::UseAction::OpenedAutomations as u8
    );
    cabin.dismiss_feed_card(&seen);
    cabin.dismiss_feed_card(&cold);
    assert_eq!(
        cabin.card_prefs.groups["run:seen-src"].neg, 0.0,
        "opened, then closed"
    );
    assert_eq!(
        cabin.card_prefs.groups["run:cold-src"].neg, 3.0,
        "closed unopened"
    );
    // A run report closed unread is "seen": its group learns, its kind doesn't sink.
    assert_eq!(cabin.card_prefs.kinds["automation_done"].neg, 0.0);
    let signals = std::fs::read_to_string(root.join("card_signals.jsonl")).unwrap_or_default();
    let events: Vec<String> = signals
        .lines()
        .filter_map(|line| serde_json::from_str::<grokhub_core::CardSignal>(line).ok())
        .map(|row| format!("{}:{:?}:{:?}", row.group, row.event, row.after_open))
        .collect();
    assert_eq!(
        events,
        vec![
            "run:seen-src:Opened:None".to_string(),
            "run:seen-src:Dismissed:Some(true)".to_string(),
            "run:cold-src:Rejected:None".to_string(),
        ]
    );
    let _ = std::fs::remove_dir_all(&root);
    std::env::remove_var("GROKHUB_CONFIG");
}

#[test]
fn a_filed_idea_that_reaches_done_counts_as_completed_once() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("feed-complete");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let _pin = crate::config::TestConfigDir::set(root.clone());

    let mut cabin = Cabin::quiet_for_test();
    let idea =
        grokhub_core::idea_card("idea-src-7", "Draft a release checklist", "steps", now_ms());
    let id = idea.id.clone();
    let group = grokhub_core::signal_group(&idea);
    cabin.updates.push(idea);
    cabin.build_idea(&id);
    assert_eq!(
        cabin.card_prefs.uses.get(&group),
        Some(&grokhub_core::CardUse {
            depth: 2,
            did: 6,
            n: 1,
            at: cabin.card_prefs.uses[&group].at,
        })
    );
    cabin.note_finished_card_uses();
    assert_eq!(
        cabin.card_prefs.uses[&group].depth, 2,
        "the todo is not done yet"
    );
    let board_id = cabin
        .updates
        .iter()
        .find(|card| card.id == id)
        .and_then(|card| card.board_id.clone())
        .expect("filed");
    for card in cabin.board.iter_mut().filter(|card| card.id == board_id) {
        card.status = BoardStatus::Done;
    }
    cabin.note_finished_card_uses();
    cabin.note_finished_card_uses();
    let row = &cabin.card_prefs.uses[&group];
    assert_eq!((row.depth, row.did, row.n), (3, 8, 2));
    let signals = std::fs::read_to_string(root.join("card_signals.jsonl")).unwrap_or_default();
    assert_eq!(signals.matches("\"event\":\"ran\"").count(), 1, "{signals}");
    assert_eq!(
        signals.matches("\"event\":\"completed\"").count(),
        1,
        "{signals}"
    );
    assert!(!signals.contains("release checklist"), "{signals}");
    let _ = std::fs::remove_dir_all(&root);
    std::env::remove_var("GROKHUB_CONFIG");
}

/// 2.10.87: the chat window has no Thinking dot and no Background button.
/// The composer glow shows a live turn; `/bg <task>` starts work beside it.
#[test]
fn chat_window_has_no_thinking_dot_or_background_button() {
    let chat = include_str!("chat_ui.rs");
    let bg = include_str!("background.rs");
    assert!(
        !chat.contains("background_pill"),
        "Background button call is gone"
    );
    assert!(
        !bg.contains("fn background_pill"),
        "dead Background button fn is gone"
    );
    assert!(
        !chat.contains("\"Background\""),
        "no Background label in the chat window"
    );
    assert!(
        !chat.contains("paint_running("),
        "no running dot in the chat transcript"
    );
    assert!(
        !chat.contains("chat_run_label("),
        "no Thinking/Running/Waiting label in chat"
    );
    assert_eq!(chat.matches("move_turn_to_background").count(), 0);
    assert_eq!(bg.matches("fn run_bg_slash(").count(), 1, "/bg is kept");
    assert_eq!(
        bg.matches("fn move_turn_to_background(").count(),
        0,
        "a live reply no longer moves"
    );
}

/// The Thinking dot's hover moved onto Stop: idle says Send, a live turn says
/// what Stop stops.
#[test]
fn stop_hover_names_the_live_turn_now_the_dot_is_gone() {
    let mut cabin = Cabin::quiet_for_test();
    assert_eq!(cabin.go_tip_here(), "Send");
    cabin.running = true;
    cabin.chat_job_thread = Some(cabin.visible_thread_id());
    assert!(cabin.thinking_here());
    assert_eq!(cabin.go_tip_here(), "Stop · Working on your reply");
    cabin.stream_buf = "Half way.".into();
    assert_eq!(cabin.go_tip_here(), "Stop · Streaming a reply");
    cabin.running = false;
    cabin.chat_job_thread = None;
    assert_eq!(cabin.go_tip_here(), "Send");
}

/// 2.10.88: a finished turn's tool card (cleared only when the next turn starts)
/// and a pending ask must not leak into the idle Send hover.
#[test]
fn idle_send_hover_ignores_leftover_tool_cards_and_asks() {
    let mut cabin = Cabin::quiet_for_test();
    cabin.tool_cards.push(grokhub_core::wire::ToolCard {
        id: "t1".into(),
        title: "Read file".into(),
        kind: String::new(),
        status: "completed".into(),
        detail: String::new(),
        diff: String::new(),
        image_data_url: None,
        raw_input: String::new(),
    });
    assert!(!cabin.thinking_here());
    assert_eq!(cabin.run_action_here(), "Read file");
    assert_eq!(cabin.go_tip_here(), "Send");
    // A permission ask still pending from another chat's turn.
    cabin.perm_ask = Some(grokhub_core::wire::PermissionAsk {
        rpc_id: serde_json::Value::Null,
        session_id: "other".into(),
        title: "Write notes.md".into(),
        tool_call_id: "t2".into(),
        action: "write".into(),
        reason: String::new(),
        reject_option: None,
    });
    assert_eq!(cabin.go_tip_here(), "Send");
    cabin.perm_ask = None;
    cabin.running = true;
    cabin.chat_job_thread = Some(cabin.visible_thread_id());
    assert_eq!(cabin.go_tip_here(), "Stop · Read file");
}

/// Painted text of the live-work strip, the one place background work shows.
#[cfg(unix)]
fn live_work_text(cabin: &mut super::Cabin) -> String {
    let mut texts = Vec::new();
    let ctx = egui::Context::default();
    let _ = crate::theme::test_pass(&ctx, Default::default(), |ui| {
        egui::CentralPanel::default().show(ui, |ui| {
            cabin.paint_live_work(ui);
            let layer = ui.layer_id();
            ui.ctx().graphics(|layers| {
                if let Some(list) = layers.get(layer) {
                    for clipped in list.all_entries() {
                        collect_shape_text(&clipped.shape, &mut texts);
                    }
                }
            });
        });
    });
    texts.join("|")
}

/// 2.10.90: a scheduled job runs like a cron, out of sight and out of your way.
/// It starts and ends while your reply streams, your reply is never paused or
/// cut, you can send and finish a turn while it works, nothing shows it running,
/// and its report still lands on the Follow up card and the Home card.
#[cfg(unix)]
#[test]
fn a_scheduled_run_never_blocks_pauses_or_shows_in_your_chat() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = bg_cabin("bg-cron");
    cabin.board.clear();
    cabin.updates.clear();
    cabin.automations = vec![
        test_automation(
            "a-snap",
            "Host snapshot",
            "every day at 9, run a harbor-snapshot of the host and report what changed",
        ),
        test_automation(
            "a-disk",
            "Disk check",
            "every day at 10, run a harbor-snapshot of disk use and report what changed",
        ),
    ];
    cabin.nav = Nav::Chat;
    let chat_id = cabin.threads[0].id.clone();

    // 1. Your reply is streaming when the job comes due.
    let tx = live_reply(&mut cabin, "chat-gate: what is 2+2?", "Four.");
    let a = cabin.automations[0].clone();
    assert!(
        !cabin.scheduled_waits(&a),
        "your turn does not hold the job back"
    );
    cabin.fire_night(a, now_ms());
    assert!(cabin.bg.scheduled_live(), "{}", cabin.status);
    assert_eq!(cabin.bg.live_count(), 0);
    assert!(
        cabin.running && cabin.grok_p_rx.is_some(),
        "the start did not touch your turn"
    );
    assert_eq!(cabin.chat_job_thread.as_deref(), Some(chat_id.as_str()));
    assert!(
        !cabin.status.contains("Host snapshot") && !cabin.status.contains("Night"),
        "{}",
        cabin.status
    );
    assert_eq!(
        live_work_text(&mut cabin),
        "",
        "the strip does not show the job"
    );

    // 2. The job ends while your reply is still streaming: nothing pauses or cuts it.
    let start = std::time::Instant::now();
    while !cabin.bg.runs.is_empty() && start.elapsed() < std::time::Duration::from_secs(10) {
        cabin.poll_bg_runs();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(cabin.bg.runs.is_empty(), "the job ends");
    assert!(
        cabin.running && cabin.grok_p_rx.is_some(),
        "your reply keeps going"
    );
    assert!(cabin.followup_queue.is_empty());
    assert!(
        !cabin.status.contains("Host snapshot") && !cabin.status.contains("Night"),
        "{}",
        cabin.status
    );
    assert_eq!(
        cabin
            .messages
            .iter()
            .map(|m| (m.0.as_str(), m.1.as_str()))
            .collect::<Vec<_>>(),
        vec![("user", "chat-gate: what is 2+2?"), ("assistant", "Four.")],
        "your chat holds your turn and your reply, nothing else"
    );
    drop(tx);
    cabin.halt_in_flight();
    assert!(cabin.nav == Nav::Chat);

    // Its report is on the Follow up card and the Home card.
    let card = cabin
        .board
        .iter()
        .find(|c| c.automation.as_deref() == Some("a-snap"))
        .expect("follow up card")
        .clone();
    assert_eq!(card.status, grokhub_core::BoardStatus::FollowUp);
    assert_eq!(card.title, "Host snapshot");
    assert_eq!(card.report, "bg finished");
    let home = cabin
        .updates
        .iter()
        .find(|u| grokhub_core::feed_group_key(u).as_deref() == Some("run:a-snap"))
        .expect("home card");
    assert_eq!(home.board_id.as_deref(), Some(card.id.as_str()));
    assert_eq!(
        home.body.as_deref(),
        Some("In Follow up on your workboard. Open it to read and reply.")
    );

    // 3. A job is mid-turn: your next message goes out at once, not behind it.
    cabin.status.clear();
    let b = cabin.automations[1].clone();
    cabin.fire_night(b, now_ms());
    assert!(cabin.bg.scheduled_live());
    assert_eq!(cabin.status, "", "no status line while it runs");
    assert!(
        !cabin.running && cabin.chat_job_thread.is_none(),
        "no glow, no Stop, the composer is yours"
    );
    assert_eq!(live_work_text(&mut cabin), "");
    let hidden = cabin
        .threads
        .iter()
        .position(|t| t.title == crate::threads::BACKGROUND_THREAD_TITLE)
        .expect("Background chat");
    assert!(cabin.threads[hidden].background);
    cabin.send_chat("chat-gate: and 3+3?".into());
    assert!(
        cabin.followup_queue.is_empty(),
        "your message is sent, not queued"
    );
    assert_eq!(
        cabin.status,
        grokhub_core::XAI_NEED_SIGNIN,
        "it tried to send now"
    );
    assert!(cabin.bg.scheduled_live(), "the job still works");
    cabin.status.clear();
    let start = std::time::Instant::now();
    while !cabin.bg.runs.is_empty() && start.elapsed() < std::time::Duration::from_secs(10) {
        cabin.poll_bg_runs();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(cabin.bg.runs.is_empty());
    assert_eq!(cabin.status, "", "its end is quiet too");
    let disk = cabin
        .board
        .iter()
        .find(|c| c.automation.as_deref() == Some("a-disk"))
        .expect("second follow up card");
    assert_eq!(disk.report, "bg finished");
    end_bg_test(root, cabin);
}

/// 2.10.90: a report never lands in the middle of your reply in its own chat.
#[test]
fn a_scheduled_report_waits_while_you_reply_in_its_follow_up_chat() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("bg-cron-follow-up");
    std::fs::create_dir_all(&root).expect("config root");
    cabin.board.clear();
    cabin.automations = vec![test_automation(
        "a-snap",
        "Host snapshot",
        "every day at 9, run a read-only host snapshot and report what changed",
    )];
    cabin.threads = vec![crate::threads::ChatThread::new("Chat", false)];
    cabin.thread_idx = 0;
    cabin.settle_scheduled_run(
        Some("a-snap"),
        &grokhub_core::BgEnd::Done,
        "## Host snapshot\nDisk at 88% on /home.",
    );
    let card = cabin
        .board
        .iter()
        .find(|c| c.automation.as_deref() == Some("a-snap"))
        .expect("follow up card")
        .clone();
    let chat = card.thread_id.clone().expect("its chat");
    let at = cabin
        .threads
        .iter()
        .position(|t| t.id == chat)
        .expect("thread");
    assert_eq!(cabin.threads[at].messages.len(), 1);
    // You answer the report in its chat; the next run ends while your reply streams.
    cabin.thread_idx = at;
    cabin.messages = cabin.threads[at].messages.clone();
    cabin
        .live_mut()
        .push(("user".into(), "which mount grew?".into()));
    cabin
        .live_mut()
        .push(("assistant".into(), "/home grew by".into()));
    cabin.running = true;
    cabin.chat_job_thread = Some(chat.clone());
    let hidden = cabin.ensure_background_history_thread();
    let bg_id = cabin.threads[hidden].id.clone();
    cabin.bg.runs.push(super::background::BgRun {
        id: 1,
        thread_id: bg_id,
        title: "Host snapshot".into(),
        origin: grokhub_core::BgOrigin::Scheduled,
        rx: None,
        say: "## Host snapshot\nDisk at 91% on /home.".into(),
        action: String::new(),
        started: std::time::Instant::now(),
        end: Some(grokhub_core::BgEnd::Done),
        session: String::new(),
        resumed: None,
        fork_hold: false,
        native_session: None,
        automation: Some("a-snap".into()),
    });
    cabin.post_finished_bg_runs();
    assert_eq!(cabin.bg.runs.len(), 1, "the report waits for your reply");
    assert_eq!(
        cabin.messages.last().map(|m| m.1.as_str()),
        Some("/home grew by"),
        "your live reply is untouched"
    );
    cabin.running = false;
    cabin.chat_job_thread = None;
    cabin.post_finished_bg_runs();
    assert!(cabin.bg.runs.is_empty());
    let last = cabin
        .messages
        .last()
        .map(|m| m.1.clone())
        .unwrap_or_default();
    assert!(
        last.ends_with("## Host snapshot\nDisk at 91% on /home."),
        "{last}"
    );
    assert_eq!(cabin.messages.len(), 4);
    release_isolated(&root, cabin);
}

/// 2.10.90: only the job before it, or your turn for a desktop replay, holds a job.
#[test]
fn a_due_job_waits_only_for_the_last_job_or_a_desktop_replay() {
    let mut cabin = Cabin::quiet_for_test();
    let snap = test_automation(
        "a-snap",
        "Host snapshot",
        "every day at 9, run a host snapshot",
    );
    let replay = test_automation("a-replay", "Replay", "every day at 21, replay last");
    assert!(!cabin.scheduled_waits(&snap));
    cabin.running = true;
    assert!(
        !cabin.scheduled_waits(&snap),
        "your turn does not hold a job"
    );
    assert!(
        cabin.scheduled_waits(&replay),
        "a replay drives the desktop you are using"
    );
    cabin.running = false;
    assert!(!cabin.scheduled_waits(&replay));
    let (_tx, rx) = std::sync::mpsc::channel();
    cabin.bg.runs.push(super::background::BgRun {
        id: 1,
        thread_id: "bg".into(),
        title: "Disk check".into(),
        origin: grokhub_core::BgOrigin::Scheduled,
        rx: Some(rx),
        say: String::new(),
        action: String::new(),
        started: std::time::Instant::now(),
        end: None,
        session: String::new(),
        resumed: None,
        fork_hold: false,
        native_session: None,
        automation: Some("a-disk".into()),
    });
    assert!(cabin.scheduled_waits(&snap), "one scheduled run at a time");
    let night = include_str!("night.rs");
    let between = |from: &str, to: &str| -> String {
        night
            .split(from)
            .nth(1)
            .and_then(|s| s.split(to).next())
            .unwrap_or_else(|| panic!("missing {from}"))
            .to_string()
    };
    let tick = between("fn tick_night(", "fn poll_night_check(");
    assert!(!tick.contains("self.running"), "{tick}");
    assert!(tick.contains("self.scheduled_waits(&a)"), "{tick}");
    let check = between("fn poll_night_check(", "fn spawn_night_check(");
    assert!(
        check.contains("night_check_may_fire(self.scheduled_waits(&a))"),
        "{check}"
    );
    // A loop never rides your chat's Grok session, and no job writes the status line.
    let fire_loop = between("fn fire_loop(", "fn tick_session_suggestions(");
    assert!(
        !fire_loop.contains("self.acp") && !fire_loop.contains("self.running = true"),
        "{fire_loop}"
    );
    for said in ["\"Night: ", "\"Night check: ", "\"Loop: "] {
        assert!(!night.contains(said), "no job shows itself running: {said}");
    }
    assert!(!include_str!("native_unattended.rs").contains("\"Loop: "));
}
