//! Recovery-first projection: one visible state over the harness ladder,
//! parks, drive gaps and input waits. This does not retry, escalate or park
//! anything on its own — those stay in `harness::ladder`, `drive` and the
//! approval path. Callers pass already-known operational facts; the
//! projection returns a calm state, reason and next action for UI and
//! evidence.

use crate::harness::Rung;

/// Visible recovery state for the active mission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryState {
    /// Current step is active.
    Running,
    /// Same approach, bounded retry.
    Retrying,
    /// Approach failed; a different next step is being selected.
    Replanning,
    /// Hard-class action waiting on the user.
    NeedsApproval,
    /// Missing credential, device, file or decision.
    NeedsInput,
    /// User or system paused the mission.
    Paused,
    /// Safe approaches exhausted or access unavailable.
    Blocked,
    /// Work resumed after a failure or wait.
    Recovered,
    /// Primary result usable; named checks remain unverified.
    CompletedGaps,
}

impl RecoveryState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "Running",
            Self::Retrying => "Retrying",
            Self::Replanning => "Replanning",
            Self::NeedsApproval => "Needs approval",
            Self::NeedsInput => "Needs input",
            Self::Paused => "Paused",
            Self::Blocked => "Blocked",
            Self::Recovered => "Recovered",
            Self::CompletedGaps => "Completed with gaps",
        }
    }
}

/// What the completion drive last said, if anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriveFact {
    /// Nothing open; the run may end.
    End,
    /// Keep going with a re-plan note.
    Push,
    /// Stalled out with named leftovers.
    Blocked,
}

/// Already-computed facts from the ladder, parks, drive and waits.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RecoveryFacts {
    /// Latest ladder rung for this turn, when the ladder stepped.
    pub ladder: Option<Rung>,
    /// One-line ladder / detector reason, when known.
    pub ladder_reason: Option<String>,
    /// Ladder repair prompt (retry / backtrack / replan), when known.
    pub ladder_next: Option<String>,
    /// An unanswered hard-class park is open.
    pub hard_park_open: bool,
    /// Soft ladder pause / SoftPark is open.
    pub soft_park_open: bool,
    /// MCP elicit, ask_user_question or similar wait is open.
    pub needs_input: bool,
    /// What `Drive::check` / `check_with` returned for this stop.
    pub drive: Option<DriveFact>,
    /// Drive blocked line, when `drive` is [`DriveFact::Blocked`].
    pub blocked_reason: Option<String>,
    /// Brain-2 gaps or open checklist items remain.
    pub gaps_open: bool,
    /// The run just resumed after a park, pause or approval.
    pub resumed_after_wait: bool,
    /// The run ended with named unverified leftovers (not a quiet success).
    pub completed_with_gaps: bool,
    /// A step is actively running.
    pub running: bool,
}

/// Projected state plus the operational reason and next action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoverySnapshot {
    pub state: RecoveryState,
    /// One-sentence operational reason (no hidden model reasoning).
    pub reason: String,
    /// What GrokHub will try next, or what it needs from the user.
    pub next: String,
}

impl RecoverySnapshot {
    /// Calm status line for the cabin chrome.
    pub fn status_line(&self) -> String {
        if self.reason.is_empty() {
            return self.state.as_str().into();
        }
        if self.next.is_empty() {
            return format!("{} — {}", self.state.as_str(), self.reason);
        }
        format!(
            "{} — {}. Next: {}",
            self.state.as_str(),
            self.reason,
            self.next
        )
    }
}

/// Map a ladder rung onto the recovery states that mean "still working".
pub fn state_for_rung(rung: Rung) -> RecoveryState {
    match rung {
        Rung::Retry => RecoveryState::Retrying,
        // Backtrack re-observes then picks another target — a new approach.
        Rung::Backtrack | Rung::Replan => RecoveryState::Replanning,
        Rung::Pause => RecoveryState::Paused,
    }
}

/// Project operational facts into one recovery snapshot.
///
/// Precedence (highest first): Needs approval → Needs input → Paused →
/// Blocked → Completed with gaps → Recovered → Retrying / Replanning →
/// Running (or Replanning when the drive is still pushing on gaps).
pub fn project(facts: &RecoveryFacts) -> RecoverySnapshot {
    if facts.hard_park_open {
        return snap(
            RecoveryState::NeedsApproval,
            facts
                .ladder_reason
                .as_deref()
                .unwrap_or("A hard-class action is waiting for your decision."),
            facts
                .ladder_next
                .as_deref()
                .unwrap_or("Approve or deny the card; recovery will not bypass it."),
        );
    }
    if facts.needs_input {
        return snap(
            RecoveryState::NeedsInput,
            facts
                .ladder_reason
                .as_deref()
                .unwrap_or("A credential, device, file or decision is missing."),
            facts
                .ladder_next
                .as_deref()
                .unwrap_or("Answer the ask, then the run continues from here."),
        );
    }
    if facts.soft_park_open || facts.ladder == Some(Rung::Pause) {
        return snap(
            RecoveryState::Paused,
            facts
                .ladder_reason
                .as_deref()
                .unwrap_or("The run paused and needs you."),
            facts
                .ladder_next
                .as_deref()
                .unwrap_or("Reply, Approve or Deny the pause card to continue."),
        );
    }
    if facts.drive == Some(DriveFact::Blocked) {
        return snap(
            RecoveryState::Blocked,
            facts
                .blocked_reason
                .as_deref()
                .or(facts.ladder_reason.as_deref())
                .unwrap_or("Safe approaches are exhausted or access is unavailable."),
            "Park only the blocked step when you can, or change the approach.",
        );
    }
    if facts.completed_with_gaps {
        return snap(
            RecoveryState::CompletedGaps,
            facts
                .ladder_reason
                .as_deref()
                .unwrap_or("The main result is usable, but some checks stay unverified."),
            "Review the named gaps before treating the work as done.",
        );
    }
    if facts.resumed_after_wait {
        return snap(
            RecoveryState::Recovered,
            facts
                .ladder_reason
                .as_deref()
                .unwrap_or("Work resumed after a failure or wait."),
            facts
                .ladder_next
                .as_deref()
                .unwrap_or("Continue from the last verified checkpoint."),
        );
    }
    if let Some(rung) = facts.ladder {
        match rung {
            Rung::Retry => {
                return snap(
                    RecoveryState::Retrying,
                    facts
                        .ladder_reason
                        .as_deref()
                        .unwrap_or("Retrying the same approach once."),
                    facts
                        .ladder_next
                        .as_deref()
                        .unwrap_or("One bounded retry, then a different approach."),
                );
            }
            Rung::Backtrack | Rung::Replan => {
                return snap(
                    RecoveryState::Replanning,
                    facts
                        .ladder_reason
                        .as_deref()
                        .unwrap_or("The last approach made no progress."),
                    facts
                        .ladder_next
                        .as_deref()
                        .unwrap_or("Pick a different next step."),
                );
            }
            Rung::Pause => {}
        }
    }
    if facts.drive == Some(DriveFact::Push) || (facts.gaps_open && facts.running) {
        return snap(
            RecoveryState::Replanning,
            facts
                .ladder_reason
                .as_deref()
                .unwrap_or("Open checks remain; the run is not done."),
            facts
                .ladder_next
                .as_deref()
                .unwrap_or("Cover the named items or mark truly blocked ones cancelled."),
        );
    }
    if facts.running {
        return snap(
            RecoveryState::Running,
            facts
                .ladder_reason
                .as_deref()
                .unwrap_or("The current step is active."),
            facts.ladder_next.as_deref().unwrap_or("Keep going."),
        );
    }
    snap(
        RecoveryState::Running,
        "Idle until the next step.",
        "Start or resume when you are ready.",
    )
}

fn snap(state: RecoveryState, reason: &str, next: &str) -> RecoverySnapshot {
    RecoverySnapshot {
        state,
        reason: reason.to_string(),
        next: next.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> RecoveryFacts {
        RecoveryFacts {
            running: true,
            ..RecoveryFacts::default()
        }
    }

    #[test]
    fn hard_park_beats_ladder_retry() {
        let mut f = base();
        f.ladder = Some(Rung::Retry);
        f.hard_park_open = true;
        f.ladder_reason = Some("Send money".into());
        let s = project(&f);
        assert_eq!(s.state, RecoveryState::NeedsApproval);
        assert!(s.next.contains("Approve or deny"));
        assert!(s.status_line().starts_with("Needs approval — "));
    }

    #[test]
    fn needs_input_beats_soft_pause() {
        let mut f = base();
        f.soft_park_open = true;
        f.needs_input = true;
        let s = project(&f);
        assert_eq!(s.state, RecoveryState::NeedsInput);
    }

    #[test]
    fn soft_park_and_ladder_pause_are_paused() {
        let mut f = base();
        f.soft_park_open = true;
        f.ladder_reason = Some("`click` ran 3 times".into());
        let s = project(&f);
        assert_eq!(s.state, RecoveryState::Paused);
        assert_eq!(s.reason, "`click` ran 3 times");

        let mut g = base();
        g.ladder = Some(Rung::Pause);
        g.ladder_reason = Some("Can't capture the screen".into());
        assert_eq!(project(&g).state, RecoveryState::Paused);
    }

    #[test]
    fn drive_blocked_is_blocked_not_completed() {
        let mut f = base();
        f.running = false;
        f.drive = Some(DriveFact::Blocked);
        f.blocked_reason = Some("Stopped with these still open: \"ports\".".into());
        f.gaps_open = true;
        let s = project(&f);
        assert_eq!(s.state, RecoveryState::Blocked);
        assert!(s.reason.contains("ports"));
    }

    #[test]
    fn rung_maps_retry_backtrack_replan() {
        assert_eq!(state_for_rung(Rung::Retry), RecoveryState::Retrying);
        assert_eq!(state_for_rung(Rung::Backtrack), RecoveryState::Replanning);
        assert_eq!(state_for_rung(Rung::Replan), RecoveryState::Replanning);
        assert_eq!(state_for_rung(Rung::Pause), RecoveryState::Paused);

        let mut f = base();
        f.ladder = Some(Rung::Retry);
        f.ladder_reason = Some("tool error".into());
        f.ladder_next = Some("Try the same click once more.".into());
        let s = project(&f);
        assert_eq!(s.state, RecoveryState::Retrying);
        assert_eq!(s.next, "Try the same click once more.");

        f.ladder = Some(Rung::Backtrack);
        assert_eq!(project(&f).state, RecoveryState::Replanning);
        f.ladder = Some(Rung::Replan);
        assert_eq!(project(&f).state, RecoveryState::Replanning);
    }

    #[test]
    fn drive_push_with_gaps_is_replanning_not_done() {
        let mut f = base();
        f.drive = Some(DriveFact::Push);
        f.gaps_open = true;
        let s = project(&f);
        assert_eq!(s.state, RecoveryState::Replanning);
        assert!(!matches!(
            s.state,
            RecoveryState::Running | RecoveryState::CompletedGaps
        ));
    }

    #[test]
    fn completed_with_gaps_and_recovered() {
        let mut f = RecoveryFacts {
            completed_with_gaps: true,
            ladder_reason: Some("Unconfirmed: no VERIFY_OK".into()),
            ..RecoveryFacts::default()
        };
        assert_eq!(project(&f).state, RecoveryState::CompletedGaps);

        f = RecoveryFacts {
            resumed_after_wait: true,
            ladder_reason: Some("Approved the park".into()),
            ..RecoveryFacts::default()
        };
        assert_eq!(project(&f).state, RecoveryState::Recovered);
    }

    #[test]
    fn running_is_default_when_active_without_signals() {
        let s = project(&base());
        assert_eq!(s.state, RecoveryState::Running);
        assert_eq!(s.reason, "The current step is active.");
    }
}
