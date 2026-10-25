//! Foreground mission read model (mission-based OS v1).
//!
//! Pure projection over Brief / todos / episode / parks / outcomes. Not a
//! second run loop. One active foreground mission at a time.

use serde::{Deserialize, Serialize};

/// How far a checklist row has gotten.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MissionCheckStatus {
    Planned,
    Attempted,
    Verified,
    Unverified,
    Cancelled,
}

impl MissionCheckStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Planned => "planned",
            Self::Attempted => "attempted",
            Self::Verified => "verified",
            Self::Unverified => "unverified",
            Self::Cancelled => "cancelled",
        }
    }

    /// Map a `todo_write` status (and optional verify flag) into mission terms.
    pub fn from_todo(status: &str, verified: bool) -> Self {
        match status.trim().to_ascii_lowercase().as_str() {
            "in_progress" => Self::Attempted,
            "completed" if verified => Self::Verified,
            "completed" => Self::Unverified,
            "cancelled" => Self::Cancelled,
            _ => Self::Planned,
        }
    }
}

/// Lifecycle of the one foreground mission.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MissionStatus {
    Active,
    NeedsApproval,
    Paused,
    Blocked,
    Done,
}

impl MissionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::NeedsApproval => "needs_approval",
            Self::Paused => "paused",
            Self::Blocked => "blocked",
            Self::Done => "done",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MissionCheckItem {
    pub id: String,
    pub text: String,
    pub status: MissionCheckStatus,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MissionOutcomeView {
    /// `success`, `failure`, or `pending`.
    pub result: String,
    pub reasons: Vec<String>,
    pub span_ids: Vec<String>,
    pub completed: String,
    pub uncertainty: String,
    pub next_action: String,
}

/// One user-visible mission projected from existing run state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MissionView {
    pub id: String,
    pub title: String,
    pub goal: String,
    pub intent: String,
    pub care: String,
    pub checklist: Vec<MissionCheckItem>,
    pub current_step: String,
    pub approval: Option<String>,
    pub activity: String,
    pub status: MissionStatus,
    pub outcome: Option<MissionOutcomeView>,
    pub resume_ok: bool,
}

/// Inputs the cabin already has; keep this free of agent/UI types.
#[derive(Debug, Clone, Default)]
pub struct MissionSeed {
    pub id: String,
    pub title: String,
    pub goal: String,
    pub intent: String,
    pub care: String,
    /// `(id, text, todo_status)` — status is pending|in_progress|completed|cancelled.
    pub todos: Vec<(String, String, String)>,
    /// When true, completed todos count as verified.
    pub todos_verified: bool,
    pub current_step: String,
    pub approval: Option<String>,
    pub activity: String,
    pub running: bool,
    pub resume_ok: bool,
    pub soft_paused: bool,
    /// From `parse_goal_outcome`: `complete`, `blocked`, or empty.
    pub goal_outcome: String,
    /// Latest TaskOutcome result word: success|failure|pending, or empty.
    pub outcome_result: String,
    pub outcome_reasons: Vec<String>,
    pub outcome_span_ids: Vec<String>,
    pub uncertainty: String,
    pub next_action: String,
    /// The Journal question when this chat is a decision's mission
    /// (`journal::promote_to_mission`); empty otherwise.
    pub journal_question: String,
}

/// Intents that promote a chat turn into a foreground mission.
pub fn mission_worthy_intent(intent: &str) -> bool {
    matches!(
        intent.trim().to_ascii_lowercase().as_str(),
        "task" | "repair" | "research"
    )
}

impl MissionView {
    /// Project a mission from seed signals. Returns `None` when there is
    /// nothing mission-shaped (casual chat with no episode/todos/parks).
    pub fn project(seed: &MissionSeed) -> Option<Self> {
        let has_work = !seed.todos.is_empty()
            || seed.approval.is_some()
            || seed.resume_ok
            || seed.soft_paused
            || seed.running
            || !seed.goal_outcome.is_empty()
            || !seed.outcome_result.is_empty()
            || (!seed.id.is_empty() && seed.id.starts_with("ep-"))
            || mission_worthy_intent(&seed.intent)
            || !seed.journal_question.is_empty();
        if !has_work {
            return None;
        }

        let checklist: Vec<MissionCheckItem> = seed
            .todos
            .iter()
            .map(|(id, text, status)| MissionCheckItem {
                id: id.clone(),
                text: text.clone(),
                status: MissionCheckStatus::from_todo(status, seed.todos_verified),
            })
            .collect();

        let current_step = if !seed.current_step.is_empty() {
            seed.current_step.clone()
        } else {
            checklist
                .iter()
                .find(|c| c.status == MissionCheckStatus::Attempted)
                .map(|c| c.text.clone())
                .unwrap_or_default()
        };

        let status = decide_status(seed, &checklist);
        let outcome = if matches!(status, MissionStatus::Done | MissionStatus::Blocked)
            || !seed.outcome_result.is_empty()
        {
            Some(MissionOutcomeView {
                result: if seed.outcome_result.is_empty() {
                    match status {
                        MissionStatus::Done => "success".into(),
                        MissionStatus::Blocked => "failure".into(),
                        _ => "pending".into(),
                    }
                } else {
                    seed.outcome_result.clone()
                },
                reasons: seed.outcome_reasons.clone(),
                span_ids: seed.outcome_span_ids.clone(),
                completed: completed_summary(&checklist),
                uncertainty: seed.uncertainty.clone(),
                next_action: if seed.next_action.is_empty() {
                    default_next_action(status, seed.resume_ok)
                } else {
                    seed.next_action.clone()
                },
            })
        } else {
            None
        };

        let title = if seed.title.is_empty() {
            fallback_title(seed)
        } else {
            seed.title.clone()
        };
        let goal = if seed.goal.is_empty() {
            title.clone()
        } else {
            seed.goal.clone()
        };

        Some(Self {
            id: if seed.id.is_empty() {
                "mission".into()
            } else {
                seed.id.clone()
            },
            title,
            goal,
            intent: seed.intent.clone(),
            care: seed.care.clone(),
            checklist,
            current_step,
            approval: seed.approval.clone(),
            activity: seed.activity.clone(),
            status,
            outcome,
            resume_ok: seed.resume_ok,
        })
    }

    /// One-line header for the Work tree.
    pub fn header_line(&self) -> String {
        let mode = if self.intent.is_empty() && self.care.is_empty() {
            String::new()
        } else if self.care.is_empty() {
            format!(" · {}", self.intent)
        } else if self.intent.is_empty() {
            format!(" · {}", self.care)
        } else {
            format!(" · {} × {}", self.intent, self.care)
        };
        format!("Mission · {} · {}{mode}", self.title, self.status.as_str())
    }
}

fn fallback_title(seed: &MissionSeed) -> String {
    if mission_worthy_intent(&seed.intent) {
        format!("{} mission", seed.intent)
    } else if seed.id.starts_with("ep-") {
        "Desktop mission".into()
    } else {
        "Mission".into()
    }
}

fn completed_summary(checklist: &[MissionCheckItem]) -> String {
    let done: Vec<&str> = checklist
        .iter()
        .filter(|c| {
            matches!(
                c.status,
                MissionCheckStatus::Verified | MissionCheckStatus::Unverified
            )
        })
        .map(|c| c.text.as_str())
        .collect();
    if done.is_empty() {
        String::new()
    } else {
        done.join(", ")
    }
}

fn default_next_action(status: MissionStatus, resume_ok: bool) -> String {
    match status {
        MissionStatus::NeedsApproval => "Approve or deny the waiting step".into(),
        MissionStatus::Paused if resume_ok => "Continue to resume".into(),
        MissionStatus::Paused => "Resume when ready".into(),
        MissionStatus::Blocked => "Unblock or replan".into(),
        MissionStatus::Done => "Review outcome".into(),
        MissionStatus::Active => "Continue the current step".into(),
    }
}

fn open_checklist(checklist: &[MissionCheckItem]) -> bool {
    checklist.iter().any(|c| {
        matches!(
            c.status,
            MissionCheckStatus::Planned | MissionCheckStatus::Attempted
        )
    })
}

fn failed_checklist(checklist: &[MissionCheckItem]) -> bool {
    checklist
        .iter()
        .any(|c| c.status == MissionCheckStatus::Cancelled)
}

fn decide_status(seed: &MissionSeed, checklist: &[MissionCheckItem]) -> MissionStatus {
    if seed.approval.is_some() {
        return MissionStatus::NeedsApproval;
    }
    if seed.goal_outcome.eq_ignore_ascii_case("blocked") {
        return MissionStatus::Blocked;
    }
    if seed.outcome_result.eq_ignore_ascii_case("failure") {
        return MissionStatus::Blocked;
    }
    // A failed/cancelled step must never look done.
    if failed_checklist(checklist) && open_checklist(checklist) {
        return MissionStatus::Active;
    }
    if failed_checklist(checklist) && !seed.goal_outcome.eq_ignore_ascii_case("complete") {
        return MissionStatus::Blocked;
    }
    let goal_done = seed.goal_outcome.eq_ignore_ascii_case("complete");
    let outcome_ok = seed.outcome_result.eq_ignore_ascii_case("success");
    let all_closed = !checklist.is_empty()
        && checklist.iter().all(|c| {
            matches!(
                c.status,
                MissionCheckStatus::Verified
                    | MissionCheckStatus::Unverified
                    | MissionCheckStatus::Cancelled
            )
        });
    let any_verified = checklist
        .iter()
        .any(|c| c.status == MissionCheckStatus::Verified);
    if (goal_done || outcome_ok || (all_closed && any_verified && !failed_checklist(checklist)))
        && !open_checklist(checklist)
        && !seed.running
        && !seed.soft_paused
        && !seed.resume_ok
    {
        return MissionStatus::Done;
    }
    if seed.soft_paused || seed.resume_ok {
        return MissionStatus::Paused;
    }
    MissionStatus::Active
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed_task() -> MissionSeed {
        MissionSeed {
            id: "chat:1".into(),
            title: "Diagnose this machine".into(),
            goal: "Find why boot is slow".into(),
            intent: "repair".into(),
            care: "critical".into(),
            todos: vec![
                ("1".into(), "Check services".into(), "completed".into()),
                ("2".into(), "Check packages".into(), "in_progress".into()),
                ("3".into(), "Check boot errors".into(), "pending".into()),
            ],
            todos_verified: true,
            activity: "3 tools · 2 min".into(),
            running: true,
            ..MissionSeed::default()
        }
    }

    #[test]
    fn active_mission_projects_checklist_and_header() {
        let m = MissionView::project(&seed_task()).expect("mission");
        assert_eq!(m.status, MissionStatus::Active);
        assert_eq!(m.checklist[0].status, MissionCheckStatus::Verified);
        assert_eq!(m.checklist[1].status, MissionCheckStatus::Attempted);
        assert_eq!(m.checklist[2].status, MissionCheckStatus::Planned);
        assert_eq!(m.current_step, "Check packages");
        assert_eq!(
            m.header_line(),
            "Mission · Diagnose this machine · active · repair × critical"
        );
    }

    #[test]
    fn needs_approval_beats_running() {
        let mut s = seed_task();
        s.approval = Some("sudo pacman -Syu".into());
        let m = MissionView::project(&s).expect("mission");
        assert_eq!(m.status, MissionStatus::NeedsApproval);
        assert_eq!(m.approval.as_deref(), Some("sudo pacman -Syu"));
    }

    #[test]
    fn paused_resume_without_duplicating_completed() {
        let mut s = seed_task();
        s.running = false;
        s.resume_ok = true;
        s.todos[1].2 = "completed".into();
        let m = MissionView::project(&s).expect("mission");
        assert_eq!(m.status, MissionStatus::Paused);
        assert!(m.resume_ok);
        assert_eq!(m.checklist[0].status, MissionCheckStatus::Verified);
        assert_eq!(m.checklist[1].status, MissionCheckStatus::Verified);
        assert_eq!(m.checklist[2].status, MissionCheckStatus::Planned);
    }

    #[test]
    fn failed_step_does_not_mark_mission_complete() {
        let s = MissionSeed {
            id: "chat:2".into(),
            title: "Ship feature".into(),
            intent: "task".into(),
            care: "careful".into(),
            todos: vec![
                ("1".into(), "Write tests".into(), "completed".into()),
                ("2".into(), "Land PR".into(), "cancelled".into()),
            ],
            todos_verified: true,
            goal_outcome: String::new(),
            outcome_result: String::new(),
            ..MissionSeed::default()
        };
        let m = MissionView::project(&s).expect("mission");
        assert_eq!(m.status, MissionStatus::Blocked);
        assert_ne!(m.status, MissionStatus::Done);
    }

    #[test]
    fn done_with_outcome_evidence() {
        let s = MissionSeed {
            id: "ep-1".into(),
            title: "Research decision".into(),
            intent: "research".into(),
            care: "careful".into(),
            todos: vec![
                ("1".into(), "Gather sources".into(), "completed".into()),
                ("2".into(), "Summarize".into(), "completed".into()),
            ],
            todos_verified: true,
            goal_outcome: "complete".into(),
            outcome_result: "success".into(),
            outcome_reasons: vec!["verify_ok".into()],
            outcome_span_ids: vec!["s1".into(), "s2".into()],
            uncertainty: "Sources agree on A; B unconfirmed".into(),
            next_action: "Share the summary".into(),
            ..MissionSeed::default()
        };
        let m = MissionView::project(&s).expect("mission");
        assert_eq!(m.status, MissionStatus::Done);
        let o = m.outcome.expect("outcome");
        assert_eq!(o.result, "success");
        assert_eq!(o.reasons, vec!["verify_ok"]);
        assert_eq!(o.span_ids, vec!["s1", "s2"]);
        assert_eq!(o.completed, "Gather sources, Summarize");
        assert_eq!(o.uncertainty, "Sources agree on A; B unconfirmed");
        assert_eq!(o.next_action, "Share the summary");
    }

    #[test]
    fn casual_chat_without_work_is_not_a_mission() {
        let s = MissionSeed {
            intent: "chat".into(),
            care: "casual".into(),
            ..MissionSeed::default()
        };
        assert!(MissionView::project(&s).is_none());
    }

    #[test]
    fn unfinished_completed_todos_without_verify_stay_unverified() {
        let item = MissionCheckStatus::from_todo("completed", false);
        assert_eq!(item, MissionCheckStatus::Unverified);
        assert_eq!(
            MissionCheckStatus::from_todo("pending", true).as_str(),
            "planned"
        );
    }

    #[test]
    fn mission_worthy_intents() {
        assert!(mission_worthy_intent("task"));
        assert!(mission_worthy_intent("Repair"));
        assert!(mission_worthy_intent("research"));
        assert!(!mission_worthy_intent("chat"));
        assert!(!mission_worthy_intent("question"));
    }

    #[test]
    fn project_is_deterministic_for_reopen() {
        let s = seed_task();
        let a = MissionView::project(&s).expect("a");
        let b = MissionView::project(&s).expect("b");
        assert_eq!(a, b);
    }
    #[test]
    fn a_journal_promotion_is_a_mission_before_any_turn() {
        let s = MissionSeed {
            id: "thr-1:0".into(),
            title: "Switch the cabin to Tauri?".into(),
            goal: "Switch the cabin to Tauri?".into(),
            journal_question: "Switch the cabin to Tauri?".into(),
            ..MissionSeed::default()
        };
        let m = MissionView::project(&s).expect("journal mission");
        assert_eq!(m.title, "Switch the cabin to Tauri?");
        assert_eq!(m.status, MissionStatus::Active);
        assert!(m.checklist.is_empty());
        let casual = MissionSeed {
            title: "Switch the cabin to Tauri?".into(),
            ..MissionSeed::default()
        };
        assert_eq!(
            MissionView::project(&casual),
            None,
            "a title alone is not a mission"
        );
    }
}
