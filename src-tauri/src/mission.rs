//! Mission projection for the shell's Workboard (parity with the cabin's
//! `mission_ui.rs`). A card that is running, blocked or resumable shows as one
//! mission: its steps as a checklist, a status and a one-line header. It is a
//! read-only adapter over what the card already holds (steps, checkpoint,
//! the newest task outcome); there is no second run loop and no new page.

use std::path::Path;

use grokhub_core::outcome::TaskOutcome;
use grokhub_core::{BoardCard, BoardStatus, MissionSeed, MissionView, StepStatus};

/// The `todo_write` word for a step, which is what the projection speaks.
fn todo_word(status: StepStatus) -> &'static str {
    match status {
        StepStatus::Planned => "pending",
        StepStatus::Doing => "in_progress",
        StepStatus::Done => "completed",
        StepStatus::Skipped => "cancelled",
        StepStatus::Failed => "pending",
    }
}

/// The outcomes log for one board page: read once, and only when a card
/// needs it (a running or blocked card that has a chat).
pub struct Outcomes<'a> {
    config_dir: &'a Path,
    read: std::cell::OnceCell<Vec<TaskOutcome>>,
}

impl<'a> Outcomes<'a> {
    pub fn new(config_dir: &'a Path) -> Self {
        Self {
            config_dir,
            read: std::cell::OnceCell::new(),
        }
    }

    fn all(&self) -> &[TaskOutcome] {
        self.read
            .get_or_init(|| grokhub_core::outcome::read_outcomes(self.config_dir))
    }
}

/// What the card gives the projection, with the newest outcome for its chat.
pub fn seed(card: &BoardCard, outcomes: &Outcomes) -> MissionSeed {
    match card.thread_id {
        Some(_) => seed_with(card, outcomes.all()),
        None => seed_with(card, &[]),
    }
}

/// [`seed`] over outcomes already read (one per task, oldest first).
fn seed_with(card: &BoardCard, outcomes: &[TaskOutcome]) -> MissionSeed {
    let outcome = card.thread_id.as_deref().and_then(|chat| {
        let turns = format!("{chat}:");
        outcomes
            .iter()
            .rev()
            .find(|o| o.task == chat || o.task.starts_with(&turns))
    });
    let (outcome_result, outcome_reasons, outcome_span_ids) = match outcome {
        Some(o) => (
            match o.result {
                grokhub_core::outcome::OutcomeResult::Success => "success",
                grokhub_core::outcome::OutcomeResult::Failure => "failure",
                grokhub_core::outcome::OutcomeResult::Pending => "pending",
            }
            .to_string(),
            o.reasons.clone(),
            o.span_ids.clone(),
        ),
        None => (String::new(), Vec::new(), Vec::new()),
    };
    let steps_done = card
        .steps
        .iter()
        .filter(|s| matches!(s.status, StepStatus::Done | StepStatus::Skipped))
        .count();
    let checkpoint = card.checkpoint.as_ref().filter(|k| !k.is_empty());
    MissionSeed {
        id: card.id.clone(),
        title: card.title.clone(),
        goal: checkpoint
            .map(|k| k.goal.clone())
            .filter(|g| !g.trim().is_empty())
            .unwrap_or_else(|| card.title.clone()),
        todos: card
            .steps
            .iter()
            .enumerate()
            .map(|(i, s)| {
                (
                    i.to_string(),
                    s.text.clone(),
                    todo_word(s.status).to_string(),
                )
            })
            .collect(),
        todos_verified: outcome_reasons.iter().any(|r| r == "verify_ok")
            || outcome_result == "success",
        activity: if card.steps.is_empty() {
            String::new()
        } else {
            format!("{steps_done} of {} steps done", card.steps.len())
        },
        running: card.status == BoardStatus::InProgress,
        resume_ok: card.status == BoardStatus::Blocked && (card.run || checkpoint.is_some()),
        outcome_result,
        outcome_reasons,
        outcome_span_ids,
        next_action: checkpoint.map(|k| k.next.clone()).unwrap_or_default(),
        ..MissionSeed::default()
    }
}

/// The mission a card shows, if it is one: a card on the move (Doing or
/// Blocked), not one waiting, finished or archived.
pub fn project(card: &BoardCard, outcomes: &Outcomes) -> Option<MissionView> {
    if !matches!(card.status, BoardStatus::InProgress | BoardStatus::Blocked) {
        return None;
    }
    MissionView::project(&seed(card, outcomes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_core::outcome::{append_outcome, outcomes_path, OutcomeResult};
    use grokhub_core::{Checkpoint, MissionStatus, WorkStep};

    /// A test's scratch folder, removed when the test ends, pass or fail.
    struct Scratch(std::path::PathBuf);

    impl std::ops::Deref for Scratch {
        type Target = Path;
        fn deref(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn temp_dir(name: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!(
            "grokhub-shell-mission-{name}-{}-{}",
            std::process::id(),
            grokhub_core::now_ms()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        Scratch(dir)
    }

    fn outcome(task: &str, result: OutcomeResult, reason: &str, finished_at: u64) -> TaskOutcome {
        TaskOutcome {
            task: task.into(),
            signature: "sig".into(),
            result,
            reasons: vec![reason.into()],
            finished_at,
            at: finished_at,
            ..TaskOutcome::default()
        }
    }

    fn step(text: &str, status: StepStatus) -> WorkStep {
        WorkStep {
            text: text.into(),
            status,
        }
    }

    fn running_card() -> BoardCard {
        let mut c = BoardCard::new("m1", "", "");
        c.id = "m1".into();
        c.title = "Fix the printer".into();
        c.status = BoardStatus::InProgress;
        c.thread_id = Some("chat-m1".into());
        c.steps = vec![
            step("Find the driver", StepStatus::Done),
            step("Install it", StepStatus::Doing),
            step("Print a test page", StepStatus::Planned),
        ];
        c
    }

    #[test]
    fn a_running_card_projects_its_steps_as_a_checklist() {
        let dir = temp_dir("running");
        let m = project(&running_card(), &Outcomes::new(&dir)).expect("a mission");
        assert_eq!(m.id, "m1");
        assert_eq!(m.title, "Fix the printer");
        assert_eq!(m.status, MissionStatus::Active);
        assert_eq!(m.current_step, "Install it");
        assert_eq!(m.activity, "1 of 3 steps done");
        let marks: Vec<(&str, &str)> = m
            .checklist
            .iter()
            .map(|c| (c.text.as_str(), c.status.as_str()))
            .collect();
        assert_eq!(
            marks,
            [
                ("Find the driver", "unverified"),
                ("Install it", "attempted"),
                ("Print a test page", "planned"),
            ]
        );
        assert_eq!(m.header_line(), "Mission · Fix the printer · active");
    }

    #[test]
    fn a_blocked_card_with_a_checkpoint_can_resume() {
        let dir = temp_dir("blocked");
        let mut c = running_card();
        c.status = BoardStatus::Blocked;
        c.run = true;
        c.checkpoint = Some(Checkpoint {
            goal: "Get printing working".into(),
            next: "Install it".into(),
            blocker: "Needs a password".into(),
            ..Checkpoint::default()
        });
        let m = project(&c, &Outcomes::new(&dir)).expect("a mission");
        assert_eq!(m.goal, "Get printing working");
        assert!(m.resume_ok);
        assert_eq!(m.status, MissionStatus::Paused);
    }

    #[test]
    fn waiting_finished_and_archived_cards_are_not_missions() {
        let dir = temp_dir("not");
        for status in [
            BoardStatus::Todo,
            BoardStatus::Done,
            BoardStatus::Dismissed,
            BoardStatus::FollowUp,
        ] {
            let mut c = running_card();
            c.status = status;
            assert_eq!(project(&c, &Outcomes::new(&dir)), None, "{status:?}");
        }
    }

    #[test]
    fn each_card_takes_the_newest_outcome_of_its_own_chat_from_one_read() {
        let outcomes = [
            outcome("chat-m1:1", OutcomeResult::Failure, "verify_failed", 10),
            outcome("chat-m2:1", OutcomeResult::Success, "verify_ok", 20),
            outcome("chat-m1:2", OutcomeResult::Pending, "waiting", 30),
            outcome("chat-m10:1", OutcomeResult::Failure, "not_mine", 40),
        ];
        let mut one = running_card();
        one.status = BoardStatus::Blocked;
        let mut two = one.clone();
        two.id = "m2".into();
        two.thread_id = Some("chat-m2".into());
        let first = seed_with(&one, &outcomes);
        assert_eq!(first.outcome_result, "pending");
        assert_eq!(first.outcome_reasons, ["waiting"]);
        assert!(!first.todos_verified);
        let second = seed_with(&two, &outcomes);
        assert_eq!(second.outcome_result, "success");
        assert_eq!(second.outcome_reasons, ["verify_ok"]);
        assert!(second.todos_verified);
        let mut no_chat = one.clone();
        no_chat.thread_id = None;
        assert_eq!(seed_with(&no_chat, &outcomes).outcome_result, "");
    }

    #[test]
    fn a_board_page_reads_the_outcomes_log_once_and_only_for_a_card_with_a_chat() {
        let dir = temp_dir("once");
        let outcomes = Outcomes::new(&dir);
        // A card with no chat needs no outcome, so nothing is read for it:
        // the lines written after it are the ones the page sees.
        let mut no_chat = running_card();
        no_chat.thread_id = None;
        assert_eq!(seed(&no_chat, &outcomes).outcome_result, "");
        append_outcome(
            &dir,
            &outcome("chat-m1:1", OutcomeResult::Failure, "verify_failed", 10),
        )
        .expect("append");
        append_outcome(
            &dir,
            &outcome("chat-m2:1", OutcomeResult::Success, "verify_ok", 20),
        )
        .expect("append");
        assert_eq!(seed(&running_card(), &outcomes).outcome_result, "failure");
        // The log goes away under the page: the next card still has the one read.
        std::fs::remove_file(outcomes_path(&dir)).expect("remove");
        let mut two = running_card();
        two.thread_id = Some("chat-m2".into());
        assert_eq!(seed(&two, &outcomes).outcome_result, "success");
        // A new page reads again.
        assert_eq!(seed(&two, &Outcomes::new(&dir)).outcome_result, "");
    }

    #[test]
    fn the_projection_shows_up_in_the_workboard_json() {
        let dir = temp_dir("json");
        let mut waiting = running_card();
        waiting.id = "w1".into();
        waiting.status = BoardStatus::Todo;
        grokhub_core::work_file::write_board(
            &crate::workboard::path(&dir),
            &[running_card(), waiting],
        )
        .expect("write");
        let page = serde_json::to_value(crate::workboard::load(&dir).expect("load")).expect("json");
        let cards: Vec<&serde_json::Value> = page["columns"]
            .as_array()
            .expect("columns")
            .iter()
            .flat_map(|c| c["cards"].as_array().expect("cards"))
            .collect();
        let find = |id: &str| cards.iter().find(|c| c["id"] == id).copied().expect(id);
        let mission = &find("m1")["mission"];
        assert_eq!(mission["title"], "Fix the printer");
        assert_eq!(mission["status"], "active");
        assert_eq!(mission["checklist"].as_array().expect("list").len(), 3);
        assert!(find("w1")["mission"].is_null());
    }
}
