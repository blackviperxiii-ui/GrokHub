//! The cabin side of the heartbeat throttle. The rules live in
//! `grokhub_core::heartbeat_throttle`; this file feeds them the cabin's state
//! (busy, Halt, outcomes) and traces each decision to `spans/heartbeat.jsonl`
//! with a reason tag and no content. `harness::decide` is untouched: an act the
//! gate lets through still meets the harness on its own path.

use super::*;
use grokhub_agent::harness as hx;
use grokhub_core::{
    pace_label, with_pace_preset, ActOutcome, PaceGate, ProactiveAct, PACE_PRESETS,
};

/// Span session for pulse decisions, beside the chat span files.
pub(super) const HEARTBEAT_TRACE: &str = "heartbeat";

const PACE_HINT: &str = "How often Grok acts on its own: anticipating a need, new ideas, the nightly review. It waits while you're mid-turn, slows down after dismissals, and Halt pauses it. Automations keep their own times.";

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

    /// Settings → Behavior: one dropdown for the pace presets.
    pub(super) fn ui_heartbeat_pace(&mut self, ui: &mut egui::Ui) {
        let labels: Vec<String> = PACE_PRESETS.iter().map(|(l, _)| (*l).to_string()).collect();
        let picked = crate::cards::settings_dropdown(
            ui,
            "Proactive pace",
            PACE_HINT,
            pace_label(&self.cfg.heartbeat),
            &labels,
        );
        if let Some((_, preset)) = picked.and_then(|i| PACE_PRESETS.get(i)) {
            self.cfg.heartbeat = with_pace_preset(&self.cfg.heartbeat, preset);
            self.pace_traced.clear();
            self.persist_cfg();
            self.status = "Saved".into();
        }
    }
}
