//! KB-02: agent work logs itself on the board. Chats file nothing; a work turn,
//! a background run and an automation each checkpoint their card.

use super::*;
use grokhub_core::work_item::{read_work_log, WorkKind};
use grokhub_core::work_run::{RunEnd, STOPPED_BLOCKER};

fn tool(id: &str, kind: &str, status: &str) -> ToolCard {
    ToolCard {
        id: id.into(),
        title: kind.into(),
        kind: kind.into(),
        status: status.into(),
        detail: String::new(),
        diff: String::new(),
        image_data_url: None,
        raw_input: String::new(),
    }
}

fn log_texts(card_id: &str) -> Vec<String> {
    read_work_log(&crate::work_store::work_dir(), card_id)
        .into_iter()
        .map(|e| e.text)
        .collect()
}

#[test]
fn chat_turn_files_no_card_and_logs_nothing() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("kb02-chat");
    cabin.board.clear();
    cabin.chat_job_thread = Some("t-chat".into());
    cabin.note_inflight_card("what do you think of rust?", "");
    assert!(!cabin.inflight_open);
    cabin.log_run_tool(&tool("c1", "read_file", "completed"));
    cabin.settle_turn_card("Rust is nice.");
    assert!(cabin.board.is_empty(), "{:?}", cabin.board);
    assert!(!crate::work_store::work_dir().exists(), "no progress log");
    release_isolated(&root, cabin);
}

#[test]
fn work_turn_logs_each_tool_once_and_stop_keeps_checkpoint() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("kb02-turn");
    cabin.board.clear();
    cabin.chat_job_thread = Some("t-work".into());
    cabin.note_inflight_card("fix the crash on resume", "");
    assert!(cabin.inflight_open);
    let card = cabin.board.last().cloned().expect("a Doing card");
    assert_eq!(card.status, BoardStatus::InProgress);
    assert_eq!(card.kind, WorkKind::Task);
    let cp = card.checkpoint.clone().expect("checkpoint");
    assert_eq!(cp.attempt, 1);
    assert!(!cp.goal.is_empty());
    assert!(!cp.model.is_empty());

    cabin.log_run_tool(&tool("a", "read_file", "in_progress"));
    cabin.log_run_tool(&tool("a", "read_file", "completed"));
    cabin.log_run_tool(&tool("a", "read_file", "completed"));
    cabin.log_run_tool(&tool("b", "search_replace", "failed"));
    assert_eq!(
        log_texts(&card.id),
        ["attempt 1", "read_file ok", "search_replace fail"],
        "one line per finished call"
    );

    // Stop: the card goes to Blocked and keeps where it was.
    cabin.abandon_turn_card();
    let stopped = cabin.board.iter().find(|c| c.id == card.id).unwrap();
    assert_eq!(stopped.status, BoardStatus::Blocked);
    let cp = stopped.checkpoint.as_ref().unwrap();
    assert_eq!(cp.blocker, STOPPED_BLOCKER);
    assert_eq!(cp.goal, card.checkpoint.as_ref().unwrap().goal);
    assert_eq!(cp.attempt, 1);
    assert_eq!(
        log_texts(&card.id).last().map(String::as_str),
        Some("stopped")
    );

    // A second work turn in another chat settles Done with a final line.
    cabin.chat_job_thread = Some("t-work-2".into());
    cabin.note_inflight_card("write the release notes", "");
    let done_id = cabin.board.last().unwrap().id.clone();
    cabin.settle_turn_card("Wrote them.");
    let done = cabin.board.iter().find(|c| c.id == done_id).unwrap();
    assert_eq!(done.status, BoardStatus::Done);
    assert_eq!(log_texts(&done_id).last().map(String::as_str), Some("done"));
    cabin.chat_job_thread = None;
    release_isolated(&root, cabin);
}

#[test]
fn background_run_gets_its_own_card() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("kb02-bg");
    cabin.board.clear();
    cabin.open_bg_card(7, "Check the backups", "", false);
    let id = cabin.bg_card_id(7).expect("mapped");
    assert!(cabin.live_card_ids().contains(&id), "the sweep spares it");
    cabin.log_bg_tools(vec![
        (7, String::new(), tool("x", "shell", "completed")),
        (7, String::new(), tool("y", "shell", "in_progress")),
        (8, String::new(), tool("z", "shell", "completed")),
    ]);
    cabin.finish_bg_card(7, &RunEnd::Failed("exit 2".into()), false);
    assert!(cabin.bg_card_id(7).is_none());
    let c = cabin.board.iter().find(|c| c.id == id).unwrap();
    assert_eq!(c.kind, WorkKind::Background);
    assert_eq!(c.status, BoardStatus::Blocked);
    assert_eq!(c.checkpoint.as_ref().unwrap().blocker, "Failed: exit 2");
    // KB-03: a failed run is Interrupted, so it can resume from here.
    assert_eq!(
        c.run_state.interrupted.map(|i| i.cause),
        Some(grokhub_core::work_resume::InterruptCause::ProviderError)
    );
    assert_eq!(c.run_state.host, "cabin");
    assert_eq!(
        log_texts(&id),
        ["attempt 1", "shell ok", "interrupted: exit 2"]
    );
    release_isolated(&root, cabin);
}

#[test]
fn automation_reuses_one_card_across_runs() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("kb02-auto");
    cabin.board.clear();
    for run in 1..=2u64 {
        cabin.open_automation_card(run, "a-tidy", "Tidy downloads");
        let id = cabin.bg_card_id(run).unwrap();
        let c = cabin.board.iter().find(|c| c.id == id).unwrap();
        assert_eq!(c.status, BoardStatus::InProgress);
        assert_eq!(c.checkpoint.as_ref().unwrap().attempt as u64, run);
        // A scheduled card is the automation's: the run end leaves it alone.
        cabin.finish_bg_card(run, &RunEnd::Done, true);
        cabin.finish_automation_card("a-tidy", &RunEnd::Done, false);
    }
    let cards: Vec<_> = cabin
        .board
        .iter()
        .filter(|c| c.automation.as_deref() == Some("a-tidy"))
        .collect();
    assert_eq!(cards.len(), 1, "{:?}", cabin.board);
    assert_eq!(cards[0].kind, WorkKind::Automation);
    // A quiet chore that is the agent's own is archived once done.
    assert_eq!(cards[0].status, BoardStatus::Dismissed);
    // A failed run leaves it Blocked with why.
    cabin.open_automation_card(3, "a-tidy", "Tidy downloads");
    cabin.finish_bg_card(3, &RunEnd::Failed("timed out".into()), true);
    cabin.finish_automation_card("a-tidy", &RunEnd::Failed("timed out".into()), false);
    let c = cabin
        .board
        .iter()
        .find(|c| c.automation.as_deref() == Some("a-tidy"))
        .unwrap();
    assert_eq!(c.status, BoardStatus::Blocked);
    assert_eq!(c.checkpoint.as_ref().unwrap().attempt, 3);
    assert_eq!(c.checkpoint.as_ref().unwrap().blocker, "Failed: timed out");
    assert!(c.run_state.interrupted.is_some());
    release_isolated(&root, cabin);
}
