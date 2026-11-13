use super::*;
use std::path::PathBuf;

use grokhub_agent::{ClientError, PermMode, ResponsesRequest, StreamEvent, TurnOutput};
use grokhub_core::wire::PermissionMode;
use grokhub_core::work_resume::InterruptCause;
use grokhub_core::{BoardStatus, WorkStep};

const NOW: u64 = 1_800_000_000_000;
const HOUR: u64 = 60 * 60 * 1000;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "grokhub-shell-run-resume-{name}-{}-{}",
        std::process::id(),
        grokhub_core::now_ms()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

/// A card the shell ran in `mode` on session `s-1` and grok-4.7, cut by a restart.
fn interrupted(id: &str, kind: WorkKind, mode: PermissionMode) -> BoardCard {
    let mut c = BoardCard::new(&format!("Card {id}"), "", "");
    c.origin = Some(grokhub_core::WorkOrigin::Agent);
    c.id = id.to_string();
    c.status = BoardStatus::InProgress;
    c.run = true;
    c.steps = vec![
        WorkStep {
            text: "Fetch the feed".into(),
            status: grokhub_core::StepStatus::Done,
        },
        WorkStep {
            text: "Write the digest".into(),
            status: grokhub_core::StepStatus::Doing,
        },
    ];
    wr::begin_run(&mut c, kind, "s-1", "grok-4.7", NOW - HOUR);
    res::claim_run(&mut c, res::HOST_SHELL, mode);
    assert!(res::interrupt_card(
        &mut c,
        InterruptCause::Restart,
        NOW - HOUR
    ));
    c
}

fn seed(dir: &Path, cards: Vec<BoardCard>) {
    wr::edit_board_file(&wr::board_path(dir), NOW - HOUR, |all| *all = cards).expect("seed");
}

fn card(dir: &Path, id: &str) -> BoardCard {
    read_card(dir, id).expect("card")
}

fn notes(dir: &Path, id: &str) -> Vec<String> {
    grokhub_core::work_item::read_work_log(&wr::work_dir(dir), id)
        .into_iter()
        .map(|e| e.text)
        .collect()
}

const ALWAYS: RunGate = RunGate {
    mode: PermMode::Always,
    desktop: false,
};

fn intact_s1(session: &str) -> bool {
    session == "s-1"
}

/// An automation the card belongs to.
fn automation(dir: &Path) -> String {
    let clock = crate::automations::Clock {
        weekday: 2,
        hour: 8,
        minute: 0,
    }
    .at(NOW);
    crate::automations::add(dir, "every day at 7am digest the feed", clock).expect("add")[0]
        .id
        .clone()
}

#[test]
fn an_automation_card_continues_its_session_with_the_block_and_keeps_ask() {
    let dir = temp_dir("same");
    let job = automation(&dir);
    let mut c = interrupted("a-1", WorkKind::Automation, PermissionMode::Ask);
    c.automation = Some(job.clone());
    seed(&dir, vec![c]);

    let p = prepare(&dir, "a-1", "grok-4.7", ALWAYS, false, &intact_s1, 0, NOW).expect("prepared");
    assert_eq!((p.session_id.as_str(), p.resume), ("s-1", true));
    // Started in Ask: the resume stays in Ask though Approvals is Always now.
    assert_eq!(p.gate.mode, PermMode::Ask);
    assert_eq!(
        p.block,
        "Resuming workboard card a-1 \"Card a-1\" after the app restarted, from its checkpoint.\n\
         Goal: Card a-1\nNext: Write the digest\nBlocked by: Interrupted: the app restarted.\n\
         Steps:\n1. [done] Fetch the feed\n2. [doing] Write the digest\n\
         Pick up at Next and do not redo done steps. Check the card with work_get and \
         checkpoint it with work_update (load them by name)."
    );
    let now = card(&dir, "a-1");
    assert_eq!(now.status, BoardStatus::InProgress);
    assert_eq!(now.run_state.interrupted, None);
    assert_eq!(now.run_state.mode, "ask");
    assert_eq!(now.checkpoint.as_ref().map(|cp| cp.attempt), Some(2));
    assert_eq!(notes(&dir, "a-1"), ["resume: same session", "attempt 2"]);

    // It runs now: a second resume is refused.
    assert_eq!(
        prepare(&dir, "a-1", "grok-4.7", ALWAYS, false, &intact_s1, 0, NOW)
            .err()
            .as_deref(),
        Some("That run is already going.")
    );
    assert!(crate::automations::is_running(&job));
    drop(p);
    assert_eq!(
        prepare(&dir, "a-1", "grok-4.7", ALWAYS, false, &intact_s1, 0, NOW)
            .err()
            .as_deref(),
        Some("Only a paused or interrupted run can resume.")
    );
}

#[test]
fn another_model_or_a_lost_session_starts_fresh_and_a_chat_card_is_not_unattended() {
    let dir = temp_dir("fresh");
    seed(
        &dir,
        vec![
            interrupted("b-1", WorkKind::Background, PermissionMode::AlwaysApprove),
            interrupted("b-2", WorkKind::Background, PermissionMode::Auto),
            interrupted("t-1", WorkKind::Task, PermissionMode::Ask),
        ],
    );
    let p = prepare(&dir, "b-1", "grok-5", ALWAYS, false, &intact_s1, 0, NOW).expect("b-1");
    assert!(!p.resume);
    assert_eq!(p.session_id, format!("shell-resume-b-1-{NOW}"));
    assert_eq!(p.gate.mode, PermMode::Always);
    assert_eq!(notes(&dir, "b-1"), ["resume: fresh session", "attempt 2"]);

    let gone = |_: &str| false;
    let p = prepare(&dir, "b-2", "grok-4.7", ALWAYS, false, &gone, 0, NOW).expect("b-2");
    assert!(!p.resume);
    assert_eq!(p.gate.mode, PermMode::Auto);

    assert_eq!(
        prepare(&dir, "t-1", "grok-4.7", ALWAYS, false, &intact_s1, 0, NOW)
            .err()
            .as_deref(),
        Some("Only an automation's or a background run's card resumes on its own.")
    );
}

struct Answers;

impl ModelClient for Answers {
    fn stream(
        &self,
        _req: &ResponsesRequest,
        _cancel: &CancelToken,
        sink: &mut dyn FnMut(StreamEvent),
    ) -> Result<TurnOutput, ClientError> {
        sink(StreamEvent::TextDelta("Digest written.".into()));
        Ok(TurnOutput {
            text: "Digest written.".into(),
            ..TurnOutput::default()
        })
    }
}

struct Stalls;

impl ModelClient for Stalls {
    fn stream(
        &self,
        _req: &ResponsesRequest,
        _cancel: &CancelToken,
        _sink: &mut dyn FnMut(StreamEvent),
    ) -> Result<TurnOutput, ClientError> {
        Err(ClientError::IdleTimeout)
    }
}

#[test]
fn a_resumed_run_settles_its_card_done_or_interrupted_again() {
    let (dir, workspace) = (temp_dir("run-cfg"), temp_dir("run-ws"));
    seed(
        &dir,
        vec![
            interrupted("b-1", WorkKind::Background, PermissionMode::Ask),
            interrupted("b-2", WorkKind::Background, PermissionMode::Ask),
        ],
    );
    let never = |_: &str| false;
    let p = prepare(&dir, "b-1", "grok-4.7", ALWAYS, false, &never, 0, NOW).expect("b-1");
    let done = run(&dir, p, Arc::new(Answers), "grok-4.7", workspace.clone());
    assert_eq!(
        (done.text.as_str(), done.error.as_str()),
        ("Digest written.", "")
    );
    // A finished background run archives itself, as its first run would.
    let b1 = card(&dir, "b-1");
    assert_eq!(b1.status, BoardStatus::Dismissed);
    assert!(b1.archived_ms > 0);
    // Its reply lands on the card, as a queued run's does.
    assert_eq!(b1.result, "Digest written.");
    assert_eq!(notes(&dir, "b-1").last().map(String::as_str), Some("done"));
    assert_eq!(live_count(), 0);

    let p = prepare(&dir, "b-2", "grok-4.7", ALWAYS, false, &never, 0, NOW).expect("b-2");
    // The run takes its room from the claim on, before it starts.
    assert_eq!(live_count(), 1);
    let failed = run(&dir, p, Arc::new(Stalls), "grok-4.7", workspace);
    assert!(failed.failed(), "{failed:?}");
    let b2 = card(&dir, "b-2");
    assert_eq!(b2.status, BoardStatus::Blocked);
    assert_eq!(
        b2.run_state.interrupted.map(|i| i.cause),
        Some(InterruptCause::from_failure(&failed.error))
    );
    assert!(notes(&dir, "b-2")
        .last()
        .is_some_and(|l| l.starts_with("interrupted: ")));
}

#[test]
fn the_agents_cards_auto_resume_twice_a_day_then_wait_with_a_bell() {
    let dir = temp_dir("auto");
    let mut mine = interrupted("t-1", WorkKind::Background, PermissionMode::Ask);
    mine.touched = true;
    seed(
        &dir,
        vec![
            interrupted("b-1", WorkKind::Background, PermissionMode::Ask),
            mine,
        ],
    );
    let never = |_: &str| false;
    let picks = |at: u64| -> Vec<(String, AutoResume)> {
        auto_picks(&dir, at)
            .into_iter()
            .map(|(c, d)| (c.id, d))
            .collect()
    };
    // Work you touched never resumes on its own.
    assert_eq!(picks(NOW), [("b-1".to_string(), AutoResume::Go)]);

    for (n, at) in [(1, NOW), (2, NOW + HOUR)] {
        let p = prepare(&dir, "b-1", "grok-4.7", ALWAYS, true, &never, 0, at).expect("resume");
        drop(p);
        // The run was cut again.
        wr::edit_board_file(&wr::board_path(&dir), at, |cards| {
            let c = cards.iter_mut().find(|c| c.id == "b-1").expect("b-1");
            assert!(res::interrupt_card(
                c,
                InterruptCause::ProviderError,
                at + 1
            ));
        })
        .expect("cut");
        assert_eq!(card(&dir, "b-1").run_state.auto_resumes.len(), n);
    }
    assert_eq!(
        picks(NOW + 2 * HOUR),
        [("b-1".to_string(), AutoResume::GiveUp)]
    );
    let stopped = give_up(&dir, "b-1", NOW + 2 * HOUR).expect("stopped");
    assert_eq!(
        stopped,
        Stopped {
            card_id: "b-1".into(),
            text: "Auto-resume stopped: Card b-1. Resume it from the Workboard.".into(),
        }
    );
    let b1 = card(&dir, "b-1");
    assert_eq!(b1.status, BoardStatus::Blocked);
    assert_eq!(
        b1.checkpoint.map(|cp| cp.blocker),
        Some(res::give_up_blocker())
    );
    assert!(picks(NOW + 2 * HOUR).is_empty());
    assert_eq!(
        notes(&dir, "b-1").last().map(String::as_str),
        Some("auto-resume stopped: twice today")
    );
}

#[test]
fn an_auto_resume_that_cannot_start_spends_its_try_and_says_why() {
    let dir = temp_dir("failed");
    seed(
        &dir,
        vec![interrupted(
            "b-1",
            WorkKind::Background,
            PermissionMode::Ask,
        )],
    );
    auto_failed(&dir, "b-1", "Sign in to xAI first.", NOW);
    let b1 = card(&dir, "b-1");
    assert_eq!(
        b1.checkpoint.map(|cp| cp.blocker).as_deref(),
        Some("Auto-resume failed: Sign in to xAI first.")
    );
    assert!(auto_picks(&dir, NOW).is_empty());
    assert_eq!(notes(&dir, "b-1"), ["auto-resume failed"]);
}

#[test]
fn an_auto_resume_waits_for_room_and_for_a_game_to_end() {
    let bg = interrupted("b-1", WorkKind::Background, PermissionMode::Ask);
    let desk = interrupted("e-1", WorkKind::Episode, PermissionMode::Ask);
    assert!(!waits(&bg, 0, false, None));
    assert!(waits(&bg, grokhub_core::BG_TASK_MAX, false, None));
    // A game holds the desktop: only a run that may drive it waits.
    assert!(!waits(&bg, 0, false, Some("A game is running")));
    assert!(waits(&bg, 0, true, Some("A game is running")));
    assert!(waits(&desk, 0, false, Some("A game is running")));
}

#[test]
fn an_auto_resume_skips_a_card_whose_automation_is_off() {
    let dir = temp_dir("off");
    let job = automation(&dir);
    let clock = crate::automations::Clock {
        weekday: 2,
        hour: 8,
        minute: 0,
    }
    .at(NOW);
    crate::automations::set_enabled(&dir, &job, false, clock).expect("off");
    let mut c = interrupted("a-1", WorkKind::Automation, PermissionMode::Ask);
    c.automation = Some(job);
    seed(&dir, vec![c]);
    assert_eq!(
        prepare(&dir, "a-1", "grok-4.7", ALWAYS, true, &intact_s1, 0, NOW)
            .err()
            .as_deref(),
        Some("Its automation is off.")
    );
    // Nothing was spent or moved.
    let a1 = card(&dir, "a-1");
    assert_eq!(a1.status, BoardStatus::Blocked);
    assert!(a1.run_state.auto_resumes.is_empty());
    // You can still resume it by hand.
    assert!(prepare(&dir, "a-1", "grok-4.7", ALWAYS, false, &intact_s1, 0, NOW).is_ok());
}

#[test]
fn a_late_tick_leaves_a_card_you_resumed_alone() {
    let dir = temp_dir("late");
    seed(
        &dir,
        vec![interrupted(
            "b-1",
            WorkKind::Background,
            PermissionMode::Ask,
        )],
    );
    // The tick read it Blocked, then you resumed it.
    let p = prepare(&dir, "b-1", "grok-4.7", ALWAYS, false, &intact_s1, 0, NOW).expect("resume");
    assert_eq!(give_up(&dir, "b-1", NOW), None);
    auto_failed(&dir, "b-1", "Sign in to xAI first.", NOW);
    let b1 = card(&dir, "b-1");
    assert_eq!(b1.status, BoardStatus::InProgress);
    assert!(b1.run_state.auto_resumes.is_empty());
    assert_eq!(b1.checkpoint.map(|cp| cp.blocker).as_deref(), Some(""));
    assert_eq!(notes(&dir, "b-1"), ["resume: same session", "attempt 2"]);
    drop(p);
}

#[test]
fn a_card_resumes_once_at_a_time_within_the_shared_room() {
    let dir = temp_dir("room");
    seed(
        &dir,
        vec![
            interrupted("b-1", WorkKind::Background, PermissionMode::Ask),
            interrupted("b-2", WorkKind::Background, PermissionMode::Ask),
        ],
    );
    let never = |_: &str| false;
    let p = prepare(&dir, "b-1", "grok-4.7", ALWAYS, false, &never, 0, NOW).expect("b-1");
    assert_eq!(status_lines(), ["Running · Resumed: Card b-1"]);
    // You moved it back to Blocked while it runs: it still can't run twice.
    wr::edit_board_file(&wr::board_path(&dir), NOW, |cards| {
        cards[0].status = BoardStatus::Blocked;
    })
    .expect("move");
    assert_eq!(
        prepare(&dir, "b-1", "grok-4.7", ALWAYS, false, &never, 0, NOW)
            .err()
            .as_deref(),
        Some("That run is already going.")
    );
    // Two /bg runs and this one fill the room.
    assert_eq!(
        prepare(&dir, "b-2", "grok-4.7", ALWAYS, false, &never, 2, NOW)
            .err()
            .as_deref(),
        Some("3 background tasks are already running. /bg stop stops them.")
    );
    assert_eq!(card(&dir, "b-2").status, BoardStatus::Blocked);
    drop(p);
    assert_eq!(live_count(), 0);
    assert!(status_lines().is_empty());
}

#[test]
fn a_removed_automation_s_card_never_resumes_with_every_tool() {
    let dir = temp_dir("removed");
    let mut c = interrupted("a-1", WorkKind::Automation, PermissionMode::AlwaysApprove);
    c.automation = Some("gone-job".into());
    seed(&dir, vec![c]);
    assert_eq!(
        prepare(&dir, "a-1", "grok-4.7", ALWAYS, false, &intact_s1, 0, NOW)
            .err()
            .as_deref(),
        Some("Its automation was removed, so its run can't resume.")
    );
    assert_eq!(card(&dir, "a-1").status, BoardStatus::Blocked);
    assert_eq!(live_count(), 0);
}

/// Waits until it is stopped.
struct Hangs;

impl ModelClient for Hangs {
    fn stream(
        &self,
        _req: &ResponsesRequest,
        cancel: &CancelToken,
        _sink: &mut dyn FnMut(StreamEvent),
    ) -> Result<TurnOutput, ClientError> {
        while !cancel.is_cancelled() {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        Err(ClientError::Cancelled)
    }
}

#[test]
fn bg_stop_stops_a_resumed_run_and_its_card_waits_in_blocked() {
    let (dir, workspace) = (temp_dir("stop-cfg"), temp_dir("stop-ws"));
    seed(
        &dir,
        vec![interrupted(
            "b-1",
            WorkKind::Background,
            PermissionMode::Ask,
        )],
    );
    let never = |_: &str| false;
    let p = prepare(&dir, "b-1", "grok-4.7", ALWAYS, false, &never, 0, NOW).expect("b-1");
    let (d, w) = (dir.clone(), workspace.clone());
    let going = std::thread::spawn(move || run(&d, p, Arc::new(Hangs), "grok-4.7", w));
    while crate::bg_runs::status() != "Running · Resumed: Card b-1" {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(crate::bg_runs::stop_text(), "Stopped 1 background task");
    let ended = going.join().expect("run");
    assert_eq!(ended.stop, "halted", "{ended:?}");
    // Stopped, not interrupted: it waits for you and never resumes on its own.
    let b1 = card(&dir, "b-1");
    assert_eq!(b1.status, BoardStatus::Blocked);
    assert_eq!(b1.run_state.interrupted, None);
    assert_eq!(
        b1.checkpoint.map(|cp| cp.blocker).as_deref(),
        Some(wr::STOPPED_BLOCKER)
    );
    assert!(auto_picks(&dir, NOW).is_empty());
    assert_eq!(live_count(), 0);
}

#[test]
fn a_run_whose_card_you_moved_leaves_it_where_you_put_it() {
    let (dir, workspace) = (temp_dir("moved-cfg"), temp_dir("moved-ws"));
    seed(
        &dir,
        vec![interrupted(
            "b-1",
            WorkKind::Background,
            PermissionMode::Ask,
        )],
    );
    let never = |_: &str| false;
    let p = prepare(&dir, "b-1", "grok-4.7", ALWAYS, false, &never, 0, NOW).expect("b-1");
    wr::edit_board_file(&wr::board_path(&dir), NOW, |cards| {
        cards[0].status = BoardStatus::Todo;
    })
    .expect("move");
    run(&dir, p, Arc::new(Answers), "grok-4.7", workspace);
    let b1 = card(&dir, "b-1");
    assert_eq!(b1.status, BoardStatus::Todo);
    assert_eq!(b1.result, "");
    assert_eq!(
        notes(&dir, "b-1").last().map(String::as_str),
        Some("resume ended after the card moved")
    );
}
