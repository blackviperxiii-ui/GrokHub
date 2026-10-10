//! A no-route pause heals itself. When no model in your plan answers, the
//! step that hit it waits; the cabin re-checks the models on a backoff
//! (15 s, 30 s, 1 m, 2 m, then every 5 m, with no cap) and resumes the step
//! on its own as soon as one answers. Only models in your plan are tried:
//! waiting never spends money on a model outside it. The budget pause
//! ([`super::budget`]) is a different pause and still waits for you.

use std::collections::BTreeMap;
use std::path::Path;

use grokhub_core::model_registry::cost_class::{probe_class, CostClass};
use grokhub_core::model_registry::profile::{read_profiles, ModelProfile};
use grokhub_core::model_registry::store::{load_registry, save_registry};
use grokhub_core::model_registry::{ModelMeta, ModelState, Registry};

use crate::harness::append_span;

use super::{heal, policy};

/// The waits between checks; the last one repeats for as long as the wait lasts.
pub const RETRY_STEPS_MS: [u64; 5] = [15_000, 30_000, 60_000, 120_000, 300_000];

/// Models a waiting card names at most.
const NAMED_MAX: usize = 3;

/// The longest step name a card or note carries.
const STEP_CHARS: usize = 48;

/// How long to wait before check number `attempt + 1`.
pub fn retry_delay_ms(attempt: u32) -> u64 {
    RETRY_STEPS_MS[(attempt as usize).min(RETRY_STEPS_MS.len() - 1)]
}

/// A step paused because no model in your plan answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoRouteWait {
    /// The route class of the paused call (`chat:default`).
    pub class: String,
    /// The model the call asked for.
    pub model: String,
    /// The paused step in plain words ("Summarize inbox").
    pub step: String,
    pub since_ms: u64,
    /// Checks that found nothing answering yet.
    pub attempts: u32,
    pub next_ms: u64,
}

impl NoRouteWait {
    /// A new wait. `attempts` carries over when the same step paused again
    /// right after a resume, so a route that keeps failing backs off too.
    pub fn new(class: &str, model: &str, step: &str, now_ms: u64, attempts: u32) -> Self {
        Self { class: class.into(), model: model.into(), step: step.into(), since_ms: now_ms, attempts, next_ms: now_ms + retry_delay_ms(attempts) }
    }

    pub fn due(&self, now_ms: u64) -> bool {
        now_ms >= self.next_ms
    }

    /// A check found nothing answering: back off one more step.
    pub fn missed(&mut self, now_ms: u64) {
        self.attempts = self.attempts.saturating_add(1);
        self.next_ms = now_ms + retry_delay_ms(self.attempts);
    }

    /// Retry now on the waiting card.
    pub fn retry_now(&mut self, now_ms: u64) {
        self.next_ms = now_ms;
    }
}

/// The paused step in plain words: your ask's first line, else the class's own words.
pub fn step_name(class: &str, ask: &str) -> String {
    let line = ask.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    if line.is_empty() {
        return policy::class_row(class).map_or(class, |r| r.plain).to_string();
    }
    if line.chars().count() <= STEP_CHARS {
        return line.to_string();
    }
    let cut: String = line.chars().take(STEP_CHARS - 1).collect();
    format!("{}…", cut.trim_end())
}

/// The models the wait is on: the one asked for, then the rest of its family
/// in your plan, newest first.
pub fn waiting_models(reg: &Registry, model: &str) -> Vec<String> {
    let model = model.trim();
    // A resting model is still in your plan: judge it as if it were answering.
    let mut view = reg.clone();
    for r in view.models.values_mut().filter(|r| r.state == ModelState::Quarantined) {
        r.state = ModelState::Live;
    }
    let cred = view.entitlement.credential;
    let in_plan: Vec<&str> = view.models.keys().filter(|id| probe_class(&view, cred, id) == CostClass::Included).map(String::as_str).collect();
    let mut out = Vec::new();
    if !model.is_empty() {
        out.push(model.to_string());
    }
    out.extend(policy::fallback_chain(model, &in_plan));
    out.truncate(NAMED_MAX);
    out
}

/// A model in your plan that can take an everyday step now: the one asked
/// for, else who stands in for it, else any. `None` while nothing answers.
pub fn model_back(reg: &Registry, profiles: &BTreeMap<String, ModelProfile>, model: &str, now_ms: u64) -> Option<String> {
    let ids = heal::candidates(reg, profiles, now_ms);
    if let Some(id) = ids.iter().find(|id| **id == model.trim()) {
        return Some(id.to_string());
    }
    heal::stand_in(reg, model, &ids).or_else(|| ids.first().map(|id| id.to_string()))
}

/// One check: if a model is back already, name it; otherwise ping the resting
/// models the wait is on (`ping` sends one small call) until one answers.
/// A model that answers leaves its rest and routes again.
pub fn recheck(config_dir: &Path, model: &str, now_ms: u64, ping: &mut dyn FnMut(&ModelMeta) -> bool) -> Option<String> {
    let mut reg = load_registry(config_dir);
    let profiles = read_profiles(config_dir);
    if let Some(back) = model_back(&reg, &profiles, model, now_ms) {
        return Some(back);
    }
    let mut changed = false;
    for id in waiting_models(&reg, model) {
        let Some(rec) = reg.get(&id).filter(|r| r.state == ModelState::Quarantined) else { continue };
        let meta = rec.meta.clone();
        reg.mark_half_open(&id);
        let ok = ping(&meta);
        if let Some(ev) = reg.half_open_result(&id, ok, now_ms) {
            let _ = append_span(config_dir, &super::refresh::event_span(&ev));
        }
        changed = true;
        if ok {
            break;
        }
    }
    if changed {
        let _ = save_registry(config_dir, &reg);
    }
    model_back(&reg, &profiles, model, now_ms)
}

fn join_names(models: &[String]) -> String {
    match models {
        [] => "your plan's models".into(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// The waiting card: (title, body). It names the models and the step and asks nothing.
pub fn waiting_card(models: &[String], since: &str, step: &str) -> (String, String) {
    let names = join_names(models);
    let verb = if models.len() == 1 { "isn't" } else { "aren't" };
    (format!("Waiting for a model: {names}"), format!("{names} {verb} answering since {since}; retrying. “{step}” picks up on its own once one answers."))
}

/// The quiet note when the wait ends. `resumed`: the paused step went out
/// again; otherwise it runs on its own next turn (a background job) or you moved on.
pub fn back_note(model: &str, step: &str, resumed: bool) -> String {
    if resumed {
        format!("Back on {model}; resumed: {step}")
    } else {
        format!("Back on {model} for {step}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::route::r2a_tests::{fleet, IDS};
    use grokhub_core::model_registry::profile::write_profile;
    use grokhub_core::model_registry::CallStatus;

    const NOW: u64 = 5_000_000;

    fn down(reg: &mut Registry, ids: &[&str]) {
        for id in ids {
            let obs: Vec<_> = (0..3).map(|i| grokhub_core::model_registry::Observation { model: id.to_string(), ts_ms: NOW + i, status: CallStatus::Error, latency_ms: 700, served_model: None, endpoint_ok: true, reasoning_tokens: 0, cost_ticks: 0, http: 0 }).collect();
            reg.fold(&obs);
        }
    }

    #[test]
    fn the_backoff_climbs_to_five_minutes_and_never_stops() {
        let delays: Vec<u64> = (0..8).map(retry_delay_ms).collect();
        assert_eq!(delays, vec![15_000, 30_000, 60_000, 120_000, 300_000, 300_000, 300_000, 300_000]);
        assert_eq!(retry_delay_ms(u32::MAX), 300_000);
        let mut w = NoRouteWait::new("chat:default", "grok-4.7", "Summarize inbox", NOW, 0);
        assert_eq!(w.next_ms, NOW + 15_000);
        assert!(!w.due(NOW + 14_999) && w.due(NOW + 15_000));
        let mut at = NOW;
        for _ in 0..50 {
            at = w.next_ms;
            w.missed(at);
        }
        assert_eq!((w.attempts, w.next_ms - at), (50, 300_000), "fifty misses later it still checks every 5 minutes");
        w.retry_now(at + 1);
        assert!(w.due(at + 1), "Retry now checks at once");
        assert_eq!(NoRouteWait::new("chat:default", "grok-4.7", "x", NOW, 3).next_ms, NOW + 120_000, "a wait that paused again keeps its place");
    }

    #[test]
    fn the_card_and_note_name_the_models_and_the_step() {
        let (reg, _) = fleet();
        let models = waiting_models(&reg, "grok-4.7");
        assert_eq!(models, vec!["grok-4.7", "grok-4.6", "grok-4.5"]);
        let (title, text) = waiting_card(&models, "12:14 AM", "Summarize inbox");
        assert_eq!(title, "Waiting for a model: grok-4.7, grok-4.6 and grok-4.5");
        assert_eq!(text, "grok-4.7, grok-4.6 and grok-4.5 aren't answering since 12:14 AM; retrying. “Summarize inbox” picks up on its own once one answers.");
        assert_eq!(waiting_card(&["grok-4.7".into()], "9:05 PM", "dream time").1, "grok-4.7 isn't answering since 9:05 PM; retrying. “dream time” picks up on its own once one answers.");
        assert_eq!(back_note("grok-4.6", "Summarize inbox", true), "Back on grok-4.6; resumed: Summarize inbox");
        assert_eq!(back_note("grok-4.6", "dream time", false), "Back on grok-4.6 for dream time");
        assert_eq!(step_name("chat:default", "\n  Summarize inbox  \nthen file it"), "Summarize inbox");
        assert_eq!(step_name("chat:default", &"word ".repeat(20)), "word word word word word word word word word wo…");
        assert_eq!(step_name("background:dream", ""), policy::class_row("background:dream").unwrap().plain);
    }

    #[test]
    fn all_down_waits_and_one_answering_again_ends_the_wait_on_that_model() {
        let dir = crate::harness::test_dir("route-wait-recheck");
        let (mut reg, profiles) = fleet();
        for p in profiles.values() {
            write_profile(&dir, &mut p.clone()).unwrap();
        }
        down(&mut reg, &IDS);
        save_registry(&dir, &reg).unwrap();
        assert_eq!(model_back(&reg, &profiles, "grok-4.7", NOW + 10), None, "all down: nothing to resume on");
        // Nothing answers: each resting model in the family gets one ping, and the wait goes on.
        let mut pinged = Vec::new();
        assert_eq!(recheck(&dir, "grok-4.7", NOW + 10, &mut |m| {
            pinged.push(m.id.clone());
            false
        }), None);
        assert_eq!(pinged, vec!["grok-4.7", "grok-4.6", "grok-4.5"]);
        // grok-4.6 answers again: the check stops there and the wait ends on it.
        let mut pinged = Vec::new();
        let back = recheck(&dir, "grok-4.7", NOW + 20, &mut |m| {
            pinged.push(m.id.clone());
            m.id == "grok-4.6"
        });
        assert_eq!(back.as_deref(), Some("grok-4.6"));
        assert_eq!(pinged, vec!["grok-4.7", "grok-4.6"]);
        let saved = load_registry(&dir);
        assert_eq!((saved.get("grok-4.6").unwrap().state, saved.get("grok-4.7").unwrap().state), (ModelState::Live, ModelState::Quarantined));
        // Next check: a model is back already, so nothing is pinged.
        let mut pings = 0;
        assert_eq!(recheck(&dir, "grok-4.7", NOW + 30, &mut |_| {
            pings += 1;
            true
        }).as_deref(), Some("grok-4.6"));
        assert_eq!(pings, 0);
        let _ = std::fs::remove_dir_all(dir);
    }
}
