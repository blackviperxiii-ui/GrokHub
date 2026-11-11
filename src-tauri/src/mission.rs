//! Mission projection for the shell's Workboard (parity with the cabin's
//! `mission_ui.rs`). A card that is running, blocked or resumable shows as one
//! mission: its steps as a checklist, a status and a one-line header. It is a
//! read-only adapter over what the card already holds (steps, checkpoint,
//! the newest task outcome); there is no second run loop and no new page.

use std::path::Path;

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

/// What the card gives the projection, with the newest outcome for its chat.
pub fn seed(config_dir: &Path, card: &BoardCard) -> MissionSeed {
    let outcome = card.thread_id.as_deref().and_then(|chat| {
        grokhub_core::outcome::read_outcomes(config_dir)
            .into_iter()
            .rev()
            .find(|o| o.task == chat || o.task.starts_with(&format!("{chat}:")))
    });
    let (outcome_result, outcome_reasons, outcome_span_ids) = match outcome {
        Some(o) => (
            match o.result {
                grokhub_core::outcome::OutcomeResult::Success => "success",
                grokhub_core::outcome::OutcomeResult::Failure => "failure",
                grokhub_core::outcome::OutcomeResult::Pending => "pending",
            }
            .to_string(),
            o.reasons,
            o.span_ids,
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
            .map(|(i, s)| (i.to_string(), s.text.clone(), todo_word(s.status).to_string()))
            .collect(),
        todos_verified: outcome_reasons.iter().any(|r| r == "verify_ok")
            || outcome_result == "success",
        activity: if card.steps.is_empty() {
            String::new()
        } else {
            format!("{steps_done} of {} steps done", card.steps.len())
        },
        running: card.status == BoardStatus::InProgress,
        resume_ok: card.status == BoardStatus::Blocked
            && (card.run || checkpoint.is_some()),
        outcome_result,
        outcome_reasons,
        outcome_span_ids,
        next_action: checkpoint.map(|k| k.next.clone()).unwrap_or_default(),
        ..MissionSeed::default()
    }
}

/// The mission a card shows, if it is one: a card on the move (Doing or
/// Blocked), not one waiting, finished or archived.
pub fn project(config_dir: &Path, card: &BoardCard) -> Option<MissionView> {
    if !matches!(card.status, BoardStatus::InProgress | BoardStatus::Blocked) {
        return None;
    }
    MissionView::project(&seed(config_dir, card))
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_core::{Checkpoint, MissionStatus, WorkStep};

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "grokhub-shell-mission-{name}-{}-{}",
            std::process::id(),
            grokhub_core::now_ms()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
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
        let m = project(&dir, &running_card()).expect("a mission");
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
        assert_eq!(
            m.header_line(),
            "Mission · Fix the printer · active"
        );
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
        let m = project(&dir, &c).expect("a mission");
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
            assert_eq!(project(&dir, &c), None, "{status:?}");
        }
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
