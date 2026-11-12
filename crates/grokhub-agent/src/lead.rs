//! Lead mode: a session a host marks as the lead plans first, then hands its
//! tasks to helpers. Only a host turns it on, per turn, with the turn's phase.
//! A session nobody marked reads none of this.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

/// Where a lead session's turn stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeadPhase {
    /// The turn plans: nothing changes until the plan is approved.
    Plan,
    /// The turn works from the task list.
    Work,
}

/// Opens every lead note.
pub(crate) const LEAD_HEAD: &str = "You are the lead for this session. This note is for the lead \
only. If you were started as a helper for one task, ignore it and do that task. As a helper, do \
not call enter_plan_mode or exit_plan_mode.";

/// The note of a turn that plans.
pub(crate) const LEAD_PLAN: &str = "Plan first. Nothing in the project changes until the person \
approves the plan.
1. Read what you need. Do wide reading through explore helpers (spawn_subagent, type explore, \
background true), collect their results with get_command_or_subagent_output, and keep only their \
summaries.
2. Show the plan with exit_plan_mode. Keep it short: what changes, where, and how it will be \
checked.
3. End the plan with a line \"Tasks\" and a numbered list under it. One line per task. Each task \
is small enough for one helper.
4. The person can edit the plan or ask for changes. If they ask for changes, revise the plan and \
call exit_plan_mode again.
If the message is only a question, answer it and leave the plan for later.";

/// The steps after an approval. Only the approved `exit_plan_mode` result
/// carries them, because only there is "the plan is approved" true.
pub(crate) const LEAD_WORK: &str = "The plan is approved. Follow the approved text.
1. Call todo_write with one row per task in the plan, in order: ids task-1, task-2, and so on, \
status pending. Skip this if the list already holds them.
2. Give each task to a helper: spawn_subagent, type general, description \"<task id>: <task>\", \
and a prompt with these lines: Goal:, Constraints:, Edge cases:, Done when:, Files:.
3. Set the task to in_progress before its helper starts. To change one task's status, call \
todo_write with merge true and that row only. Tasks that change different files can run \
together: give each helper isolation worktree and background true, at most three at once, then \
wait with get_command_or_subagent_output. Tasks that change the same files run one after \
another. A worktree helper's work is not in the project until it is merged: ask with \
merge_subagent_work, and wait for the person. A result that says partial, held or conflict is \
not done: tell the person. Work that conflicts can be redone: its result says how. If the folder \
is not a git repository, run one writing helper at a time.
4. Read each result. Check the change yourself before you set the task to completed. If a task \
failed and can't be done, set it to cancelled and say why.
5. Make only small edits yourself. If a helper is refused, do what the refusal says, and tell \
the person about any task that is left.
6. When every task is closed, report what changed, what was checked, and what is left.";

/// The note of a turn that does not plan. It claims no approval.
pub(crate) const LEAD_NEXT: &str = "Keep working from the task list and hand open tasks to \
helpers as before. For new work that is more than a small change, call enter_plan_mode and plan \
it first.";

/// Lead sessions remembered; a full map starts over.
const LEADS_CAP: usize = 256;

static TURN: AtomicU64 = AtomicU64::new(1);

/// Session id to its lead turn number and phase.
fn leads() -> &'static Mutex<HashMap<String, (u64, LeadPhase)>> {
    static MAP: OnceLock<Mutex<HashMap<String, (u64, LeadPhase)>>> = OnceLock::new();
    MAP.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Marks the session's next turn as a lead turn in this phase, or with `None`
/// as no lead turn. Each `Some` call starts a new lead turn with its own
/// helper allowance.
pub fn set_lead_session(session: &str, phase: Option<LeadPhase>) {
    if session.is_empty() {
        return;
    }
    let mut map = leads().lock().unwrap_or_else(|err| err.into_inner());
    match phase {
        Some(phase) => {
            if map.len() >= LEADS_CAP && !map.contains_key(session) {
                map.clear();
            }
            let turn = TURN.fetch_add(1, Ordering::Relaxed);
            map.insert(session.to_string(), (turn, phase));
        }
        None => {
            map.remove(session);
        }
    }
}

fn entry(session: &str) -> Option<(u64, LeadPhase)> {
    leads()
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .get(session)
        .copied()
}

pub fn lead_on(session: &str) -> bool {
    entry(session).is_some()
}

/// The host's count of the session's lead turn.
pub(crate) fn lead_turn(session: &str) -> Option<u64> {
    entry(session).map(|(turn, _)| turn)
}

/// The plan was approved inside the turn: the same lead turn now works.
pub(crate) fn plan_approved(session: &str) {
    let mut map = leads().lock().unwrap_or_else(|err| err.into_inner());
    if let Some((_, phase)) = map.get_mut(session) {
        *phase = LeadPhase::Work;
    }
}

/// The note a lead session's turn carries, by its phase.
pub(crate) fn turn_note(session: &str) -> Option<String> {
    let (_, phase) = entry(session)?;
    let body = match phase {
        LeadPhase::Plan => LEAD_PLAN,
        LeadPhase::Work => LEAD_NEXT,
    };
    Some(format!("{LEAD_HEAD}\n{body}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_session_leads_only_while_a_host_says_so() {
        let id = "lead-flag-test";
        assert!(!lead_on(id));
        assert_eq!(lead_turn(id), None);
        set_lead_session(id, Some(LeadPhase::Work));
        assert!(lead_on(id));
        set_lead_session(id, None);
        assert!(!lead_on(id));
        assert_eq!(lead_turn(id), None);
        set_lead_session("", Some(LeadPhase::Plan));
        assert!(!lead_on(""));
        assert_eq!(turn_note(""), None);
    }

    #[test]
    fn each_marked_turn_is_a_new_lead_turn() {
        let id = "lead-turn-test";
        set_lead_session(id, Some(LeadPhase::Plan));
        let first = lead_turn(id).expect("first turn");
        set_lead_session(id, Some(LeadPhase::Plan));
        let second = lead_turn(id).expect("second turn");
        assert!(second > first, "{first} then {second}");
        set_lead_session(id, None);
    }

    #[test]
    fn the_note_follows_the_phase_the_host_gave() {
        let id = "lead-note-test";
        assert_eq!(turn_note(id), None);
        set_lead_session(id, Some(LeadPhase::Plan));
        let plan = turn_note(id).expect("plan note");
        assert!(plan.starts_with("You are the lead for this session."));
        assert!(plan.contains("\nPlan first. Nothing in the project changes"));
        assert!(plan.contains("keep only their summaries"));
        assert!(!plan.contains("Keep working from the task list"));
        set_lead_session(id, Some(LeadPhase::Work));
        assert_eq!(
            turn_note(id).as_deref(),
            Some(
                "You are the lead for this session. This note is for the lead only. If you were \
                 started as a helper for one task, ignore it and do that task. As a helper, do \
                 not call enter_plan_mode or exit_plan_mode.\nKeep working from the task list \
                 and hand open tasks to helpers as before. For new work that is more than a \
                 small change, call enter_plan_mode and plan it first."
            )
        );
        set_lead_session(id, None);
        assert_eq!(turn_note(id), None);
    }

    #[test]
    fn an_approval_moves_the_same_turn_to_work() {
        let id = "lead-approve-test";
        set_lead_session(id, Some(LeadPhase::Plan));
        let turn = lead_turn(id);
        plan_approved(id);
        assert_eq!(lead_turn(id), turn);
        let note = turn_note(id).expect("work note");
        assert!(note.contains("Keep working from the task list"));
        assert!(!note.contains("Plan first."));
        set_lead_session(id, None);
        plan_approved(id);
        assert!(!lead_on(id));
    }

    #[test]
    fn the_notes_say_what_the_lead_must_not_get_wrong() {
        assert!(LEAD_HEAD.ends_with("As a helper, do not call enter_plan_mode or exit_plan_mode."));
        assert!(LEAD_WORK.starts_with("The plan is approved. Follow the approved text."));
        assert!(LEAD_WORK.contains(
            "To change one task's status, call todo_write with merge true and that row only."
        ));
        assert!(LEAD_WORK.contains("merge true"));
        assert!(!LEAD_WORK.contains("Run one writing helper at a time."));
        assert!(LEAD_WORK.contains(
            "3. Set the task to in_progress before its helper starts. To change one task's \
             status, call todo_write with merge true and that row only. Tasks that change \
             different files can run together: give each helper isolation worktree and \
             background true, at most three at once, then wait with \
             get_command_or_subagent_output. Tasks that change the same files run one after \
             another. A worktree helper's work is not in the project until it is merged: ask \
             with merge_subagent_work, and wait for the person. A result that says partial, \
             held or conflict is not done: tell the person. Work that conflicts can be \
             redone: its result says how. If the folder is not a git repository, run one \
             writing helper at a time.\n4. Read each result."
        ));
        assert!(!LEAD_NEXT.contains("approved"));
        assert!(!LEAD_PLAN.contains("approved"));
    }
}
