//! The cabin side of the heartbeat throttle. The rules live in
//! `grokhub_core::heartbeat_throttle`; this file feeds them the cabin's state
//! (busy, Halt, outcomes) and traces each decision to `spans/heartbeat.jsonl`
//! with a reason tag and no content. `harness::decide` is untouched: an act the
//! gate lets through still meets the harness on its own path.

use super::*;
use grokhub_agent::harness as hx;
use grokhub_core::{ActOutcome, PaceGate, ProactiveAct};

/// Span session for pulse decisions, beside the chat span files.
pub(super) const HEARTBEAT_TRACE: &str = "heartbeat";

impl Cabin {
    /// The user is mid-turn or mid-thought: a live or queued turn, text in the
    /// composer, or a card waiting on them. The pulse starts nothing over that.
    pub(super) fn heartbeat_busy(&self) -> bool {
        self.running
            || self.pending_kick.is_some()
            || !self.composer.trim().is_empty()
            || self.decisions_waiting() > 0
    }

    /// May the pulse start `act` at `now`? Spends a slot when it may. Every
    /// decision that differs from the last one for this act is traced.
    pub(super) fn heartbeat_may(&mut self, act: ProactiveAct, now: u64) -> bool {
        let busy = self.heartbeat_busy();
        let gate = self.pace.gate(&self.cfg.heartbeat, now, busy);
        if gate == PaceGate::Act {
            self.pace.record_act(act, now);
        }
        self.trace_pace(act, gate);
        gate == PaceGate::Act
    }

    /// Halt is holding the pulse: only local upkeep runs.
    pub(super) fn heartbeat_halted(&mut self, now: u64) -> bool {
        self.pace.halted(&self.cfg.heartbeat, now)
    }

    pub(super) fn heartbeat_outcome(&mut self, act: ProactiveAct, outcome: ActOutcome) {
        let _ = self.pace.record_outcome(act, outcome);
    }

    /// A card the pulse put up was dismissed or marked Not this.
    pub(super) fn heartbeat_card_dismissed(&mut self) {
        self.pace.note_dismissed();
    }

    /// Your own message: an anticipate turn you answered was useful, and a Halt
    /// hold ends.
    pub(super) fn heartbeat_user_sent(&mut self) {
        self.pace.resume();
        self.heartbeat_outcome(ProactiveAct::Anticipate, ActOutcome::Useful);
    }

    /// Composer Stop or `/stop` on a live anticipate turn: you did not want it.
    pub(super) fn heartbeat_turn_stopped(&mut self) {
        if self.running && self.pace.pending() == Some(ProactiveAct::Anticipate) {
            self.heartbeat_outcome(ProactiveAct::Anticipate, ActOutcome::Dismissed);
        }
    }

    /// Tray Halt and the halt hotkeys: the pulse stops at once. Nothing new starts
    /// until the hold ends or you send, and an ask already out lands nowhere.
    pub(super) fn heartbeat_halt(&mut self, now: u64) {
        self.pace.halt(now);
        self.ideas_rx = None;
        self.review_rx = None;
        self.review_busy = false;
        self.night_check_rx = None;
        self.pace_traced.clear();
        self.write_pace_span("halt", "hold", "halted");
    }

    /// A background kind spent today's budget: the governor's note goes on the
    /// status line once, and the trace gets the kind and the reason, no content.
    pub(super) fn governor_note(&mut self, note: &grokhub_core::GovernorNote) {
        self.status = note.text.clone();
        let act = format!("governor.{}", note.kind.as_str());
        self.write_pace_span(&act, "hold", grokhub_core::JobHold::BudgetSpent.reason());
    }

    /// You're active: a chat run is going, the composer holds a draft, or your
    /// last key or click was moments ago. The governor holds heavy jobs then.
    pub(super) fn user_active_now(&self) -> bool {
        grokhub_core::user_active(
            self.running,
            !self.composer.trim().is_empty(),
            self.last_activity.elapsed().as_millis() as u64,
        )
    }

    /// The router goal a governor job's model call runs on: cost, reason "user
    /// active", for a light model job while you're active; else none.
    pub(super) fn bg_goal(&self, kind: JobKind) -> Option<grokhub_agent::route::goal::RouteGoal> {
        let spec = self.governor.spec(kind)?;
        if !grokhub_core::runs_cheap(spec, self.user_active_now()) {
            return None;
        }
        grokhub_agent::route::goal::RouteGoal::of(
            grokhub_core::ACTIVE_GOAL,
            grokhub_core::ACTIVE_REASON,
        )
    }

    /// Charge tokens a background job spent to its governor kind for today.
    pub(super) fn governor_charge(&mut self, kind: JobKind, tokens: u64) {
        if tokens == 0 {
            return;
        }
        self.governor.charge(kind, tokens, &Self::local_day());
    }

    fn trace_pace(&mut self, act: ProactiveAct, gate: PaceGate) {
        let reason = gate.reason();
        let acted = gate == PaceGate::Act;
        if !acted && self.pace_traced.contains(&(act, reason)) {
            return;
        }
        self.pace_traced.retain(|(a, _)| *a != act);
        self.pace_traced.push((act, reason));
        self.write_pace_span(act.as_str(), if acted { "allow" } else { "hold" }, reason);
    }

    /// Local upkeep on the Housekeep slot: rhythm, recap, print alerts, persist.
    pub(super) fn tick_housekeep(&mut self) {
        self.housekeep_rhythm_and_recap();
        self.tick_print_alerts();
        if self.last_persist.elapsed() > std::time::Duration::from_secs(2) {
            self.persist_bg();
        }
    }

    /// One span line: the act, allow or hold, and the reason tag. No prompt,
    /// no reply, no chat id.
    fn write_pace_span(&self, act: &str, decision: &str, reason: &str) {
        let mut span = hx::Span::soft_allow(
            HEARTBEAT_TRACE,
            &format!("heartbeat.{act}"),
            "",
            reason,
            "pace",
            self.access_mode(),
            "none",
        )
        .from_origin(hx::Origin::Proactive)
        .on_path("heartbeat");
        span.decision = decision.into();
        let _ = hx::append_span(&crate::config::config_dir(), &span);
    }
}
