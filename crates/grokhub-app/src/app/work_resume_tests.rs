//! KB-03: interrupted work resumes from its card's checkpoint. No network: the
//! chat resume is checked up to the turn input, and background resumes run on
//! the test model client.

use super::*;
use grokhub_core::work_item::{read_work_log, Checkpoint, StepStatus, WorkKind, WorkStep};
use grokhub_core::work_resume::{
    self as res, InterruptCause, Interruption, AUTO_RESUME_PER_DAY, HOST_CABIN, RESUME_BLOCK_MAX,
};
use grokhub_core::work_run::RunEnd;

fn log_texts(card_id: &str) -> Vec<String> {
    read_work_log(&crate::work_store::work_dir(), card_id)
        .into_iter()
        .map(|e| e.text)
        .collect()
}

/// A run card in Doing on a real chat, as the cabin left it when it quit.
fn doing_card(cabin: &mut Cabin, model: &str, session: &str) -> String {
    let thread = cabin.make_card_thread("Fix the tray icon", false);
    if let Some(t) = cabin.threads.iter_mut().find(|t| t.id == thread) {
        t.grok_session = Some(session.to_string());
        t.native_carry = true;
    }
    let mut c = BoardCard::new("Fix the tray icon", "", "");
    c.status = BoardStatus::InProgress;
    c.run = true;
    c.thread_id = Some(thread);
    c.steps = vec![
        WorkStep {
            text: "Read the tray code".into(),
            status: StepStatus::Done,
        },
        WorkStep {
            text: "Patch the DPI scaling".into(),
            status: StepStatus::Doing,
        },
    ];
    c.checkpoint = Some(Checkpoint {
        goal: "Fix the tray icon on Windows".into(),
        next: "Patch the DPI scaling".into(),
        session: session.into(),
        model: model.into(),
        attempt: 1,
        ..Default::default()
    });
    res::claim_run(&mut c, HOST_CABIN, grokhub_core::wire::PermissionMode::Auto);
    let id = c.id.clone();
    cabin.board.push(c);
    id
}

/// Run the startup pass again, as on a fresh start.
fn boot_pass(cabin: &mut Cabin) {
    cabin.work_resume.booted = false;
    if cabin.board_view.paused_seen.is_none() {
        cabin.board_view.paused_seen = Some(Default::default());
    }
    cabin.interrupt_boot_orphans();
}

fn card<'a>(cabin: &'a Cabin, id: &str) -> &'a BoardCard {
    cabin.board.iter().find(|c| c.id == id).expect("card")
}

#[test]
fn restart_interrupts_a_doing_card_and_the_resume_turn_carries_the_block() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("kb03-restart");
    cabin.board.clear();
    let model = cabin.run_model();
    let id = doing_card(&mut cabin, &model, "native-gone");

    // The first frames: the bell looks once, then the startup pass runs.
    cabin.tick_board_notices();
    cabin.interrupt_boot_orphans();
    let c = card(&cabin, &id);
    assert_eq!(c.status, BoardStatus::Blocked);
    assert_eq!(
        c.run_state.interrupted.map(|i| i.cause),
        Some(InterruptCause::Restart)
    );
    assert_eq!(
        c.checkpoint.as_ref().unwrap().blocker,
        "Interrupted: the app restarted."
    );
    assert_eq!(log_texts(&id), ["interrupted: the app restarted"]);
    // Work you started rings the bell and waits for Resume.
    cabin.tick_board_notices();
    assert!(cabin
        .notices
        .items
        .iter()
        .any(|n| n.text == "Interrupted: Fix the tray icon. Resume it from the Workboard."));
    cabin.tick_auto_resume();
    assert_eq!(card(&cabin, &id).status, BoardStatus::Blocked, "never auto");

    // Resume: the session file is gone, so a fresh session gets only the block.
    let block = res::resume_block(card(&cabin, &id));
    assert!(block.len() <= RESUME_BLOCK_MAX);
    let snapshot = card(&cabin, &id).clone();
    cabin.prepare_chat_resume(&snapshot).expect("resume");
    assert_eq!(cabin.work_resume.follow.as_deref(), Some(block.as_str()));
    assert_eq!(cabin.work_resume.pending.as_deref(), Some(id.as_str()));
    let thread = snapshot.thread_id.clone().unwrap();
    let t = cabin.threads.iter().find(|t| t.id == thread).unwrap();
    let fresh = t.grok_session.clone().unwrap();
    assert_ne!(fresh, "native-gone", "a new session");
    assert!(!t.native_carry, "no recap of earlier turns rides along");
    assert_eq!(
        t.messages.last().map(|m| m.1.as_str()),
        Some("Resume: Fix the tray icon")
    );
    // The turn input is the block, then the ask: nothing else.
    assert!(cabin.card_notes_follow.is_none() && cabin.bg.results_follow.is_none());
    let input = grokhub_core::apply_skill_follow(
        "Resume: Fix the tray icon",
        cabin.work_resume.follow.as_deref(),
    );
    assert_eq!(input, format!("{block}\n\nResume: Fix the tray icon"));
    assert!(input.contains("Next: Patch the DPI scaling"));
    assert_eq!(card(&cabin, &id).status, BoardStatus::InProgress);
    assert!(card(&cabin, &id).run_state.interrupted.is_none());

    // The turn starts on the resumed card: a second attempt on the new session.
    cabin.note_inflight_card("Resume: Fix the tray icon", "");
    assert!(cabin.inflight_open && cabin.work_resume.pending.is_none());
    let cp = card(&cabin, &id).checkpoint.clone().unwrap();
    assert_eq!(cp.attempt, 2);
    assert_eq!(cp.session, fresh);
    assert_eq!(cp.blocker, "");
    assert_eq!(cabin.board.len(), 1, "no second card");
    // Turns after the resume carry nothing more.
    cabin.settle_turn_card("Patched it.");
    assert!(cabin.work_resume.follow.is_none());
    assert_eq!(card(&cabin, &id).status, BoardStatus::Done);
    cabin.chat_job_thread = None;
    assert_eq!(
        log_texts(&id),
        [
            "interrupted: the app restarted",
            "resume: fresh session",
            "attempt 2",
            "done"
        ]
    );
    release_isolated(&root, cabin);
}

#[test]
fn same_model_with_its_session_continues_it_and_a_swap_starts_fresh() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("kb03-swap");
    cabin.board.clear();
    let model = cabin.run_model();
    grokhub_agent::record_turn(
        "native-kept",
        "/work",
        &model,
        &[grokhub_agent::InputItem::Message {
            role: "user".into(),
            content: vec![grokhub_agent::ContentPart::InputText("fix the tray".into())],
        }],
        &grokhub_agent::Usage::default(),
        "",
        "fix the tray",
    )
    .expect("session file");
    assert!(grokhub_agent::session_intact("native-kept"));

    // Same model, session intact: continue it (prefix cache) plus the block.
    let same = doing_card(&mut cabin, &model, "native-kept");
    boot_pass(&mut cabin);
    let snap = card(&cabin, &same).clone();
    cabin.prepare_chat_resume(&snap).expect("resume");
    let thread = snap.thread_id.clone().unwrap();
    let t = cabin.threads.iter().find(|t| t.id == thread).unwrap();
    assert_eq!(t.grok_session.as_deref(), Some("native-kept"));
    assert_eq!(
        log_texts(&same).last().map(String::as_str),
        Some("resume: same session")
    );
    cabin.abandon_turn_card();
    assert_eq!(
        card(&cabin, &same).status,
        BoardStatus::Blocked,
        "a resume that never started goes back"
    );
    assert!(cabin.work_resume.follow.is_none());

    // The route swapped the model mid-run: a fresh session with the block only.
    let mut swapped = card(&cabin, &same).clone();
    swapped.run_state.interrupted = Some(Interruption {
        cause: InterruptCause::ModelSwap,
        at_ms: 1,
        auto_used: false,
    });
    cabin.prepare_chat_resume(&swapped).expect("resume");
    let t = cabin.threads.iter().find(|t| t.id == thread).unwrap();
    assert_ne!(t.grok_session.as_deref(), Some("native-kept"));
    assert!(cabin
        .work_resume
        .follow
        .as_deref()
        .is_some_and(|b| b.contains("after the model changed mid-run")));
    cabin.abandon_turn_card();

    // Another model in Settings: fresh too.
    let other = doing_card(&mut cabin, "grok-other", "native-kept");
    boot_pass(&mut cabin);
    let snap = card(&cabin, &other).clone();
    cabin.prepare_chat_resume(&snap).expect("resume");
    assert_eq!(
        log_texts(&other).last().map(String::as_str),
        Some("resume: fresh session")
    );
    cabin.abandon_turn_card();
    release_isolated(&root, cabin);
}

#[test]
fn a_cut_turn_is_interrupted_and_a_swap_cut_halts_the_turn() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("kb03-cut");
    cabin.board.clear();
    cabin.chat_job_thread = Some("t-cut".into());
    cabin.note_inflight_card("fix the crash on resume", "");
    let id = cabin.board.last().unwrap().id.clone();
    cabin.interrupt_turn_card(InterruptCause::WorkerGone);
    let c = card(&cabin, &id);
    assert_eq!(c.status, BoardStatus::Blocked);
    assert_eq!(
        c.run_state.interrupted.map(|i| i.cause),
        Some(InterruptCause::WorkerGone)
    );
    assert_eq!(c.run_state.host, HOST_CABIN);
    assert!(!cabin.inflight_open);

    // The swap check flags the live turn; the next frame cuts it.
    cabin.chat_job_thread = Some("t-swap".into());
    cabin.note_inflight_card("write the release notes", "");
    let swap = cabin.board.last().unwrap().id.clone();
    cabin.running = true;
    cabin.work_resume.swap_cut = true;
    cabin.tick_work_resume();
    assert!(!cabin.running && !cabin.work_resume.swap_cut);
    assert_eq!(
        card(&cabin, &swap).run_state.interrupted.map(|i| i.cause),
        Some(InterruptCause::ModelSwap)
    );
    assert_eq!(card(&cabin, &swap).status, BoardStatus::Blocked);
    release_isolated(&root, cabin);
}

/// The agent's own background card, interrupted, with the run it last had.
fn agent_bg_card(cabin: &mut Cabin, mode: grokhub_core::wire::PermissionMode) -> String {
    let mut c = BoardCard::new("Check the backups", "", "");
    c.origin = Some(grokhub_core::WorkOrigin::Agent);
    c.kind = WorkKind::Background;
    c.status = BoardStatus::InProgress;
    c.checkpoint = Some(Checkpoint {
        goal: "Check the backups".into(),
        next: "Verify last night's archive".into(),
        session: "native-bg-gone".into(),
        model: cabin.run_model(),
        attempt: 1,
        ..Default::default()
    });
    res::claim_run(&mut c, HOST_CABIN, mode);
    let id = c.id.clone();
    cabin.board.push(c);
    id
}

#[cfg(unix)]
#[test]
fn agent_background_work_auto_resumes_twice_then_waits_with_a_bell() {
    use grokhub_core::wire::PermissionMode;
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = bg_cabin("kb03-auto");
    cabin.cfg.api_key = "sk-abcdefghijklmnopqrstuv".into();
    assert!(cabin.agent_ready());
    cabin.board.clear();
    cabin.permission_mode = PermissionMode::Auto;
    let id = agent_bg_card(&mut cabin, PermissionMode::Ask);
    cabin.tick_board_notices();
    cabin.interrupt_boot_orphans();
    let bells = cabin.notices.items.len();
    for round in 1..=AUTO_RESUME_PER_DAY {
        cabin.tick_board_notices();
        assert_eq!(
            cabin.notices.items.len(),
            bells,
            "no bell while it resumes itself"
        );
        cabin.tick_auto_resume();
        let c = card(&cabin, &id);
        assert_eq!(c.status, BoardStatus::InProgress, "round {round}");
        assert_eq!(c.run_state.auto_resumes.len(), round);
        // Resume keeps the mode it started in, though the pill is now Auto.
        assert_eq!(c.run_state.mode, "ask");
        let cp = c.checkpoint.as_ref().unwrap();
        assert_eq!(cp.attempt as usize, round + 1);
        assert_ne!(cp.session, "native-bg-gone", "lost session: fresh");
        let run_id = cabin
            .bg
            .cards
            .iter()
            .find(|(_, card)| *card == id)
            .map(|(r, _)| *r)
            .expect("the run checkpoints this card");
        // The provider cuts it again.
        cabin.finish_bg_card(run_id, &RunEnd::Failed("502".into()), false);
        assert_eq!(card(&cabin, &id).status, BoardStatus::Blocked);
    }
    cabin.tick_auto_resume();
    let c = card(&cabin, &id);
    assert_eq!(c.status, BoardStatus::Blocked, "two a day, then it waits");
    assert_eq!(
        c.checkpoint.as_ref().unwrap().blocker,
        res::give_up_blocker()
    );
    assert_eq!(cabin.notices.items.len(), bells + 1);
    assert!(cabin.notices.items[0]
        .text
        .starts_with("Auto-resume stopped: Check the backups"));
    cabin.tick_board_notices();
    assert_eq!(cabin.notices.items.len(), bells + 1, "one bell, not two");
    cabin.tick_auto_resume();
    assert_eq!(card(&cabin, &id).status, BoardStatus::Blocked);
    end_bg_test(root, cabin);
}

#[test]
fn work_you_started_never_auto_resumes() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("kb03-yours");
    cabin.cfg.api_key = "sk-abcdefghijklmnopqrstuv".into();
    cabin.board.clear();
    let model = cabin.run_model();
    let chat = doing_card(&mut cabin, &model, "native-x");
    // `/bg` you typed is yours, not the agent's.
    cabin.open_bg_card(3, "Check the backups", "", true);
    let typed = cabin.bg_card_id(3).unwrap();
    cabin.finish_bg_card(3, &RunEnd::Failed("502".into()), false);
    boot_pass(&mut cabin);
    for _ in 0..3 {
        cabin.tick_auto_resume();
    }
    for id in [&chat, &typed] {
        let c = card(&cabin, id);
        assert_eq!(c.status, BoardStatus::Blocked);
        assert!(c.run_state.interrupted.is_some());
        assert!(c.run_state.auto_resumes.is_empty());
    }
    assert!(cabin.bg.runs.is_empty(), "nothing started");
    assert_eq!(
        cabin.resume_card_now(&chat, true),
        Err("Work you started waits for Resume".into())
    );
    release_isolated(&root, cabin);
}

#[test]
fn the_resume_block_rides_in_the_turn_input_and_the_guard_comes_first() {
    let src = include_str!("chat_kick.rs").replace("\r\n", "\n");
    let kick = src
        .split("pub(super) fn kick_model(")
        .nth(1)
        .and_then(|s| s.split("\n    pub(super) fn ").next())
        .expect("kick_model");
    assert!(
        kick.contains("self.work_resume.follow.as_deref()"),
        "the block goes in the turn input: {kick}"
    );
    let prompt = include_str!("native_engine.rs");
    assert!(
        !prompt.contains("work_resume"),
        "never in the system prompt"
    );
    let resume = include_str!("work_resume.rs").replace("\r\n", "\n");
    let now = resume
        .split("pub(super) fn resume_card_now(")
        .nth(1)
        .expect("resume_card_now");
    let guard_at = now.find("resume_guard_blocks").expect("guard");
    let start_at = now.find("resume_bg_card").expect("start");
    assert!(guard_at < start_at, "game guard before any resumed step");
    assert!(resume.contains("Guarded::DesktopControl"));
}
