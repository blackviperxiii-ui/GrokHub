//! Brain 4: the background task governor.
//!
//! One scheduler for everything that is not the user's current turn: proactive
//! cards and anticipate turns, the Pulse refresh and its digest, the ideas ask,
//! the nightly review, self-review and dream, memory retention, idle reflect,
//! the local indexers, the model list refresh, and scheduled automations and
//! loops. Each job registers once ([`cabin_jobs`]) with a kind, a priority, a
//! heavy flag, a budget class, a cadence and a quiet-hours rule. The 15s pulse
//! (`crate::heartbeat`) is only the governor's clock: on each beat
//! [`Governor::tick`] hands back one queue of due jobs, highest priority first,
//! and the cabin runs exactly that queue.
//!
//! What it owns:
//! - **Cadence.** A job runs at most once per due time; its next due time is set
//!   when it is handed out, so a second tick at the same moment returns nothing.
//! - **Quiet hours.** A job whose rule is [`QuietRule::Holds`] waits them out.
//! - **Halt.** Only jobs registered to run while halted (local upkeep) run.
//! - **Per-day token budgets.** A [`BudgetClass::Tokens`] job is charged what it
//!   spent ([`Governor::charge`]). Once today's spend reaches its budget the job
//!   stops until the local day changes, and one named note says so.
//! - **Preemption.** While you're active ([`user_active`]: typing, a draft in
//!   the composer, a key or click in the last [`ACTIVE_WINDOW_MS`], or a chat
//!   run going) the cabin sets [`TickCtx::hold_heavy`]: every heavy job waits
//!   and runs on the first idle beat, and light model jobs keep running on the
//!   router's cost goal ([`ACTIVE_GOAL`], reason [`ACTIVE_REASON`]).
//!
//! Pure and clock-injected: every call takes the time and the local day, so
//! tests drive a fake clock. The state is in memory; a relaunch starts a fresh
//! day of budget.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::heartbeat::HEARTBEAT_MS;
use crate::organs::quiet_hours_active;

/// Once-a-day jobs check their day gate this often.
const DAILY_CHECK_MS: u64 = 60_000;

/// How long after your last key or click the cabin still counts you as active.
pub const ACTIVE_WINDOW_MS: u64 = 20_000;

/// The router goal light model jobs run on while you're active.
pub const ACTIVE_GOAL: &str = "cost";
/// The reason the router logs with [`ACTIVE_GOAL`].
pub const ACTIVE_REASON: &str = "user active";

/// You're active: a chat run is going, the composer holds a draft, or your last
/// key or click was under [`ACTIVE_WINDOW_MS`] ago.
pub fn user_active(run_active: bool, composer_has_text: bool, since_input_ms: u64) -> bool {
    run_active || composer_has_text || since_input_ms < ACTIVE_WINDOW_MS
}

/// Every kind of background job. Each one has exactly one registration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum JobKind {
    /// Grok `/loop` intervals and clock-time cabin automations (night jobs).
    Automations,
    /// Feed upkeep: idea expiry, quiet release, the digest clock. No model call.
    PulseRefresh,
    /// The model list refresh and its guard.
    ModelRegistry,
    /// Spike-6a/6b proactive cards and admitted auto-acts.
    Proactive,
    /// The automatic ideas ask.
    Ideas,
    /// A follow-skill anticipate turn in the chat.
    Anticipate,
    /// The digest lookup and Pulse source fetches.
    Digest,
    /// The nightly review model call and session suggestions.
    Review,
    /// The night passes: Sunday systems briefing, self-review, lessons tidy, and AMR dream.
    NightPasses,
    /// AMR memory retention, once a local day.
    MemoryRetention,
    /// Idle reflect: fact extraction into memory.
    Reflect,
    /// The local indexers (Spike-8a).
    Indexers,
    /// Dreambeans daily drop: local pick from ranked Pulse sources once a local night.
    DailyDrop,
}

impl JobKind {
    pub const ALL: [JobKind; 13] = [
        Self::Automations,
        Self::PulseRefresh,
        Self::ModelRegistry,
        Self::Proactive,
        Self::Ideas,
        Self::Anticipate,
        Self::Digest,
        Self::Review,
        Self::NightPasses,
        Self::MemoryRetention,
        Self::DailyDrop,
        Self::Reflect,
        Self::Indexers,
    ];

    /// The `app.json` key and trace tag.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Automations => "automations",
            Self::PulseRefresh => "pulse",
            Self::ModelRegistry => "model_registry",
            Self::Proactive => "proactive",
            Self::Ideas => "ideas",
            Self::Anticipate => "anticipate",
            Self::Digest => "digest",
            Self::Review => "review",
            Self::NightPasses => "night_passes",
            Self::MemoryRetention => "memory_retention",
            Self::Reflect => "reflect",
            Self::Indexers => "indexers",
            Self::DailyDrop => "daily_drop",
        }
    }

    /// The name a person reads in a note.
    pub fn label(self) -> &'static str {
        match self {
            Self::Automations => "Automations and loops",
            Self::PulseRefresh => "Pulse refresh",
            Self::ModelRegistry => "Model list refresh",
            Self::Proactive => "Proactive cards",
            Self::Ideas => "Ideas",
            Self::Anticipate => "Anticipate",
            Self::Digest => "Pulse digest",
            Self::Review => "Nightly review",
            Self::NightPasses => "Systems briefing, self-review, and dream",
            Self::MemoryRetention => "Memory retention",
            Self::Reflect => "Idle reflect",
            Self::Indexers => "Indexers",
            Self::DailyDrop => "Daily drop",
        }
    }

    pub fn parse(tag: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == tag)
    }
}

/// Whether a job spends model tokens against a per-day budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetClass {
    /// Local work: no model call, no budget.
    Local,
    /// Model calls, charged per local day.
    Tokens,
}

/// What a job does in quiet hours.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuietRule {
    /// Runs; anything it posts is held by the job itself until quiet hours end.
    Runs,
    /// Does not start until quiet hours end.
    Holds,
}

/// One registration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JobSpec {
    pub kind: JobKind,
    /// Higher runs first in a tick's queue. Equal priorities keep registration order.
    pub priority: u8,
    /// Long or disk- and CPU-heavy work that user activity may hold.
    pub heavy: bool,
    pub budget: BudgetClass,
    /// The gap between due times.
    pub every_ms: u64,
    pub quiet: QuietRule,
    /// Runs while Halt holds the pulse (local upkeep only).
    pub while_halted: bool,
}

/// Every background job the cabin runs, registered once. The order of this
/// table is the tie-break for equal priorities.
pub fn cabin_jobs() -> Vec<JobSpec> {
    use BudgetClass::{Local, Tokens};
    use QuietRule::{Holds, Runs};
    let job = |kind, priority, heavy, budget, every_ms, quiet, while_halted| JobSpec {
        kind,
        priority,
        heavy,
        budget,
        every_ms,
        quiet,
        while_halted,
    };
    vec![
        // Scheduled work the user asked for comes first, and before the review
        // so the review can wait for a run that just fired.
        job(
            JobKind::Automations,
            90,
            true,
            Tokens,
            HEARTBEAT_MS,
            Runs,
            false,
        ),
        job(
            JobKind::PulseRefresh,
            80,
            false,
            Local,
            HEARTBEAT_MS,
            Runs,
            true,
        ),
        job(
            JobKind::ModelRegistry,
            75,
            false,
            Local,
            HEARTBEAT_MS,
            Runs,
            true,
        ),
        job(
            JobKind::Proactive,
            70,
            false,
            Local,
            HEARTBEAT_MS,
            Runs,
            false,
        ),
        job(
            JobKind::Ideas,
            68,
            false,
            Tokens,
            HEARTBEAT_MS,
            Holds,
            false,
        ),
        // A turn in your own chat: it waits while you're active, like heavy work.
        job(
            JobKind::Anticipate,
            65,
            true,
            Tokens,
            HEARTBEAT_MS,
            Holds,
            false,
        ),
        job(
            JobKind::Digest,
            60,
            false,
            Tokens,
            HEARTBEAT_MS,
            Holds,
            false,
        ),
        job(
            JobKind::Review,
            50,
            false,
            Tokens,
            DAILY_CHECK_MS,
            Runs,
            false,
        ),
        job(
            JobKind::NightPasses,
            45,
            true,
            Tokens,
            DAILY_CHECK_MS,
            Runs,
            false,
        ),
        job(
            JobKind::MemoryRetention,
            40,
            false,
            Local,
            DAILY_CHECK_MS,
            Runs,
            false,
        ),
        job(
            JobKind::DailyDrop,
            38,
            false,
            Local,
            DAILY_CHECK_MS,
            Runs,
            false,
        ),
        job(
            JobKind::Reflect,
            30,
            false,
            Local,
            HEARTBEAT_MS,
            Runs,
            false,
        ),
        job(
            JobKind::Indexers,
            20,
            true,
            Local,
            HEARTBEAT_MS,
            Holds,
            false,
        ),
    ]
}

/// The default tokens a day for each kind that spends them.
pub fn default_daily_tokens(kind: JobKind) -> u64 {
    match kind {
        JobKind::Automations => 1_000_000,
        JobKind::Anticipate => 125_000,
        JobKind::Ideas => 50_000,
        JobKind::Digest => 75_000,
        JobKind::Review => 50_000,
        JobKind::NightPasses => 50_000,
        _ => 0,
    }
}

/// `app.json` → `"background"`. `dailyTokens` maps a kind tag to tokens a day;
/// a missing tag keeps its default, and 0 stops that kind for every day.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackgroundBudgets {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub daily_tokens: BTreeMap<String, u64>,
}

impl BackgroundBudgets {
    /// Omitted from `app.json` while it stays default.
    pub fn is_default(&self) -> bool {
        self.daily_tokens.is_empty()
    }

    /// Today's budget for `kind`: the override, else the default.
    pub fn daily_tokens(&self, kind: JobKind) -> u64 {
        self.daily_tokens
            .get(kind.as_str())
            .copied()
            .unwrap_or_else(|| default_daily_tokens(kind))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegisterError {
    /// The kind already has a registration.
    Duplicate(JobKind),
}

/// Why a due job did not run this tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobHold {
    Halted,
    Quiet,
    BudgetSpent,
    Heavy,
}

impl JobHold {
    pub fn reason(self) -> &'static str {
        match self {
            Self::Halted => "halted",
            Self::Quiet => "quiet_hours",
            Self::BudgetSpent => "budget_spent",
            Self::Heavy => "heavy_held",
        }
    }

    /// Why it waits, for Settings → Background.
    pub fn plain(self) -> &'static str {
        match self {
            Self::Halted => "Paused: Halt is on",
            Self::Quiet => "Waiting for quiet hours to end",
            Self::BudgetSpent => "Stopped for today: budget used",
            Self::Heavy => "Waiting while you're active",
        }
    }
}

/// A light model job runs on the router's cost goal while you're active.
pub fn runs_cheap(spec: &JobSpec, active: bool) -> bool {
    active && !spec.heavy && spec.budget == BudgetClass::Tokens
}

/// A note left when a kind's budget for the day is spent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GovernorNote {
    pub kind: JobKind,
    pub day: String,
    pub text: String,
}

/// The day's spend-out line for `kind`.
pub fn budget_spent_note(kind: JobKind, spent: u64, budget: u64) -> String {
    format!(
        "{} used today's background budget ({spent} of {budget} tokens), so it stops until tomorrow.",
        kind.label()
    )
}

/// One beat's inputs.
#[derive(Debug, Clone, Copy)]
pub struct TickCtx<'a> {
    pub now_ms: u64,
    /// The local day, `YYYY-MM-DD`. Budgets reset when it changes.
    pub day: &'a str,
    /// The local clock, `HH:MM`.
    pub hm: &'a str,
    pub quiet_start: &'a str,
    pub quiet_end: &'a str,
    pub halted: bool,
    /// You're active ([`user_active`]): hold every heavy job this beat, and run
    /// light model jobs on the cost goal.
    pub hold_heavy: bool,
    pub budgets: &'a BackgroundBudgets,
}

/// What one tick decided.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TickPlan {
    /// The queue: due jobs to run now, highest priority first.
    pub run: Vec<JobKind>,
    /// Due jobs that wait, and why. They stay due.
    pub held: Vec<(JobKind, JobHold)>,
    /// Quiet hours are on this beat.
    pub quiet: bool,
    /// Notes first left on this tick.
    pub new_notes: Vec<GovernorNote>,
    /// Queued light model jobs that run on the cost goal because you're active.
    pub cheap: Vec<JobKind>,
}

impl TickPlan {
    pub fn runs(&self, kind: JobKind) -> bool {
        self.run.contains(&kind)
    }
}

/// One job's line in Settings → Background.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobStatus {
    pub kind: JobKind,
    /// When it is next due (ms since the epoch); 0 before its first beat.
    pub next_due_ms: u64,
    pub spent_today: u64,
    /// Today's token budget; `None` for local work.
    pub daily_budget: Option<u64>,
    /// Why it waited on the last beat; `None` when it ran or was not due.
    pub waiting: Option<JobHold>,
}

#[derive(Debug, Clone)]
struct Slot {
    spec: JobSpec,
    next_due_ms: u64,
    spent_today: u64,
    held: Option<JobHold>,
}

#[derive(Debug, Clone, Default)]
pub struct Governor {
    slots: Vec<Slot>,
    day: String,
    notes: Vec<GovernorNote>,
}

impl Governor {
    /// A governor with every cabin job registered.
    pub fn cabin() -> Self {
        let mut g = Self::default();
        for spec in cabin_jobs() {
            // `cabin_jobs` names each kind once; a duplicate is a test failure.
            let _ = g.register(spec);
        }
        g
    }

    /// Register a job. A kind registers once; a second registration is refused.
    pub fn register(&mut self, spec: JobSpec) -> Result<(), RegisterError> {
        if self.slots.iter().any(|s| s.spec.kind == spec.kind) {
            return Err(RegisterError::Duplicate(spec.kind));
        }
        self.slots.push(Slot {
            spec,
            next_due_ms: 0,
            spent_today: 0,
            held: None,
        });
        Ok(())
    }

    /// Registered kinds, in registration order.
    pub fn kinds(&self) -> Vec<JobKind> {
        self.slots.iter().map(|s| s.spec.kind).collect()
    }

    pub fn spec(&self, kind: JobKind) -> Option<&JobSpec> {
        self.slot(kind).map(|s| &s.spec)
    }

    /// Tokens charged to `kind` today.
    pub fn spent_today(&self, kind: JobKind) -> u64 {
        self.slot(kind).map_or(0, |s| s.spent_today)
    }

    /// Today's notes, oldest first.
    pub fn notes(&self) -> &[GovernorNote] {
        &self.notes
    }

    /// Every job's line for Settings → Background, in registration order.
    pub fn status(&self, budgets: &BackgroundBudgets) -> Vec<JobStatus> {
        self.slots
            .iter()
            .map(|s| JobStatus {
                kind: s.spec.kind,
                next_due_ms: s.next_due_ms,
                spent_today: s.spent_today,
                daily_budget: (s.spec.budget == BudgetClass::Tokens)
                    .then(|| budgets.daily_tokens(s.spec.kind)),
                waiting: s.held,
            })
            .collect()
    }

    /// One beat: the queue of due jobs to run now. A job handed out is not due
    /// again until `every_ms` later. A held job keeps its due time.
    pub fn tick(&mut self, ctx: &TickCtx<'_>) -> TickPlan {
        self.roll_day(ctx.day);
        let quiet = quiet_hours_active(ctx.hm, ctx.quiet_start, ctx.quiet_end);
        let mut plan = TickPlan {
            quiet,
            ..TickPlan::default()
        };
        let mut due: Vec<usize> = (0..self.slots.len())
            .filter(|i| self.slots[*i].next_due_ms <= ctx.now_ms)
            .collect();
        // Stable: equal priorities keep registration order.
        due.sort_by_key(|i| std::cmp::Reverse(self.slots[*i].spec.priority));
        for i in due {
            let spec = self.slots[i].spec;
            if let Some(hold) = self.hold(i, ctx, quiet) {
                if hold == JobHold::BudgetSpent && self.note_for(spec.kind).is_none() {
                    let budget = ctx.budgets.daily_tokens(spec.kind);
                    let note = GovernorNote {
                        kind: spec.kind,
                        day: self.day.clone(),
                        text: budget_spent_note(spec.kind, self.slots[i].spent_today, budget),
                    };
                    self.notes.push(note.clone());
                    plan.new_notes.push(note);
                }
                self.slots[i].held = Some(hold);
                plan.held.push((spec.kind, hold));
                continue;
            }
            self.slots[i].held = None;
            self.slots[i].next_due_ms = ctx.now_ms.saturating_add(spec.every_ms);
            plan.run.push(spec.kind);
            if runs_cheap(&spec, ctx.hold_heavy) {
                plan.cheap.push(spec.kind);
            }
        }
        plan
    }

    /// Charge `tokens` a job of `kind` spent on `day`. Local jobs are not charged.
    pub fn charge(&mut self, kind: JobKind, tokens: u64, day: &str) {
        self.roll_day(day);
        if let Some(slot) = self.slots.iter_mut().find(|s| s.spec.kind == kind) {
            if slot.spec.budget == BudgetClass::Tokens {
                slot.spent_today = slot.spent_today.saturating_add(tokens);
            }
        }
    }

    fn hold(&self, i: usize, ctx: &TickCtx<'_>, quiet: bool) -> Option<JobHold> {
        let slot = &self.slots[i];
        let spec = slot.spec;
        if ctx.halted && !spec.while_halted {
            return Some(JobHold::Halted);
        }
        if quiet && spec.quiet == QuietRule::Holds {
            return Some(JobHold::Quiet);
        }
        if spec.budget == BudgetClass::Tokens
            && slot.spent_today >= ctx.budgets.daily_tokens(spec.kind)
        {
            return Some(JobHold::BudgetSpent);
        }
        if ctx.hold_heavy && spec.heavy {
            return Some(JobHold::Heavy);
        }
        None
    }

    fn note_for(&self, kind: JobKind) -> Option<&GovernorNote> {
        self.notes.iter().find(|n| n.kind == kind)
    }

    fn slot(&self, kind: JobKind) -> Option<&Slot> {
        self.slots.iter().find(|s| s.spec.kind == kind)
    }

    fn roll_day(&mut self, day: &str) {
        if self.day == day {
            return;
        }
        self.day = day.to_string();
        self.notes.clear();
        for slot in &mut self.slots {
            slot.spent_today = 0;
        }
    }
}

/// Inputs for the governor's [`JobKind::DailyDrop`] beat. Ranked rows are already on
/// this machine (no fetch, no Google APIs, no model).
#[derive(Debug)]
pub struct DailyDropRefreshCtx<'a> {
    pub settings: &'a crate::daily_drop::DailyDropSettings,
    pub ranked: &'a [crate::pulse_sources::Ranked],
    pub ledger: &'a mut crate::daily_drop::DailyDropLedger,
    /// Local calendar day (`YYYY-MM-DD`) of the last successful overnight pass.
    pub last_run_day: Option<&'a str>,
    pub today: &'a str,
    pub clock: &'a crate::organs::LocalClock,
    /// Same night hour as the nightly review (`dream_hour`).
    pub night_hour: u32,
    pub now_ms: u64,
}

/// Outcome of one daily-drop refresh attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DailyDropRefreshResult {
    pub cards: Vec<crate::update_feed::UpdateCard>,
    /// When set, persist as `last_run_day` so the job does not spin every minute overnight.
    pub mark_day: Option<String>,
}

/// Overnight refresh: calls the daily-drop picker on local ranked source rows. Off by
/// default ([`crate::daily_drop::DailyDropSettings::active`]); no external I/O.
pub fn daily_drop_refresh(ctx: DailyDropRefreshCtx<'_>) -> DailyDropRefreshResult {
    if !ctx.settings.active() {
        return DailyDropRefreshResult {
            cards: Vec::new(),
            mark_day: None,
        };
    }
    if !crate::review::review_due(ctx.last_run_day, ctx.today, ctx.clock, ctx.night_hour) {
        return DailyDropRefreshResult {
            cards: Vec::new(),
            mark_day: None,
        };
    }
    let cards = crate::daily_drop::pick_cards(ctx.settings, ctx.ranked, ctx.ledger, ctx.now_ms);
    DailyDropRefreshResult {
        cards,
        mark_day: Some(ctx.today.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: u64 = 1_800_000_000_000;
    const DAY: &str = "2026-10-11";

    fn ctx<'a>(
        now_ms: u64,
        day: &'a str,
        hm: &'a str,
        budgets: &'a BackgroundBudgets,
    ) -> TickCtx<'a> {
        TickCtx {
            now_ms,
            day,
            hm,
            quiet_start: "22:00",
            quiet_end: "07:00",
            halted: false,
            hold_heavy: false,
            budgets,
        }
    }

    #[test]
    fn the_queue_runs_in_priority_order() {
        let budgets = BackgroundBudgets::default();
        let mut g = Governor::cabin();
        let plan = g.tick(&ctx(T0, DAY, "12:00", &budgets));
        assert_eq!(
            plan.run,
            vec![
                JobKind::Automations,
                JobKind::PulseRefresh,
                JobKind::ModelRegistry,
                JobKind::Proactive,
                JobKind::Ideas,
                JobKind::Anticipate,
                JobKind::Digest,
                JobKind::Review,
                JobKind::NightPasses,
                JobKind::MemoryRetention,
                JobKind::DailyDrop,
                JobKind::Reflect,
                JobKind::Indexers,
            ]
        );
        assert!(plan.held.is_empty());
        assert!(!plan.quiet);
    }

    #[test]
    fn registration_order_breaks_a_priority_tie_and_priority_beats_order() {
        let spec = |kind, priority| JobSpec {
            kind,
            priority,
            heavy: false,
            budget: BudgetClass::Local,
            every_ms: 1_000,
            quiet: QuietRule::Runs,
            while_halted: false,
        };
        let mut g = Governor::default();
        g.register(spec(JobKind::Indexers, 10)).expect("indexers");
        g.register(spec(JobKind::Reflect, 50)).expect("reflect");
        g.register(spec(JobKind::Review, 10)).expect("review");
        g.register(spec(JobKind::Automations, 99))
            .expect("automations");
        let budgets = BackgroundBudgets::default();
        let plan = g.tick(&ctx(T0, DAY, "12:00", &budgets));
        assert_eq!(
            plan.run,
            vec![
                JobKind::Automations,
                JobKind::Reflect,
                JobKind::Indexers,
                JobKind::Review
            ]
        );
    }

    #[test]
    fn each_kind_registers_once() {
        let g = Governor::cabin();
        assert_eq!(g.kinds(), JobKind::ALL.to_vec());
        let mut again = Governor::cabin();
        let dup = cabin_jobs()[3];
        assert_eq!(dup.kind, JobKind::Proactive);
        assert_eq!(
            again.register(dup),
            Err(RegisterError::Duplicate(JobKind::Proactive))
        );
        assert_eq!(again.kinds().len(), 13);
        let mut tags: Vec<&str> = JobKind::ALL.iter().map(|k| k.as_str()).collect();
        tags.sort_unstable();
        tags.dedup();
        assert_eq!(tags.len(), 13, "tags are unique");
        assert_eq!(JobKind::parse("night_passes"), Some(JobKind::NightPasses));
        assert_eq!(JobKind::parse("daily_drop"), Some(JobKind::DailyDrop));
        assert_eq!(JobKind::parse("nope"), None);
    }

    #[test]
    fn a_tick_runs_each_kind_at_most_once_per_due_time() {
        let budgets = BackgroundBudgets::default();
        let mut g = Governor::cabin();
        assert_eq!(g.tick(&ctx(T0, DAY, "12:00", &budgets)).run.len(), 13);
        // Same moment, and just before the pulse: nothing is due again.
        assert_eq!(
            g.tick(&ctx(T0, DAY, "12:00", &budgets)).run,
            Vec::<JobKind>::new()
        );
        assert_eq!(
            g.tick(&ctx(T0 + HEARTBEAT_MS - 1, DAY, "12:00", &budgets))
                .run,
            Vec::<JobKind>::new()
        );
        // One pulse later the pulse-rate jobs are due once; the daily checks are not.
        let beat = g.tick(&ctx(T0 + HEARTBEAT_MS, DAY, "12:00", &budgets));
        assert_eq!(
            beat.run,
            vec![
                JobKind::Automations,
                JobKind::PulseRefresh,
                JobKind::ModelRegistry,
                JobKind::Proactive,
                JobKind::Ideas,
                JobKind::Anticipate,
                JobKind::Digest,
                JobKind::Reflect,
                JobKind::Indexers,
            ]
        );
        assert_eq!(
            g.tick(&ctx(T0 + HEARTBEAT_MS, DAY, "12:00", &budgets)).run,
            Vec::<JobKind>::new()
        );
        // A full day of 15s beats runs Review once a minute: 1440 times, never twice a minute.
        let mut g = Governor::cabin();
        let mut reviews = 0;
        let mut automations = 0;
        let mut now = T0;
        while now < T0 + 24 * 60 * 60 * 1000 {
            let plan = g.tick(&ctx(now, DAY, "12:00", &budgets));
            reviews += usize::from(plan.runs(JobKind::Review));
            automations += usize::from(plan.runs(JobKind::Automations));
            now += HEARTBEAT_MS;
        }
        assert_eq!(reviews, 1440);
        assert_eq!(automations, 5760);
    }

    #[test]
    fn quiet_hours_hold_the_jobs_that_wait_them_out() {
        let budgets = BackgroundBudgets::default();
        let mut g = Governor::cabin();
        let plan = g.tick(&ctx(T0, DAY, "23:30", &budgets));
        assert!(plan.quiet);
        assert_eq!(
            plan.held,
            vec![
                (JobKind::Ideas, JobHold::Quiet),
                (JobKind::Anticipate, JobHold::Quiet),
                (JobKind::Digest, JobHold::Quiet),
                (JobKind::Indexers, JobHold::Quiet),
            ]
        );
        assert!(!plan.runs(JobKind::Indexers) && !plan.runs(JobKind::Anticipate));
        assert!(plan.runs(JobKind::Automations) && plan.runs(JobKind::NightPasses));
        // Held jobs stay due: at 07:00 they run on the next beat without waiting a cadence.
        let morning = g.tick(&ctx(T0 + 1_000, DAY, "07:00", &budgets));
        assert!(!morning.quiet);
        assert_eq!(
            morning.run,
            vec![
                JobKind::Ideas,
                JobKind::Anticipate,
                JobKind::Digest,
                JobKind::Indexers
            ]
        );
        assert_eq!(JobHold::Quiet.reason(), "quiet_hours");
    }

    #[test]
    fn a_spent_budget_stops_that_kind_for_the_day_with_a_named_note() {
        let budgets = BackgroundBudgets {
            daily_tokens: BTreeMap::from([("review".to_string(), 1_000)]),
        };
        let mut g = Governor::cabin();
        assert!(g
            .tick(&ctx(T0, DAY, "12:00", &budgets))
            .runs(JobKind::Review));
        g.charge(JobKind::Review, 600, DAY);
        g.charge(JobKind::Review, 400, DAY);
        assert_eq!(g.spent_today(JobKind::Review), 1_000);
        let later = T0 + DAILY_CHECK_MS;
        let plan = g.tick(&ctx(later, DAY, "12:01", &budgets));
        assert!(!plan.runs(JobKind::Review));
        assert!(plan.held.contains(&(JobKind::Review, JobHold::BudgetSpent)));
        assert_eq!(
            plan.new_notes,
            vec![GovernorNote {
                kind: JobKind::Review,
                day: DAY.to_string(),
                text: "Nightly review used today's background budget (1000 of 1000 tokens), so it stops until tomorrow."
                    .to_string(),
            }]
        );
        // Other kinds keep running, and the note is left once.
        assert!(plan.runs(JobKind::NightPasses));
        let again = g.tick(&ctx(later + DAILY_CHECK_MS, DAY, "12:02", &budgets));
        assert!(!again.runs(JobKind::Review));
        assert_eq!(again.new_notes, Vec::<GovernorNote>::new());
        assert_eq!(g.notes().len(), 1);
        assert_eq!(g.notes()[0].kind, JobKind::Review);
        // A new local day resets the spend and clears the note.
        let next = g.tick(&ctx(
            later + 2 * DAILY_CHECK_MS,
            "2026-10-12",
            "00:01",
            &budgets,
        ));
        assert!(next.runs(JobKind::Review));
        assert_eq!(g.spent_today(JobKind::Review), 0);
        assert!(g.notes().is_empty());
    }

    #[test]
    fn local_jobs_are_never_charged_and_a_zero_budget_stops_a_kind() {
        let budgets = BackgroundBudgets {
            daily_tokens: BTreeMap::from([("anticipate".to_string(), 0)]),
        };
        let mut g = Governor::cabin();
        g.charge(JobKind::Indexers, 9_999_999, DAY);
        assert_eq!(g.spent_today(JobKind::Indexers), 0);
        let plan = g.tick(&ctx(T0, DAY, "12:00", &budgets));
        assert!(plan.runs(JobKind::Indexers));
        assert_eq!(plan.held, vec![(JobKind::Anticipate, JobHold::BudgetSpent)]);
        assert_eq!(
            plan.new_notes[0].text,
            "Anticipate used today's background budget (0 of 0 tokens), so it stops until tomorrow."
        );
    }

    #[test]
    fn halt_runs_only_local_upkeep_and_hold_heavy_holds_heavy_jobs() {
        let budgets = BackgroundBudgets::default();
        let mut g = Governor::cabin();
        let mut c = ctx(T0, DAY, "12:00", &budgets);
        c.halted = true;
        let plan = g.tick(&c);
        assert_eq!(
            plan.run,
            vec![JobKind::PulseRefresh, JobKind::ModelRegistry]
        );
        assert_eq!(plan.held.len(), 11);
        assert!(plan.held.contains(&(JobKind::Digest, JobHold::Halted)));

        let mut g = Governor::cabin();
        let mut c = ctx(T0, DAY, "12:00", &budgets);
        c.hold_heavy = true;
        let plan = g.tick(&c);
        assert_eq!(
            plan.held,
            vec![
                (JobKind::Automations, JobHold::Heavy),
                (JobKind::Anticipate, JobHold::Heavy),
                (JobKind::NightPasses, JobHold::Heavy),
                (JobKind::Indexers, JobHold::Heavy),
            ]
        );
    }

    #[test]
    fn activity_is_a_run_a_draft_or_a_key_or_click_under_twenty_seconds_ago() {
        assert!(user_active(true, false, u64::MAX));
        assert!(user_active(false, true, u64::MAX));
        assert!(user_active(false, false, 0));
        assert!(user_active(false, false, 19_999));
        assert!(!user_active(false, false, 20_000));
        assert!(!user_active(false, false, 3_600_000));
    }

    #[test]
    fn activity_holds_heavy_jobs_moves_light_model_jobs_to_the_cost_goal_and_idle_resumes() {
        let budgets = BackgroundBudgets::default();
        let mut g = Governor::cabin();
        let mut busy = ctx(T0, DAY, "12:00", &budgets);
        busy.hold_heavy = true;
        let plan = g.tick(&busy);
        assert_eq!(
            plan.held,
            vec![
                (JobKind::Automations, JobHold::Heavy),
                (JobKind::Anticipate, JobHold::Heavy),
                (JobKind::NightPasses, JobHold::Heavy),
                (JobKind::Indexers, JobHold::Heavy),
            ]
        );
        assert_eq!(
            plan.run,
            vec![
                JobKind::PulseRefresh,
                JobKind::ModelRegistry,
                JobKind::Proactive,
                JobKind::Ideas,
                JobKind::Digest,
                JobKind::Review,
                JobKind::MemoryRetention,
                JobKind::DailyDrop,
                JobKind::Reflect,
            ]
        );
        // Light jobs that call a model run on the cost goal; local upkeep is untouched.
        assert_eq!(
            plan.cheap,
            vec![JobKind::Ideas, JobKind::Digest, JobKind::Review]
        );
        assert_eq!((ACTIVE_GOAL, ACTIVE_REASON), ("cost", "user active"));
        let rows = g.status(&budgets);
        assert_eq!(rows[0].kind, JobKind::Automations);
        assert_eq!(rows[0].waiting, Some(JobHold::Heavy));
        assert_eq!(rows[0].next_due_ms, 0, "a held job stays due");
        assert_eq!(rows[4].kind, JobKind::Ideas);
        assert_eq!(rows[4].waiting, None);
        assert_eq!(rows[4].next_due_ms, T0 + HEARTBEAT_MS);
        assert_eq!(JobHold::Heavy.plain(), "Waiting while you're active");
        // Still active a second later: the held jobs keep waiting, nothing light runs twice.
        let mut still = ctx(T0 + 1_000, DAY, "12:00", &budgets);
        still.hold_heavy = true;
        let again = g.tick(&still);
        assert_eq!(again.run, Vec::<JobKind>::new());
        assert_eq!(again.held.len(), 4);
        // Idle: the held jobs run on the very next beat, once, at full goal.
        let idle = g.tick(&ctx(T0 + 2_000, DAY, "12:00", &budgets));
        assert_eq!(
            idle.run,
            vec![
                JobKind::Automations,
                JobKind::Anticipate,
                JobKind::NightPasses,
                JobKind::Indexers
            ]
        );
        assert!(idle.held.is_empty());
        assert_eq!(idle.cheap, Vec::<JobKind>::new());
        assert_eq!(g.status(&budgets)[0].waiting, None);
        assert_eq!(
            g.tick(&ctx(T0 + 2_000, DAY, "12:00", &budgets)).run,
            Vec::<JobKind>::new()
        );
        // One pulse after the first beat only the light pulse-rate jobs are due; the
        // resumed ones keep their own cadence from when they ran.
        assert_eq!(
            g.tick(&ctx(T0 + HEARTBEAT_MS, DAY, "12:00", &budgets)).run,
            vec![
                JobKind::PulseRefresh,
                JobKind::ModelRegistry,
                JobKind::Proactive,
                JobKind::Ideas,
                JobKind::Digest,
                JobKind::Reflect,
            ]
        );
        assert_eq!(
            g.tick(&ctx(T0 + 2_000 + HEARTBEAT_MS, DAY, "12:00", &budgets))
                .run,
            vec![JobKind::Automations, JobKind::Anticipate, JobKind::Indexers]
        );
    }

    #[test]
    fn quiet_hours_and_spent_budgets_win_over_activity_and_settings_rows_say_why() {
        let budgets = BackgroundBudgets {
            daily_tokens: BTreeMap::from([("review".to_string(), 1_000)]),
        };
        let mut g = Governor::cabin();
        g.charge(JobKind::Review, 1_000, DAY);
        let mut night = ctx(T0, DAY, "23:30", &budgets);
        night.hold_heavy = true;
        let plan = g.tick(&night);
        assert_eq!(
            plan.held,
            vec![
                (JobKind::Automations, JobHold::Heavy),
                (JobKind::Ideas, JobHold::Quiet),
                (JobKind::Anticipate, JobHold::Quiet),
                (JobKind::Digest, JobHold::Quiet),
                (JobKind::Review, JobHold::BudgetSpent),
                (JobKind::NightPasses, JobHold::Heavy),
                (JobKind::Indexers, JobHold::Quiet),
            ]
        );
        assert_eq!(plan.cheap, Vec::<JobKind>::new());
        assert_eq!(
            plan.new_notes[0].text,
            "Nightly review used today's background budget (1000 of 1000 tokens), so it stops until tomorrow."
        );
        let rows = g.status(&budgets);
        let row = |k: JobKind| rows.iter().find(|r| r.kind == k).cloned().expect("row");
        assert_eq!(
            row(JobKind::Review),
            JobStatus {
                kind: JobKind::Review,
                next_due_ms: 0,
                spent_today: 1_000,
                daily_budget: Some(1_000),
                waiting: Some(JobHold::BudgetSpent),
            }
        );
        assert_eq!(row(JobKind::Digest).daily_budget, Some(75_000));
        assert_eq!(row(JobKind::Indexers).daily_budget, None);
        assert_eq!(row(JobKind::Indexers).waiting, Some(JobHold::Quiet));
        assert_eq!(row(JobKind::Proactive).waiting, None);
        assert_eq!(
            [
                JobHold::Quiet.plain(),
                JobHold::BudgetSpent.plain(),
                JobHold::Halted.plain()
            ],
            [
                "Waiting for quiet hours to end",
                "Stopped for today: budget used",
                "Paused: Halt is on"
            ]
        );
    }

    #[test]
    fn budgets_parse_and_default_omits_the_key() {
        let empty: BackgroundBudgets = serde_json::from_str("{}").expect("empty");
        assert!(empty.is_default());
        assert_eq!(empty.daily_tokens(JobKind::Automations), 1_000_000);
        assert_eq!(empty.daily_tokens(JobKind::Review), 50_000);
        assert_eq!(empty.daily_tokens(JobKind::Anticipate), 125_000);
        assert_eq!(empty.daily_tokens(JobKind::NightPasses), 50_000);
        assert_eq!(serde_json::to_string(&empty).expect("ser"), "{}");
        let b: BackgroundBudgets =
            serde_json::from_str(r#"{"dailyTokens":{"digest":5000}}"#).expect("override");
        assert_eq!(b.daily_tokens(JobKind::Digest), 5_000);
        assert_eq!(b.daily_tokens(JobKind::Ideas), 50_000);
        assert!(!b.is_default());
    }

    #[test]
    fn daily_drop_refresh_is_off_until_settings_are_active() {
        use crate::daily_drop::{DailyDropLedger, DailyDropSettings};
        use crate::organs::LocalClock;
        use crate::pulse_sources::{Ranked, SourceItem, SourceKind};

        let today = "2026-10-11";
        let clock = LocalClock {
            now_ms: T0,
            weekday: 6,
            hour: 22,
            minute: 0,
        };
        let item = SourceItem {
            kind: SourceKind::HackerNews,
            source_name: "HN".into(),
            title: "Story".into(),
            url: "https://news.ycombinator.com/item?id=1".into(),
            published_ms: T0,
            summary: String::new(),
            image: None,
            topic: None,
            engagement: 0,
        };
        let ranked = vec![Ranked {
            item,
            score: 90,
            because: None,
        }];
        let mut ledger = DailyDropLedger::default();
        let off = daily_drop_refresh(DailyDropRefreshCtx {
            settings: &DailyDropSettings::default(),
            ranked: &ranked,
            ledger: &mut ledger,
            last_run_day: None,
            today,
            clock: &clock,
            night_hour: 21,
            now_ms: T0,
        });
        assert!(off.cards.is_empty());
        assert!(off.mark_day.is_none());
        assert_eq!(ledger.picked_today, 0);

        let on_settings = DailyDropSettings {
            enabled: true,
            hacker_news: true,
            ..DailyDropSettings::default()
        };
        let mut ledger = DailyDropLedger::default();
        let on = daily_drop_refresh(DailyDropRefreshCtx {
            settings: &on_settings,
            ranked: &ranked,
            ledger: &mut ledger,
            last_run_day: None,
            today,
            clock: &clock,
            night_hour: 21,
            now_ms: T0,
        });
        assert_eq!(on.cards.len(), 1);
        assert_eq!(on.mark_day.as_deref(), Some(today));
        assert_eq!(ledger.picked_today, 1);
    }

    #[test]
    fn daily_drop_refresh_waits_for_night_hour_and_marks_day_without_cards() {
        use crate::daily_drop::{DailyDropLedger, DailyDropSettings};
        use crate::organs::LocalClock;

        let today = "2026-10-11";
        let early = LocalClock {
            now_ms: T0,
            weekday: 6,
            hour: 12,
            minute: 0,
        };
        let settings = DailyDropSettings {
            enabled: true,
            hacker_news: true,
            ..DailyDropSettings::default()
        };
        let mut ledger = DailyDropLedger::default();
        let wait = daily_drop_refresh(DailyDropRefreshCtx {
            settings: &settings,
            ranked: &[],
            ledger: &mut ledger,
            last_run_day: None,
            today,
            clock: &early,
            night_hour: 21,
            now_ms: T0,
        });
        assert!(wait.cards.is_empty());
        assert!(wait.mark_day.is_none());

        let mut ledger = DailyDropLedger::default();
        let ran = daily_drop_refresh(DailyDropRefreshCtx {
            settings: &settings,
            ranked: &[],
            ledger: &mut ledger,
            last_run_day: Some("2026-10-10"),
            today,
            clock: &early,
            night_hour: 21,
            now_ms: T0,
        });
        assert!(ran.cards.is_empty());
        assert_eq!(ran.mark_day.as_deref(), Some(today));
    }
}
