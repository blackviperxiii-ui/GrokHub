//! Spike-6a proactive engine: GrokHub notices what you might need and offers
//! it as a few well-timed Pulse cards. This half only suggests; nothing runs
//! without your click, and a click still meets `harness::decide` on its normal
//! path (origin proactive).
//!
//! Pure and deterministic: the clock is injected, rules rank (the Pulse
//! invariant: the same inputs give the same order), and a model may only write
//! a candidate's sentences. `help = value × confidence × reversibility`, each in
//! [0, 1], goes into `pulse_score` as the integer `help × 1000`.
//!
//! Two budgets sit in front of a card. The heartbeat throttle budgets act
//! starts (Anticipate spends one); [`ProactiveBudget`] budgets the cards those
//! acts may put up: 3 per 4 hours, 8 per day, none in quiet hours (queued to
//! the end of the window) or while you are busy, a 7-day mute per "Not this"
//! topic, and half the budget for 24 hours after 2 dismissals in a row.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::ideas::same_topic;
use crate::pulse::{pulse_visible, PULSE_THRESHOLD};
use crate::update_feed::{UpdateCard, UpdateKind};

pub const HOUR_MS: u64 = 60 * 60 * 1000;
pub const DAY_MS: u64 = 24 * HOUR_MS;
/// At most this many new proactive cards in any 4-hour window.
pub const CARDS_PER_WINDOW: usize = 3;
pub const CARD_WINDOW_MS: u64 = 4 * HOUR_MS;
/// At most this many new proactive cards in any 24 hours.
pub const CARDS_PER_DAY: usize = 8;
/// "Not this" mutes that topic for 7 days.
pub const NOT_THIS_MUTE_MS: u64 = 7 * DAY_MS;
/// This many dismissals in a row halve both caps…
pub const DISMISS_STREAK: u32 = 2;
/// …for 24 hours.
pub const HALVED_MS: u64 = DAY_MS;
/// Quiet-hours queue length; the best ones are kept.
pub const QUIET_QUEUE_MAX: usize = CARDS_PER_DAY;
/// An OS notification only for something due in under an hour.
pub const NOTIFY_DUE_MS: u64 = HOUR_MS;
/// Card ids start with this, so the cabin can tell its own offers apart.
pub const PROACTIVE_ID_PREFIX: &str = "proactive-";

// ---------------------------------------------------------------- candidates

/// Where a candidate came from. Calendar, Mail and SystemState are read only
/// with a consent grant for their scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateSource {
    Pulse,
    Workboard,
    AutomationHealth,
    Calendar,
    Mail,
    SystemState,
    Spans,
    UserModel,
}

impl CandidateSource {
    /// The consent scope key that must be granted before this source is read.
    pub fn grant_scope(self) -> Option<&'static str> {
        match self {
            Self::Calendar => Some("calendar"),
            Self::Mail => Some("mail"),
            Self::SystemState => Some("system_state"),
            _ => None,
        }
    }
}

/// Soft runs on the normal gate path; Hard (money, send, delete, credentials,
/// an irreversible OS action) always parks a hard card and is only prepared.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateClass {
    #[default]
    Soft,
    Hard,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reversibility {
    /// An undo ledger entry exists: 1.0.
    Ledger,
    /// You can put it back by hand: 0.5.
    #[default]
    ByHand,
    /// Irreversible: 0.
    Irreversible,
}

impl Reversibility {
    pub fn weight(self) -> f64 {
        match self {
            Self::Ledger => 1.0,
            Self::ByHand => 0.5,
            Self::Irreversible => 0.0,
        }
    }
}

/// `{action, class, reversible, scope, value, confidence, why}` plus what the
/// rules and the card need. `action` reads after "I can …".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Candidate {
    pub action: String,
    /// Short topic words for muting and dedupe ("standup prep").
    pub topic: String,
    pub class: CandidateClass,
    pub reversible: Reversibility,
    /// The consent scope it reads or acts in, if any.
    pub scope: Option<String>,
    pub value: f64,
    pub confidence: f64,
    pub why: String,
    pub source: CandidateSource,
    /// The tool the step would call, for `harness::decide` on click.
    pub tool: String,
    #[serde(default)]
    pub due_at: Option<u64>,
    /// Hard candidates only: what GrokHub prepares (a draft, a filled form).
    #[serde(default)]
    pub prepared: Option<String>,
}

impl Candidate {
    /// A soft candidate with the common defaults.
    pub fn soft(source: CandidateSource, action: &str, topic: &str, value: f64, confidence: f64, why: &str) -> Self {
        Self {
            action: action.to_string(),
            topic: topic.to_string(),
            class: CandidateClass::Soft,
            reversible: Reversibility::ByHand,
            scope: source.grant_scope().map(str::to_string),
            value,
            confidence,
            why: why.to_string(),
            source,
            tool: "run_task".into(),
            due_at: None,
            prepared: None,
        }
    }

    /// `value × confidence × reversibility`, each clamped to [0, 1]. Hard class
    /// is 0: GrokHub never takes a hard step on its own.
    pub fn help(&self) -> f64 {
        if self.class == CandidateClass::Hard {
            return 0.0;
        }
        unit(self.value) * unit(self.confidence) * self.reversible.weight()
    }

    /// `help × 1000`, the integer that joins `pulse_score`.
    pub fn help_milli(&self) -> u32 {
        (self.help() * 1000.0).round() as u32
    }

    /// Hard means prepare, don't do: the card is the preparing step (a draft
    /// you can throw away, so by-hand reversible), and the hard step stays a
    /// click that parks a hard card.
    pub fn prepare_step(&self) -> Candidate {
        if self.class != CandidateClass::Hard {
            return self.clone();
        }
        Candidate {
            class: CandidateClass::Soft,
            reversible: Reversibility::ByHand,
            prepared: Some(self.prepared.clone().unwrap_or_else(|| format!("Draft: {}", self.action))),
            ..self.clone()
        }
    }

    fn text(&self) -> String {
        format!(
            "{} {} {} {}",
            self.action,
            self.topic,
            self.why,
            self.prepared.as_deref().unwrap_or("")
        )
    }
}

fn unit(v: f64) -> f64 {
    if v.is_nan() {
        0.0
    } else {
        v.clamp(0.0, 1.0)
    }
}

// ---------------------------------------------------------------- routes

/// Where a surfaced candidate goes. There is no "act" route in 6a: every
/// route is a card and nothing runs without your click.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProactiveRoute {
    /// A Pulse "I can …" card.
    #[default]
    ICan,
    /// Hard class: a prepared draft, and its final step is a hard card.
    Prepare,
    /// MindCheck is unsure: "Should I …?" first.
    Ask,
}

/// `None` under the Pulse threshold. `unsure` is MindCheck's Ask.
pub fn route(candidate: &Candidate, unsure: bool) -> Option<ProactiveRoute> {
    let step = candidate.prepare_step();
    if i64::from(step.help_milli()) < PULSE_THRESHOLD {
        return None;
    }
    if candidate.class == CandidateClass::Hard {
        return Some(ProactiveRoute::Prepare);
    }
    Some(if unsure { ProactiveRoute::Ask } else { ProactiveRoute::ICan })
}

/// The proactive part of a Pulse card. Serde-default on `PulseMeta`, so
/// older cards and builds are unaffected.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProactiveMeta {
    /// `help × 1000`, added to `pulse_score`.
    pub help: u32,
    pub route: ProactiveRoute,
    pub topic: String,
    pub tool: String,
    #[serde(default)]
    pub hard: bool,
    /// The step a Prepare card holds back ("send the reply to Sam").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub final_step: Option<String>,
}

/// The card for a surfaced candidate, on the existing Pulse/Home surfaces.
pub fn proactive_card(candidate: &Candidate, route: ProactiveRoute, now_ms: u64) -> UpdateCard {
    let step = candidate.prepare_step();
    let title = match route {
        ProactiveRoute::Ask => format!("Should I {}?", candidate.action.trim_end_matches(['.', '?'])),
        ProactiveRoute::Prepare => format!("I can draft this: {}", candidate.action),
        ProactiveRoute::ICan => format!("I can {}", candidate.action),
    };
    let mut card = crate::update_feed::blank_card(
        format!("{PROACTIVE_ID_PREFIX}{}-{now_ms}", topic_key(&candidate.topic)),
        UpdateKind::Suggestion,
        title,
        step.prepared.clone(),
        now_ms,
    );
    card.why = Some(candidate.why.clone());
    card.prompt = Some(candidate.action.clone());
    card.pulse.due_at = candidate.due_at;
    card.pulse.proactive = Some(ProactiveMeta {
        help: step.help_milli(),
        route,
        topic: candidate.topic.clone(),
        tool: candidate.tool.clone(),
        hard: candidate.class == CandidateClass::Hard,
        final_step: (candidate.class == CandidateClass::Hard).then(|| candidate.action.clone()),
    });
    card
}

/// Lowercase words joined by `-`: the topic as a card id part and a MindCheck key.
pub fn topic_key(text: &str) -> String {
    let s: String = text
        .to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    s.split('-').filter(|w| !w.is_empty()).collect::<Vec<_>>().join("-")
}

/// An OS notification only when you opted into reminders and it is due in
/// under an hour (not already past).
pub fn notify_os(candidate: &Candidate, now_ms: u64, reminders_on: bool) -> bool {
    reminders_on && candidate.due_at.is_some_and(|due| due >= now_ms && due - now_ms < NOTIFY_DUE_MS)
}

// ---------------------------------------------------------------- watch

/// An open workboard card.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WorkItem {
    pub title: String,
    pub blocked: bool,
    /// A follow-up report you have not opened.
    pub fresh_report: bool,
    pub due_at: Option<u64>,
}

/// A span-derived signal: a task that failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailedTask {
    pub title: String,
}

/// A user-model routine or need (Spike-5a nodes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserNeed {
    pub text: String,
    pub routine: bool,
}

/// Everything the engine watches that needs no grant.
#[derive(Debug, Clone, Default)]
pub struct Watch<'a> {
    /// Pulse, Ideas and Feed cards and Home updates. Used to skip topics
    /// already on the page.
    pub cards: &'a [UpdateCard],
    pub work: &'a [WorkItem],
    /// `automation_health_line` per failing automation.
    pub automation_health: &'a [String],
    pub failed: &'a [FailedTask],
    pub needs: &'a [UserNeed],
}

/// A connector or system source behind a consent grant. `read` is only
/// called when its scope is granted.
pub trait GrantedSource {
    fn source(&self) -> CandidateSource;
    fn read(&self) -> Vec<Candidate>;
}

/// The engine: gathers candidates from the watched sources, honoring grants,
/// and ranks them by rules. It never acts.
pub struct ProactiveEngine {
    clock: Arc<dyn Fn() -> u64 + Send + Sync>,
}

impl std::fmt::Debug for ProactiveEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProactiveEngine").finish()
    }
}

impl ProactiveEngine {
    pub fn new(clock: Arc<dyn Fn() -> u64 + Send + Sync>) -> Self {
        Self { clock }
    }

    pub fn now_ms(&self) -> u64 {
        (self.clock)()
    }

    /// Candidates from every watched source, deduped by topic, minus any
    /// topic already on a visible card, minus anything carrying a secret.
    /// A granted source is read only when `granted` names its scope.
    pub fn gather(&self, watch: &Watch<'_>, sources: &[&dyn GrantedSource], granted: &[&str]) -> Vec<Candidate> {
        let now = self.now_ms();
        let mut out = Vec::new();
        for item in watch.work {
            if item.blocked {
                let mut c = Candidate::soft(
                    CandidateSource::Workboard,
                    &format!("help unblock \"{}\"", item.title),
                    &item.title,
                    0.7,
                    0.6,
                    "It is blocked on your board.",
                );
                c.due_at = item.due_at;
                out.push(c);
            } else if item.fresh_report {
                out.push(Candidate::soft(
                    CandidateSource::Workboard,
                    &format!("sum up the new report on \"{}\"", item.title),
                    &item.title,
                    0.6,
                    0.8,
                    "A run left a report you have not opened.",
                ));
            } else if let Some(due) = item.due_at.filter(|d| *d <= now.saturating_add(DAY_MS)) {
                let mut c = Candidate::soft(
                    CandidateSource::Workboard,
                    &format!("get \"{}\" ready", item.title),
                    &item.title,
                    0.8,
                    0.7,
                    "It is due within a day.",
                );
                c.due_at = Some(due);
                out.push(c);
            }
        }
        for line in watch.automation_health {
            out.push(Candidate::soft(
                CandidateSource::AutomationHealth,
                &format!("look into why {}", lower_first(line)),
                line,
                0.6,
                0.8,
                "An automation keeps failing.",
            ));
        }
        for task in watch.failed {
            out.push(Candidate::soft(
                CandidateSource::Spans,
                &format!("retry \"{}\"", task.title),
                &task.title,
                0.5,
                0.6,
                "It failed last time.",
            ));
        }
        for need in watch.needs {
            let (action, value) = if need.routine {
                (format!("get {} ready before you start", lower_first(&need.text)), 0.6)
            } else {
                (format!("help with {}", lower_first(&need.text)), 0.5)
            };
            out.push(Candidate::soft(
                CandidateSource::UserModel,
                &action,
                &need.text,
                value,
                0.5,
                if need.routine { "You do this regularly." } else { "You said you need this." },
            ));
        }
        for src in sources {
            let source = src.source();
            let allowed = match source.grant_scope() {
                Some(scope) => granted.contains(&scope),
                None => true,
            };
            if !allowed {
                continue;
            }
            out.extend(src.read().into_iter().map(|mut c| {
                c.source = source;
                c.scope = source.grant_scope().map(str::to_string);
                c
            }));
        }
        out.retain(|c| !carries_secret(c));
        out.retain(|c| {
            !watch
                .cards
                .iter()
                .filter(|card| pulse_visible(card, now))
                .any(|card| same_topic(&card.title, &c.topic) || card.title.eq_ignore_ascii_case(&c.topic))
        });
        rank_candidates(dedupe(out))
    }
}

fn lower_first(text: &str) -> String {
    let t = text.trim().trim_end_matches('.');
    let mut chars = t.chars();
    match chars.next() {
        Some(first) => first.to_lowercase().chain(chars).collect(),
        None => String::new(),
    }
}

fn carries_secret(c: &Candidate) -> bool {
    let text = c.text();
    crate::redact::redact_secrets(&text) != text
}

/// Keep the best candidate per topic.
fn dedupe(cands: Vec<Candidate>) -> Vec<Candidate> {
    let mut kept: Vec<Candidate> = Vec::new();
    for c in rank_candidates(cands) {
        if !kept.iter().any(|k| same_topic(&k.topic, &c.topic) || k.topic.eq_ignore_ascii_case(&c.topic)) {
            kept.push(c);
        }
    }
    kept
}

/// Rules order: help (as its prepared step), then sooner due, then topic,
/// then action. No clock noise, no model: the same inputs give the same order.
pub fn rank_candidates(mut cands: Vec<Candidate>) -> Vec<Candidate> {
    cands.sort_by(|a, b| {
        b.prepare_step()
            .help_milli()
            .cmp(&a.prepare_step().help_milli())
            .then(a.due_at.unwrap_or(u64::MAX).cmp(&b.due_at.unwrap_or(u64::MAX)))
            .then(a.topic.cmp(&b.topic))
            .then(a.action.cmp(&b.action))
    });
    cands
}

// ---------------------------------------------------------------- budget

/// The card budget, beside (not instead of) the heartbeat act throttle.
/// Serde so mutes and the quiet queue survive a relaunch.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProactiveBudget {
    /// When each surfaced card went up (last 24 h kept).
    #[serde(default)]
    pub shown: Vec<u64>,
    #[serde(default)]
    pub dismiss_streak: u32,
    #[serde(default)]
    pub halved_until: u64,
    /// `(topic, muted until)`.
    #[serde(default)]
    pub mutes: Vec<(String, u64)>,
    /// Held during quiet hours; drains after the window.
    #[serde(default)]
    pub queue: Vec<Candidate>,
}

impl ProactiveBudget {
    /// `(per 4 hours, per day)`, halved after a dismissal streak.
    pub fn caps(&self, now_ms: u64) -> (usize, usize) {
        if now_ms < self.halved_until {
            ((CARDS_PER_WINDOW / 2).max(1), CARDS_PER_DAY / 2)
        } else {
            (CARDS_PER_WINDOW, CARDS_PER_DAY)
        }
    }

    /// Cards that may still go up now.
    pub fn room(&self, now_ms: u64) -> usize {
        let (window_cap, day_cap) = self.caps(now_ms);
        let in_window = self.shown.iter().filter(|t| now_ms.saturating_sub(**t) < CARD_WINDOW_MS).count();
        let in_day = self.shown.iter().filter(|t| now_ms.saturating_sub(**t) < DAY_MS).count();
        window_cap.saturating_sub(in_window).min(day_cap.saturating_sub(in_day))
    }

    pub fn muted(&self, topic: &str, now_ms: u64) -> bool {
        self.mutes
            .iter()
            .any(|(t, until)| now_ms < *until && (t.eq_ignore_ascii_case(topic) || same_topic(t, topic)))
    }

    /// "Not this": the topic is muted for 7 days, and it counts as a dismissal.
    pub fn not_this(&mut self, topic: &str, now_ms: u64) {
        self.mutes.retain(|(_, until)| now_ms < *until);
        self.mutes.push((topic.to_string(), now_ms.saturating_add(NOT_THIS_MUTE_MS)));
        self.dismissed(now_ms);
    }

    /// A proactive card dismissed. Two in a row halve the budget for 24 h.
    pub fn dismissed(&mut self, now_ms: u64) {
        self.dismiss_streak += 1;
        if self.dismiss_streak >= DISMISS_STREAK {
            self.halved_until = now_ms.saturating_add(HALVED_MS);
            self.dismiss_streak = 0;
        }
    }

    /// You used a proactive card: the streak resets.
    pub fn engaged(&mut self) {
        self.dismiss_streak = 0;
    }

    /// The candidates that become cards now, best first. Busy: none, and
    /// nothing queued. Quiet hours: none, and the best are queued for the end
    /// of the window. Otherwise the queue drains first, then the new ones, up
    /// to the room left; muted topics never surface.
    pub fn surface(&mut self, ranked: Vec<Candidate>, now_ms: u64, quiet: bool, busy: bool) -> Vec<Candidate> {
        self.shown.retain(|t| now_ms.saturating_sub(*t) < DAY_MS);
        self.mutes.retain(|(_, until)| now_ms < *until);
        if busy {
            return Vec::new();
        }
        let mut pool = std::mem::take(&mut self.queue);
        pool.extend(ranked);
        let pool: Vec<Candidate> = dedupe(pool).into_iter().filter(|c| !self.muted(&c.topic, now_ms)).collect();
        if quiet {
            self.queue = pool.into_iter().take(QUIET_QUEUE_MAX).collect();
            return Vec::new();
        }
        let room = self.room(now_ms);
        let mut out = Vec::new();
        for c in pool {
            if out.len() >= room {
                break;
            }
            if route(&c, false).is_some() {
                self.shown.push(now_ms);
                out.push(c);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    const T0: u64 = 1_800_000_000_000;

    fn engine_at(now: u64) -> ProactiveEngine {
        ProactiveEngine::new(Arc::new(move || now))
    }

    fn cand(topic: &str, value: f64) -> Candidate {
        let mut c = Candidate::soft(CandidateSource::UserModel, &format!("prep {topic}"), topic, value, 1.0, "test");
        c.reversible = Reversibility::Ledger;
        c
    }

    #[test]
    fn help_is_value_times_confidence_times_reversibility() {
        let mut c = Candidate::soft(CandidateSource::Pulse, "tidy downloads", "downloads", 0.8, 0.5, "w");
        c.reversible = Reversibility::Ledger;
        assert_eq!(c.help_milli(), 400);
        c.reversible = Reversibility::ByHand;
        assert_eq!(c.help_milli(), 200);
        c.reversible = Reversibility::Irreversible;
        assert_eq!(c.help_milli(), 0);
        c.reversible = Reversibility::Ledger;
        c.class = CandidateClass::Hard;
        assert_eq!(c.help_milli(), 0, "hard class is never help on its own");
        c.class = CandidateClass::Soft;
        c.value = 4.0;
        c.confidence = f64::NAN;
        assert_eq!(c.help_milli(), 0, "out-of-range inputs clamp");
    }

    #[test]
    fn half_reversible_candidate_is_a_card_never_an_action() {
        let c = Candidate::soft(CandidateSource::Workboard, "tidy the board", "board tidy", 0.9, 0.9, "w");
        assert_eq!(c.reversible, Reversibility::ByHand);
        assert_eq!(c.help_milli(), 405);
        // Confident and unasked-for still routes to an "I can …" card.
        assert_eq!(route(&c, false), Some(ProactiveRoute::ICan));
        let card = proactive_card(&c, ProactiveRoute::ICan, T0);
        assert_eq!(card.title, "I can tidy the board");
        assert_eq!(card.kind, UpdateKind::Suggestion);
        assert_eq!(card.pulse.proactive.as_ref().map(|p| p.help), Some(405));
        // The only routes are cards: ICan, Prepare, Ask.
        for unsure in [false, true] {
            assert!(matches!(
                route(&c, unsure),
                Some(ProactiveRoute::ICan | ProactiveRoute::Ask)
            ));
        }
    }

    #[test]
    fn reply_to_sam_is_prepared_as_a_draft_with_a_held_send_step() {
        let mut c = Candidate::soft(CandidateSource::Mail, "reply to Sam's email", "sam reply", 0.9, 0.8, "Sam asked about Friday.");
        c.class = CandidateClass::Hard;
        c.reversible = Reversibility::Irreversible;
        c.tool = "send_email".into();
        c.prepared = Some("Hi Sam, Friday works for me.".into());
        assert_eq!(c.help_milli(), 0);
        assert_eq!(c.prepare_step().help_milli(), 360, "the draft is by-hand reversible");
        assert_eq!(route(&c, false), Some(ProactiveRoute::Prepare));
        assert_eq!(route(&c, true), Some(ProactiveRoute::Prepare), "unsure changes nothing for hard");
        let card = proactive_card(&c, ProactiveRoute::Prepare, T0);
        assert_eq!(card.title, "I can draft this: reply to Sam's email");
        assert_eq!(card.body.as_deref(), Some("Hi Sam, Friday works for me."));
        let meta = card.pulse.proactive.unwrap();
        assert!(meta.hard);
        assert_eq!(meta.tool, "send_email");
        assert_eq!(meta.final_step.as_deref(), Some("reply to Sam's email"));
    }

    #[test]
    fn unsure_gives_an_ask_card() {
        let c = cand("standup prep", 0.5);
        assert_eq!(route(&c, true), Some(ProactiveRoute::Ask));
        let card = proactive_card(&c, ProactiveRoute::Ask, T0);
        assert_eq!(card.title, "Should I prep standup prep?");
    }

    #[test]
    fn under_the_threshold_gives_no_card() {
        let c = cand("low", 0.09);
        assert_eq!(c.help_milli(), 90);
        assert_eq!(route(&c, false), None);
    }

    #[test]
    fn twenty_candidates_in_an_hour_surface_at_most_three() {
        let mut budget = ProactiveBudget::default();
        let mut shown = 0;
        for i in 0..20u64 {
            let batch = vec![cand(&format!("topic{i} alpha{i}"), 0.9)];
            shown += budget.surface(batch, T0 + i * 3 * 60_000, false, false).len();
        }
        assert_eq!(shown, 3);
        // The next 4-hour window gives three more, and the day stops at 8.
        let mut day = 3;
        for w in 1..6u64 {
            let at = T0 + w * CARD_WINDOW_MS + HOUR_MS;
            let batch: Vec<Candidate> = (0..5).map(|i| cand(&format!("w{w}x{i} beta{w}{i}"), 0.9)).collect();
            day += budget.surface(batch, at, false, false).len();
        }
        assert_eq!(day, CARDS_PER_DAY);
    }

    #[test]
    fn quiet_hours_surface_nothing_and_the_queue_drains_after() {
        let mut budget = ProactiveBudget::default();
        let batch: Vec<Candidate> = (0..5).map(|i| cand(&format!("night{i} gamma{i}"), 0.5 + i as f64 / 10.0)).collect();
        assert!(budget.surface(batch, T0, true, false).is_empty());
        assert_eq!(budget.queue.len(), 5);
        assert!(budget.shown.is_empty());
        let out = budget.surface(Vec::new(), T0 + 8 * HOUR_MS, false, false);
        let topics: Vec<&str> = out.iter().map(|c| c.topic.as_str()).collect();
        assert_eq!(topics, vec!["night4 gamma4", "night3 gamma3", "night2 gamma2"]);
        assert!(budget.queue.is_empty());
    }

    #[test]
    fn busy_surfaces_nothing_and_queues_nothing() {
        let mut budget = ProactiveBudget::default();
        assert!(budget.surface(vec![cand("busy topic", 0.9)], T0, false, true).is_empty());
        assert!(budget.queue.is_empty());
        assert_eq!(budget.room(T0), 3);
    }

    #[test]
    fn not_this_mutes_standup_prep_for_seven_days() {
        let mut budget = ProactiveBudget::default();
        budget.not_this("standup prep", T0);
        let again = || vec![cand("standup prep", 0.9)];
        assert!(budget.surface(again(), T0 + HOUR_MS, false, false).is_empty());
        assert!(budget.surface(again(), T0 + NOT_THIS_MUTE_MS - 1, false, false).is_empty());
        let back = budget.surface(again(), T0 + NOT_THIS_MUTE_MS, false, false);
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].topic, "standup prep");
    }

    #[test]
    fn two_dismissals_in_a_row_halve_the_budget_for_a_day() {
        let mut budget = ProactiveBudget::default();
        budget.dismissed(T0);
        assert_eq!(budget.caps(T0), (3, 8), "one dismissal changes nothing");
        budget.engaged();
        budget.dismissed(T0);
        assert_eq!(budget.caps(T0), (3, 8), "a use in between resets the streak");
        budget.dismissed(T0 + 1);
        assert_eq!(budget.caps(T0 + 1), (1, 4));
        let batch: Vec<Candidate> = (0..5).map(|i| cand(&format!("half{i} delta{i}"), 0.9)).collect();
        assert_eq!(budget.surface(batch, T0 + 2, false, false).len(), 1);
        assert_eq!(budget.caps(T0 + 1 + HALVED_MS), (3, 8));
    }

    #[test]
    fn same_inputs_give_the_same_order() {
        let mut a = cand("b topic", 0.5);
        a.due_at = Some(T0 + HOUR_MS);
        let b = cand("a topic", 0.5);
        let c = cand("c topic", 0.9);
        let ranked = rank_candidates(vec![b.clone(), c.clone(), a.clone()]);
        let again = rank_candidates(vec![a.clone(), b.clone(), c.clone()]);
        assert_eq!(ranked, again);
        let topics: Vec<&str> = ranked.iter().map(|c| c.topic.as_str()).collect();
        assert_eq!(topics, vec!["c topic", "b topic", "a topic"], "help, then sooner due, then topic");
    }

    struct CountingCalendar<'a> {
        reads: &'a Cell<u32>,
    }

    impl GrantedSource for CountingCalendar<'_> {
        fn source(&self) -> CandidateSource {
            CandidateSource::Calendar
        }
        fn read(&self) -> Vec<Candidate> {
            self.reads.set(self.reads.get() + 1);
            vec![Candidate::soft(CandidateSource::Calendar, "prep the 10:00 standup", "standup prep", 0.8, 0.8, "On your calendar.")]
        }
    }

    #[test]
    fn calendar_without_a_grant_is_never_read() {
        let reads = Cell::new(0);
        let cal = CountingCalendar { reads: &reads };
        let engine = engine_at(T0);
        let none = engine.gather(&Watch::default(), &[&cal], &[]);
        assert!(none.is_empty());
        assert_eq!(reads.get(), 0, "no grant: the connector is not read");
        let none = engine.gather(&Watch::default(), &[&cal], &["mail"]);
        assert!(none.is_empty());
        assert_eq!(reads.get(), 0);
        let some = engine.gather(&Watch::default(), &[&cal], &["calendar"]);
        assert_eq!(reads.get(), 1);
        assert_eq!(some.len(), 1);
        assert_eq!(some[0].scope.as_deref(), Some("calendar"));
    }

    #[test]
    fn gather_reads_the_board_health_spans_and_needs_and_skips_what_is_on_the_page() {
        let work = [
            WorkItem { title: "Quarterly taxes".into(), blocked: true, ..Default::default() },
            WorkItem { title: "Ship notes".into(), due_at: Some(T0 + HOUR_MS), ..Default::default() },
            WorkItem { title: "Someday idea".into(), ..Default::default() },
        ];
        let health = ["Backup failed 3 times in a row".to_string()];
        let failed = [FailedTask { title: "Sync photos".into() }];
        let needs = [UserNeed { text: "Standup prep".into(), routine: true }];
        let mut on_page = crate::update_feed::blank_card("x".into(), UpdateKind::Idea, "Sync photos tonight".into(), None, T0);
        on_page.status = crate::update_feed::UpdateStatus::Unread;
        let cards = [on_page];
        let watch = Watch { cards: &cards, work: &work, automation_health: &health, failed: &failed, needs: &needs };
        let got = engine_at(T0).gather(&watch, &[], &[]);
        let actions: Vec<(&str, u32)> = got.iter().map(|c| (c.action.as_str(), c.help_milli())).collect();
        assert_eq!(
            actions,
            vec![
                ("get \"Ship notes\" ready", 280),
                ("look into why backup failed 3 times in a row", 240),
                ("help unblock \"Quarterly taxes\"", 210),
                ("get standup prep ready before you start", 150),
            ]
        );
    }

    #[test]
    fn a_candidate_with_a_secret_never_reaches_a_card() {
        struct Leaky;
        impl GrantedSource for Leaky {
            fn source(&self) -> CandidateSource {
                CandidateSource::Mail
            }
            fn read(&self) -> Vec<Candidate> {
                vec![Candidate::soft(
                    CandidateSource::Mail,
                    "save the key sk-abcdefghijklmnopqrstuv",
                    "api key",
                    0.9,
                    0.9,
                    "w",
                )]
            }
        }
        assert!(engine_at(T0).gather(&Watch::default(), &[&Leaky], &["mail"]).is_empty());
    }

    #[test]
    fn os_notification_only_under_an_hour_and_opted_in() {
        let mut c = cand("bill", 0.9);
        c.due_at = Some(T0 + 30 * 60_000);
        assert!(notify_os(&c, T0, true));
        assert!(!notify_os(&c, T0, false));
        c.due_at = Some(T0 + HOUR_MS);
        assert!(!notify_os(&c, T0, true));
        c.due_at = None;
        assert!(!notify_os(&c, T0, true));
    }

    #[test]
    fn budget_round_trips_and_old_files_load() {
        let mut b = ProactiveBudget::default();
        b.not_this("standup prep", T0);
        b.queue.push(cand("q topic", 0.5));
        let json = serde_json::to_string(&b).unwrap();
        let back: ProactiveBudget = serde_json::from_str(&json).unwrap();
        assert_eq!(back, b);
        let empty: ProactiveBudget = serde_json::from_str("{}").unwrap();
        assert_eq!(empty, ProactiveBudget::default());
    }
}
