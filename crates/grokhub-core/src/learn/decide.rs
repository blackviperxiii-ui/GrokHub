//! The decision engine: the one place a change the app would make on its
//! own is weighed.
//!
//! The beliefs say what is true. A learner that wants to act on one hands in
//! a [`Proposal`], and [`State::decide`] answers with one [`Verdict`]: make
//! the change, offer it, wait, or never. Every decider gets the same terms in
//! the same order, one shared daily budget and one log of what was decided
//! and why.
//!
//! Three guards live here:
//!
//! - Shadow: a rule may not act until the user has said yes to its offer
//!   [`SHADOW_PASSES`] times in a row. Until then it only offers.
//! - The dial: each [`Area`] is set to watch, offer or act.
//! - The watchdog: [`WATCH_UNDOS`] undos inside [`WATCH_WINDOW_MS`] pause all
//!   acting for [`PAUSE_MS`] and name the changes to put back.
//!
//! This only narrows. An `Act` here still goes through the caller's own gate
//! (the harness for a tool step, auto setup's store for a setting), and a
//! `hard` proposal can never come out as `Act`.

use std::collections::{BTreeMap, VecDeque};
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::ladder::{step, Step, Undo, ACT_AT};
use super::score::{Board, Trend};
use super::signal::Subject;
use super::store;

pub const FILE: &str = "decisions.json";
/// Changes the app may make on its own in one day, across every area.
pub const ACTS_PER_DAY: u32 = 5;
/// Yes answers in a row a rule needs before it may act.
pub const SHADOW_PASSES: u32 = 3;
/// Undos of one rule's changes that turn the rule off.
pub const UNDOS_TURN_OFF: u32 = 2;
/// How long a rule rests after the user says no to its offer.
pub const REST_MS: u64 = 30 * DAY_MS;
/// Undos inside [`WATCH_WINDOW_MS`] that trip the watchdog.
pub const WATCH_UNDOS: usize = 2;
pub const WATCH_WINDOW_MS: u64 = DAY_MS;
/// How long the watchdog holds acting back.
pub const PAUSE_MS: u64 = 3 * DAY_MS;
/// Decisions kept in the log. One more drops the oldest.
pub const LOG_CAP: usize = 200;
/// Rules kept. One more drops the one decided longest ago.
pub const RULE_CAP: usize = 500;
const DAY_MS: u64 = 24 * 60 * 60 * 1_000;

/// A part of the app that can change something on its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Area {
    Settings,
    Skills,
    Router,
    LocalAi,
    Proactive,
    Chips,
}

impl Area {
    pub const ALL: [Area; 6] = [
        Area::Settings,
        Area::Skills,
        Area::Router,
        Area::LocalAi,
        Area::Proactive,
        Area::Chips,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Settings => "settings",
            Self::Skills => "skills",
            Self::Router => "router",
            Self::LocalAi => "local-ai",
            Self::Proactive => "proactive",
            Self::Chips => "chips",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|a| a.as_str().eq_ignore_ascii_case(text))
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Settings => "Settings",
            Self::Skills => "Skills",
            Self::Router => "Model choice",
            Self::LocalAi => "Local AI",
            Self::Proactive => "Doing things for you",
            Self::Chips => "Quick chips",
        }
    }
}

/// How far an area may go.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Dial {
    /// Keep learning, show nothing.
    Watch,
    /// Offer changes, never make one.
    Offer,
    /// Make a change when every term holds.
    #[default]
    Act,
}

impl Dial {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Watch => "watch",
            Self::Offer => "offer",
            Self::Act => "act",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        [Self::Watch, Self::Offer, Self::Act]
            .into_iter()
            .find(|d| d.as_str().eq_ignore_ascii_case(text))
    }
}

/// Something the user wants from the app. Habits say what the user does;
/// goals say what they are after, and settle a tie between two likely
/// changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Goal {
    Cheapest,
    Fastest,
    Quiet,
    BestQuality,
}

impl Goal {
    pub const ALL: [Goal; 4] = [
        Goal::Cheapest,
        Goal::Fastest,
        Goal::Quiet,
        Goal::BestQuality,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cheapest => "cheapest",
            Self::Fastest => "fastest",
            Self::Quiet => "quiet",
            Self::BestQuality => "best-quality",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|g| g.as_str().eq_ignore_ascii_case(text))
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Cheapest => "Lowest cost",
            Self::Fastest => "Fastest answers",
            Self::Quiet => "Fewest interruptions",
            Self::BestQuality => "Best quality",
        }
    }
}

/// How much the user's first goal lifts a proposal that serves it. Each
/// later goal lifts half as much as the one before.
pub const GOAL_LIFT: f32 = 0.5;

/// One change a learner would like to make.
#[derive(Debug, Clone, PartialEq)]
pub struct Proposal {
    pub area: Area,
    /// The kind of change, as an id (`rule:settings/text-size`). Shadow
    /// passes, undos and rests are counted per rule.
    pub rule: Subject,
    pub confidence: f32,
    /// Sightings behind the belief, after fading.
    pub evidence: f32,
    pub undo: Undo,
    /// How much it helps, 0 to 1. Ranks proposals against each other.
    pub value: f32,
    /// It deletes, spends, sends something off the PC, or touches a
    /// permission or a sign-in. Always asks.
    pub hard: bool,
    /// The goal this change serves, if it serves one.
    pub helps: Option<Goal>,
    /// The goal this change works against, if any.
    pub hurts: Option<Goal>,
    /// The change has had its trial another way: the learner tried it on a
    /// share of real work and it held (the router's canary), or it comes
    /// from a built-in rule that reads the PC (auto setup). It does not
    /// wait for three yes answers first.
    pub tested: bool,
}

/// What is going on right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Situation {
    pub quiet: bool,
    pub busy: bool,
    pub halted: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    /// Make the change and say so.
    Act,
    /// Put it in front of the user.
    Offer,
    /// Not now. Ask again later.
    Wait,
    /// This rule is off.
    Never,
}

/// The first term that settled the verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    /// Every term held.
    Sure,
    TurnedOff,
    DialWatch,
    Resting,
    NotSureYet,
    AlwaysAsks,
    NoUndo,
    SureEnoughToOffer,
    DialOffer,
    Slipped,
    Halted,
    Quiet,
    Busy,
    Paused,
    Shadow,
    Budget,
}

impl Reason {
    /// The reason as the user reads it.
    pub fn why(self) -> &'static str {
        match self {
            Self::Sure => "It was sure, you said yes to this before, and it can be undone.",
            Self::TurnedOff => "You undid this twice, so it stopped.",
            Self::DialWatch => "This area is set to watch only.",
            Self::Resting => "You said no to this, so it waits a month.",
            Self::NotSureYet => "It hasn't seen this often enough yet.",
            Self::AlwaysAsks => {
                "It deletes, spends, sends or changes a permission, so it always asks."
            }
            Self::NoUndo => "It can't be undone, so it asks.",
            Self::SureEnoughToOffer => "It's sure enough to offer, not to do it.",
            Self::DialOffer => "This area is set to offer only.",
            Self::Slipped => "Its changes went worse lately, so this area asks for a month.",
            Self::Halted => "Halt is on.",
            Self::Quiet => "It's quiet hours.",
            Self::Busy => "You're busy right now.",
            Self::Paused => "Too much was undone lately, so it asks first for a few days.",
            Self::Shadow => "It's new at this, so it asks until you've said yes three times.",
            Self::Budget => "It already made 5 changes on its own today.",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Decision {
    pub at_ms: u64,
    pub area: Area,
    pub rule: Subject,
    pub verdict: Verdict,
    pub reason: Reason,
}

/// What became of an offer or of a change the app made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Yes to an offer.
    Accepted,
    /// No to an offer.
    Declined,
    /// A change the app made was left in place.
    Kept,
    /// A change the app made was undone.
    Undone,
    /// A change the app made went worse and the app took it back itself.
    /// It counts against the area's score, not as the user's undo.
    RolledBack,
}

/// The watchdog tripped: acting is paused, and these rules' changes from
/// the last day should be put back, newest first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Alarm {
    pub paused_until_ms: u64,
    pub put_back: Vec<Subject>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
struct Rule {
    /// Yes answers in a row.
    passes: u32,
    acted: u32,
    kept: u32,
    undone: u32,
    resting_until_ms: u64,
    last_ms: u64,
    /// Where its proposals come from, so its outcomes reach the scoreboard.
    area: Option<Area>,
    /// The last verdict logged, so the same answer isn't logged twice.
    last: Option<(Verdict, Reason)>,
}

/// Everything the decision engine remembers. One small file.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    dials: BTreeMap<Area, Dial>,
    rules: BTreeMap<Subject, Rule>,
    day: u64,
    acts_today: u32,
    paused_until_ms: u64,
    /// Changes the app made in the last day: when, and by which rule.
    recent_acts: VecDeque<(u64, Subject)>,
    recent_undos: VecDeque<u64>,
    log: VecDeque<Decision>,
    score: Board,
    /// What matters most to the user, first to last. Theirs, like the dials.
    goals: Vec<Goal>,
    /// What was tried on purpose, and when ([`super::experiment`]).
    tries: super::experiment::Tries,
}

impl State {
    pub fn dial(&self, area: Area) -> Dial {
        self.dials.get(&area).copied().unwrap_or_default()
    }

    pub fn set_dial(&mut self, area: Area, dial: Dial) {
        self.dials.insert(area, dial);
    }

    /// The log, oldest first.
    pub fn log(&self) -> impl DoubleEndedIterator<Item = &Decision> {
        self.log.iter()
    }

    pub fn paused(&self, now_ms: u64) -> bool {
        now_ms < self.paused_until_ms
    }

    /// Forgets every rule, count and log line. The dials are the user's own
    /// settings and stay.
    pub fn forget_learned(&mut self) {
        *self = Self {
            dials: std::mem::take(&mut self.dials),
            goals: std::mem::take(&mut self.goals),
            ..Self::default()
        };
    }

    /// The user handed this kind of change back to the app: its undos and
    /// its rest are forgotten, so it may be weighed afresh.
    pub fn forgive(&mut self, rule: &Subject) {
        self.rules.remove(rule);
    }

    /// The user's "carry on" after a watchdog pause.
    pub fn resume(&mut self) {
        self.paused_until_ms = 0;
        self.recent_undos.clear();
        self.score.resume();
    }

    /// What matters most to the user, first to last.
    pub fn goals(&self) -> &[Goal] {
        &self.goals
    }

    /// Sets the goals in the user's order. A goal named twice counts once.
    pub fn set_goals(&mut self, goals: &[Goal]) {
        self.goals.clear();
        for goal in goals {
            if !self.goals.contains(goal) {
                self.goals.push(*goal);
            }
        }
    }

    /// How much `goal` moves a proposal's worth: the first goal by
    /// [`GOAL_LIFT`], each later one by half the one before, none if the
    /// user didn't name it.
    fn pull(&self, goal: Option<Goal>) -> f32 {
        goal.and_then(|g| self.goals.iter().position(|have| *have == g))
            .map_or(0.0, |at| GOAL_LIFT / (1u32 << at) as f32)
    }

    /// A proposal's place in the queue for the day's budget: how much it
    /// helps and how sure it is, lifted by a goal it serves and lowered by
    /// one it works against.
    pub fn worth(&self, p: &Proposal) -> f32 {
        let base = p.value.clamp(0.0, 1.0) * p.confidence.clamp(0.0, 1.0);
        base * (1.0 + self.pull(p.helps) - self.pull(p.hurts)).max(0.0)
    }

    /// The scoreboard: how each area's decisions went, week by week.
    pub fn score(&self) -> &Board {
        &self.score
    }

    /// Changes left today.
    pub fn budget_left(&self, now_ms: u64) -> u32 {
        let used = if self.day == now_ms / DAY_MS {
            self.acts_today
        } else {
            0
        };
        ACTS_PER_DAY.saturating_sub(used)
    }

    /// Whether `rule` has earned the right to act.
    /// Whether `rule` may be today's experiment in `area`: the dial is on
    /// act, nothing holds the app back, the rule isn't resting or off, and
    /// the day's one try is free or already this rule's.
    pub fn try_once(&mut self, area: Area, rule: &Subject, now: Situation, now_ms: u64) -> bool {
        let held = self
            .rules
            .get(rule)
            .is_some_and(|r| r.undone >= UNDOS_TURN_OFF || now_ms < r.resting_until_ms);
        let clear = self.dial(area) == Dial::Act
            && !held
            && !(now.halted || now.quiet || now.busy)
            && !self.paused(now_ms)
            && !self.score.resting(area, now_ms);
        self.tries.allow(rule, clear, now_ms)
    }

    pub fn graduated(&self, rule: &Subject) -> bool {
        self.rules
            .get(rule)
            .is_some_and(|r| r.passes >= SHADOW_PASSES)
    }

    fn settle(&self, p: &Proposal, now: Situation, now_ms: u64) -> (Verdict, Reason) {
        let rule = self.rules.get(&p.rule);
        if rule.is_some_and(|r| r.undone >= UNDOS_TURN_OFF) {
            return (Verdict::Never, Reason::TurnedOff);
        }
        let dial = self.dial(p.area);
        if dial == Dial::Watch {
            return (Verdict::Wait, Reason::DialWatch);
        }
        if rule.is_some_and(|r| now_ms < r.resting_until_ms) {
            return (Verdict::Wait, Reason::Resting);
        }
        let undo = if p.hard { Undo::Never } else { p.undo };
        match step(p.confidence, p.evidence, undo) {
            Step::Watch => return (Verdict::Wait, Reason::NotSureYet),
            Step::Suggest if p.hard => return (Verdict::Offer, Reason::AlwaysAsks),
            Step::Suggest if undo == Undo::Never && p.confidence >= ACT_AT => {
                return (Verdict::Offer, Reason::NoUndo)
            }
            Step::Suggest => return (Verdict::Offer, Reason::SureEnoughToOffer),
            Step::Act => {}
        }
        let held = [
            (dial == Dial::Offer, Verdict::Offer, Reason::DialOffer),
            (
                self.score.resting(p.area, now_ms),
                Verdict::Offer,
                Reason::Slipped,
            ),
            (now.halted, Verdict::Wait, Reason::Halted),
            (now.quiet, Verdict::Wait, Reason::Quiet),
            (now.busy, Verdict::Wait, Reason::Busy),
            (self.paused(now_ms), Verdict::Offer, Reason::Paused),
            (
                !p.tested && !self.graduated(&p.rule),
                Verdict::Offer,
                Reason::Shadow,
            ),
            (
                self.budget_left(now_ms) == 0,
                Verdict::Offer,
                Reason::Budget,
            ),
        ];
        held.into_iter()
            .find(|(on, _, _)| *on)
            .map_or((Verdict::Act, Reason::Sure), |(_, v, r)| (v, r))
    }

    /// Weighs one proposal. An `Act` spends one of the day's changes.
    pub fn decide(&mut self, p: &Proposal, now: Situation, now_ms: u64) -> Decision {
        self.score.check(p.area, now_ms);
        let (verdict, reason) = self.settle(p, now, now_ms);
        if !self.rules.contains_key(&p.rule) && self.rules.len() >= RULE_CAP {
            let oldest = self
                .rules
                .iter()
                .min_by_key(|(_, r)| r.last_ms)
                .map(|(k, _)| k.clone());
            if let Some(key) = oldest {
                self.rules.remove(&key);
            }
        }
        let rule = self.rules.entry(p.rule.clone()).or_default();
        rule.last_ms = now_ms;
        rule.area = Some(p.area);
        let decision = Decision {
            at_ms: now_ms,
            area: p.area,
            rule: p.rule.clone(),
            verdict,
            reason,
        };
        if verdict == Verdict::Act {
            rule.acted += 1;
            let day = now_ms / DAY_MS;
            if self.day != day {
                self.day = day;
                self.acts_today = 0;
            }
            self.acts_today += 1;
            self.recent_acts.push_back((now_ms, p.rule.clone()));
        }
        // An act is logged every time; any other answer only when it changes.
        if verdict == Verdict::Act || rule.last != Some((verdict, reason)) {
            rule.last = Some((verdict, reason));
            self.log.push_back(decision.clone());
            while self.log.len() > LOG_CAP {
                self.log.pop_front();
            }
        }
        self.trim(now_ms);
        decision
    }

    /// Weighs several proposals, the most helpful first, so the day's budget
    /// goes to the best of them. Answers come back in that order.
    pub fn decide_all(
        &mut self,
        mut proposals: Vec<Proposal>,
        now: Situation,
        now_ms: u64,
    ) -> Vec<Decision> {
        proposals.sort_by(|a, b| {
            self.worth(b)
                .total_cmp(&self.worth(a))
                .then_with(|| a.rule.cmp(&b.rule))
        });
        proposals
            .iter()
            .map(|p| self.decide(p, now, now_ms))
            .collect()
    }

    /// Takes in what became of an offer or a change. An undo that trips the
    /// watchdog returns the [`Alarm`].
    pub fn outcome(&mut self, rule: &Subject, outcome: Outcome, now_ms: u64) -> Option<Alarm> {
        self.trim(now_ms);
        let r = self.rules.entry(rule.clone()).or_default();
        r.last_ms = now_ms;
        if let Some(area) = r.area {
            let good = matches!(outcome, Outcome::Accepted | Outcome::Kept);
            self.score.note(area, good, now_ms);
        }
        match outcome {
            Outcome::Accepted => r.passes += 1,
            Outcome::Declined => {
                r.passes = 0;
                r.resting_until_ms = now_ms + REST_MS;
            }
            Outcome::Kept => r.kept += 1,
            Outcome::RolledBack => r.passes = 0,
            Outcome::Undone => {
                r.undone += 1;
                r.passes = 0;
                self.recent_acts.retain(|(_, key)| key != rule);
                self.recent_undos.push_back(now_ms);
                if self.recent_undos.len() >= WATCH_UNDOS && !self.paused(now_ms) {
                    self.paused_until_ms = now_ms + PAUSE_MS;
                    return Some(Alarm {
                        paused_until_ms: self.paused_until_ms,
                        put_back: self
                            .recent_acts
                            .iter()
                            .rev()
                            .map(|(_, key)| key.clone())
                            .collect(),
                    });
                }
            }
        }
        None
    }

    fn trim(&mut self, now_ms: u64) {
        let fresh = |at: u64| now_ms.saturating_sub(at) < WATCH_WINDOW_MS;
        self.recent_acts.retain(|(at, _)| fresh(*at));
        self.recent_undos.retain(|at| fresh(*at));
    }
}

/// The saved state. A missing or unreadable file reads as new.
pub fn load(config_dir: &Path) -> State {
    std::fs::read_to_string(store::dir(config_dir).join(FILE))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// Reads the state, runs `change` on it and writes it back when it changed,
/// all under the engine's lock, so two apps on one folder share one budget.
pub fn with_state<R>(config_dir: &Path, change: impl FnOnce(&mut State) -> R) -> R {
    let _lock = store::hold_lock(config_dir);
    let mut state = load(config_dir);
    let before = state.clone();
    let out = change(&mut state);
    if state != before {
        if let Ok(text) = serde_json::to_string(&state) {
            // A full disk must not stall the caller: it already has its answer.
            let _ = crate::pulse_writer::write_atomic(&store::dir(config_dir).join(FILE), &text);
        }
    }
    out
}

pub const NO_DECISIONS: &str =
    "No decisions yet. They show up here once the app has something it could change for you.";
pub const NO_GOALS: &str = "Nothing set. Name what matters with `/learned goals`.";
pub const GOALS_USAGE: &str = "Use `/learned goals <goal> [<goal> ...]`, first is most important, or `/learned goals none`. Goals: cheapest, fastest, quiet, best-quality.";
pub const TRUST_USAGE: &str = "Use `/learned trust <area> <watch|offer|act>`. Areas: settings, skills, router, local-ai, proactive, chips.";

pub(super) fn verdict_words(v: Verdict) -> &'static str {
    match v {
        Verdict::Act => "Did it",
        Verdict::Offer => "Asked",
        Verdict::Wait => "Held back",
        Verdict::Never => "Stopped",
    }
}

/// The dials and the newest decisions as Markdown, for the cabin's chat.
pub fn text(state: &State, now_ms: u64) -> String {
    let mut out = String::from("**How far each area may go**\n");
    for area in Area::ALL {
        out.push_str(&format!(
            "- {}: {} `{}`\n",
            area.label(),
            state.dial(area).as_str(),
            area.as_str()
        ));
    }
    out.push_str("\n**What matters most to you**\n");
    if state.goals.is_empty() {
        out.push_str(NO_GOALS);
        out.push('\n');
    }
    for (at, goal) in state.goals.iter().enumerate() {
        out.push_str(&format!(
            "{}. {} `{}`\n",
            at + 1,
            goal.label(),
            goal.as_str()
        ));
    }
    if state.paused(now_ms) {
        out.push_str("\nToo much was undone lately, so everything asks first for now.\n");
    }
    let scored: Vec<String> = Area::ALL
        .into_iter()
        .filter_map(|area| {
            let score = state.score.score(area, now_ms);
            let now = score.now?;
            let was = match (score.trend, score.before) {
                (Trend::TooEarly, _) | (_, None) => String::new(),
                (Trend::Better, Some(b)) => format!(", up from {b}%"),
                (Trend::Worse, Some(b)) => format!(", down from {b}%"),
                (Trend::Steady, Some(b)) => format!(", about the same as before ({b}%)"),
            };
            Some(format!("- {}: {now}% went your way{was}\n", area.label()))
        })
        .collect();
    if !scored.is_empty() {
        out.push_str("\n**The last four weeks**\n");
        out.extend(scored);
    }
    out.push_str("\n**Latest decisions**\n");
    if state.log.is_empty() {
        out.push_str(NO_DECISIONS);
        out.push('\n');
    }
    for d in state.log.iter().rev().take(15) {
        out.push_str(&format!(
            "- {}: {} `{}`\n",
            verdict_words(d.verdict),
            d.reason.why(),
            d.rule.as_str()
        ));
    }
    out.push('\n');
    out.push_str(TRUST_USAGE);
    out
}
