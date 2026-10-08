//! Router R2b: the weekly budget (§14.5, §14.6) and the upgrade nudge (§14.9).
//!
//! Spend is summed per day and per week (weeks start Monday 00:00 UTC) from
//! the route records in `spans/model-calls.jsonl`: `usage.cost_in_usd_ticks`
//! when xAI reported it, else registry list prices × tokens. With an API key
//! the week's cap is the Settings weekly $ cap. On the plan pool GrokHub can't
//! see a pool percentage (R0 Step-0), so a 429 `free-usage-exhausted` this
//! week reads as 100%.
//!
//! What the router does with it (DE3 in R1's ladder):
//! - 80% or more: background goes to low effort, user-facing work stays at its
//!   class start, a Fast variant falls back to its plain model, and one card a
//!   week says so ([`EIGHTY_CARD`]).
//! - 100%: background pauses and user-facing work asks first ([`BUDGET_ASK`]).
//! - Background never takes more than [`BACKGROUND_SHARE`] of the weekly cap.
//!
//! Everything here is pure (the clock is an argument) except [`load_week`] and
//! [`BudgetNotes`], which read and write `models/budget_notes.json`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use grokhub_core::model_registry::store::{read_json, write_json};
use grokhub_core::model_registry::{Credential, Registry};

use super::log::{route_records, with_route_records, RouteRecord};

pub const DAY_MS: u64 = 24 * 60 * 60 * 1000;
pub const WEEK_MS: u64 = 7 * DAY_MS;
/// At this share of the week the budget is tight (DE3, the 80% card, Fast falls back).
pub const BUDGET_TIGHT_PCT: u8 = 80;
/// Background work may use at most this share of the weekly cap.
pub const BACKGROUND_SHARE: f64 = 0.15;
/// The upgrade nudge shows at most once in this long, and "Not now" mutes it this long.
pub const NUDGE_EVERY_MS: u64 = 30 * DAY_MS;
/// Tasks in 7 days where a model outside your plan would have been picked.
pub const NUDGE_MIN_TASKS: usize = 5;
/// Failed checks or usage-limit hits on your plan's models this week.
pub const NUDGE_MIN_FAILS: usize = 2;
/// Route-record lines the budget reads from the end of the model-call log.
pub const BUDGET_SCAN_LINES: usize = 20_000;
/// The budget snapshot is re-read at most this often.
pub const BUDGET_EVERY_MS: u64 = 60 * 1000;
const NOTES_FILE: &str = "budget_notes.json";

/// The 80% card (once a week).
pub const EIGHTY_CARD: &str =
    "You've used 80% of this week's usage. I'll keep chats at normal quality and move background work to cheaper settings. OK?";
/// The 100% ask on user-facing work.
pub const BUDGET_ASK: &str = "You've used this week's budget. Keep chatting this week anyway?";
/// What a paused call returns at 100%, or when background used its share.
pub const BUDGET_PAUSE_MSG: &str = "This week's budget is used up, so GrokHub paused this step. Home has your options.";
pub const BACKGROUND_PAUSE_MSG: &str = "Background work used its share of this week's budget, so it waits until next week.";

/// The week number (weeks start Monday 00:00 UTC; 1970-01-01 was a Thursday).
pub fn week_of(ms: u64) -> u64 {
    (ms / DAY_MS + 3) / 7
}

/// One routed call's spend.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CallCost {
    pub ts_ms: u64,
    pub usd: f64,
    /// A `background:*` class, or automation or proactive work.
    pub background: bool,
    /// It hit the plan's usage limit (429 `free-usage-exhausted`).
    pub limit: bool,
}

/// A class or origin that nobody is waiting on.
pub fn is_background(class: &str, origin: Option<&str>) -> bool {
    class.trim().starts_with("background:") || matches!(origin, Some("automation" | "proactive"))
}

/// The cost of one record: what xAI reported, else list price × tokens.
pub fn call_cost(ts_ms: u64, rec: &RouteRecord, reg: &Registry) -> CallCost {
    let background = is_background(&rec.class, rec.signals.origin.as_deref());
    let Some(out) = &rec.outcome else {
        return CallCost { ts_ms, usd: 0.0, background, limit: false };
    };
    let usd = if out.cost_usd > 0.0 {
        out.cost_usd
    } else {
        reg.get(&rec.used.model).map_or(0.0, |r| {
            let p = &r.meta.prices;
            let t = &out.tokens;
            let fresh = t.input.saturating_sub(t.cached);
            let cents = fresh * p.prompt.unwrap_or(0)
                + t.cached * p.cached.or(p.prompt).unwrap_or(0)
                + (t.out + t.reasoning) * p.completion.unwrap_or(0);
            cents as f64 / 100_000_000.0 / 100.0
        })
    };
    CallCost { ts_ms, usd, background, limit: out.limit }
}

/// One week of spend.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Week {
    pub week: u64,
    pub spent_usd: f64,
    pub background_usd: f64,
    /// Day number → USD.
    pub by_day: BTreeMap<u64, f64>,
    pub limit_hit: bool,
}

/// Sum this week's calls.
pub fn tally(calls: &[CallCost], now_ms: u64) -> Week {
    let week = week_of(now_ms);
    let mut w = Week { week, ..Week::default() };
    for c in calls.iter().filter(|c| week_of(c.ts_ms) == week) {
        w.spent_usd += c.usd;
        if c.background {
            w.background_usd += c.usd;
        }
        *w.by_day.entry(c.ts_ms / DAY_MS).or_default() += c.usd;
        w.limit_hit |= c.limit;
    }
    w
}

/// What the router reads from the week.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Budget {
    pub week: u64,
    /// Percent of the week used (capped at 100). `None` with no cap and no limit hit.
    pub pct: Option<u8>,
    /// Background spend's share of the cap, percent.
    pub background_pct: Option<f64>,
    pub cap_usd: f64,
    pub background_usd: f64,
}

impl Budget {
    pub fn of(week: &Week, cred: Credential, cap_usd: f64) -> Self {
        let keyed = cred == Credential::ApiKey && cap_usd > 0.0;
        let pct = if keyed {
            Some((week.spent_usd * 100.0 / cap_usd).floor().clamp(0.0, 100.0) as u8)
        } else if week.limit_hit {
            Some(100)
        } else {
            None
        };
        let background_pct = keyed.then(|| week.background_usd * 100.0 / cap_usd);
        Self { week: week.week, pct, background_pct, cap_usd: if keyed { cap_usd } else { 0.0 }, background_usd: week.background_usd }
    }

    pub fn tight(&self) -> bool {
        self.pct.is_some_and(|p| p >= BUDGET_TIGHT_PCT)
    }

    pub fn used_up(&self) -> bool {
        self.pct.is_some_and(|p| p >= 100)
    }

    /// May one more background call of about `est_usd` go? Not at 100%, and
    /// not past [`BACKGROUND_SHARE`] of the cap.
    pub fn admits_background(&self, est_usd: f64) -> bool {
        if self.used_up() {
            return false;
        }
        self.cap_usd <= 0.0 || self.background_usd + est_usd <= self.cap_usd * BACKGROUND_SHARE
    }
}

/// What the cabin remembers about budget cards. Written only by the cabin
/// (a shown card or your click), read by the router.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BudgetNotes {
    /// The week the 80% card last showed.
    #[serde(default)]
    pub eighty_week: u64,
    /// The week you said to keep chatting past 100%.
    #[serde(default)]
    pub go_on_week: u64,
    #[serde(default)]
    pub nudge_shown_ms: u64,
    #[serde(default)]
    pub nudge_muted_until_ms: u64,
}

pub fn notes_path(config_dir: &Path) -> PathBuf {
    config_dir.join("models").join(NOTES_FILE)
}

impl BudgetNotes {
    pub fn load(config_dir: &Path) -> Self {
        read_json(&notes_path(config_dir)).unwrap_or_default()
    }

    pub fn save(&self, config_dir: &Path) {
        let _ = write_json(&notes_path(config_dir), self);
    }

    /// The 80% card is due: the week is tight and it hasn't shown this week.
    pub fn eighty_due(&self, b: &Budget) -> bool {
        b.tight() && self.eighty_week != b.week
    }

    /// At 100%, user-facing work waits for your answer this week.
    pub fn must_ask(&self, b: &Budget) -> bool {
        b.used_up() && self.go_on_week != b.week
    }
}

/// The facts the upgrade nudge counts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NudgeFacts {
    /// Tasks (episodes) in the last 7 days where a model outside your plan would have been picked.
    pub would_upgrade: usize,
    /// This week's plan-model calls that failed the check or hit the usage limit.
    pub fails: usize,
    /// Of the last (up to) 5 would-upgrade tasks, how many failed the check.
    pub last_failed: usize,
    pub last_of: usize,
}

/// Count the nudge facts from route records `(ts_ms, record)`, oldest first.
pub fn nudge_facts(records: &[(u64, RouteRecord)], now_ms: u64) -> NudgeFacts {
    let week = week_of(now_ms);
    let recent: Vec<&(u64, RouteRecord)> = records.iter().filter(|(t, _)| now_ms.saturating_sub(*t) <= WEEK_MS).collect();
    let mut tasks: Vec<(String, bool)> = Vec::new();
    let mut seen = BTreeSet::new();
    for (_, r) in &recent {
        if r.rule_ids.iter().any(|x| x == "plan:would_pick") && seen.insert(r.episode.clone()) {
            tasks.push((r.episode.clone(), false));
        }
    }
    let rejected = |r: &RouteRecord| r.outcome.as_ref().is_some_and(|o| o.verify.as_deref() == Some("reject"));
    for (ep, failed) in tasks.iter_mut() {
        *failed = recent.iter().any(|(_, r)| r.episode == *ep && rejected(r));
    }
    let fails = records
        .iter()
        .filter(|(t, r)| week_of(*t) == week && r.cost_class == "included")
        .filter(|(_, r)| rejected(r) || r.outcome.as_ref().is_some_and(|o| o.limit))
        .count();
    let last: Vec<&(String, bool)> = tasks.iter().rev().take(5).collect();
    NudgeFacts { would_upgrade: tasks.len(), fails, last_failed: last.iter().filter(|(_, f)| *f).count(), last_of: last.len() }
}

/// Where the nudge stands right now.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NudgeGate {
    /// A turn or episode is running.
    pub in_task: bool,
    pub quiet_hours: bool,
    /// The P2 card throttle has no room.
    pub throttled: bool,
}

/// The nudge may show: its thresholds hold, nothing is running, not in quiet
/// hours or under the throttle, and not shown or muted in the last 30 days.
pub fn nudge_due(f: &NudgeFacts, notes: &BudgetNotes, gate: NudgeGate, now_ms: u64) -> bool {
    if gate.in_task || gate.quiet_hours || gate.throttled {
        return false;
    }
    if now_ms < notes.nudge_muted_until_ms || (notes.nudge_shown_ms > 0 && now_ms.saturating_sub(notes.nudge_shown_ms) < NUDGE_EVERY_MS) {
        return false;
    }
    f.would_upgrade >= NUDGE_MIN_TASKS && f.fails >= NUDGE_MIN_FAILS
}

/// The nudge card's words. It names SuperGrok Heavy only when your plan is a
/// smaller one GrokHub knows.
pub fn nudge_text(f: &NudgeFacts, tier: Option<&str>) -> String {
    let plan = match tier {
        Some(t) if !t.to_ascii_lowercase().contains("heavy") => "SuperGrok Heavy includes",
        _ => "A bigger Grok plan includes",
    };
    let lead = if f.last_of > 0 && f.last_failed > 0 {
        format!("{} of your last {} big tasks failed the check on your plan's models.", f.last_failed, f.last_of)
    } else {
        format!("Your plan's models failed the check or hit the usage limit {} times this week.", f.fails)
    };
    format!("{lead} {plan} more capacity for this kind of work. Want details?")
}

/// "Want details": counts and where plans live. GrokHub opens nothing and
/// buys nothing; plans are changed on grok.com by you.
pub fn nudge_details(f: &NudgeFacts) -> String {
    format!(
        "In the last 7 days, {} tasks would have used a model outside your plan, and {} checks failed or hit the usage limit this week. Plans and their limits are on grok.com, under your account. GrokHub doesn't change plans or buy anything.",
        f.would_upgrade, f.fails
    )
}

/// Route records from the model-call log, `(ts_ms, record)`, oldest first.
pub fn read_records(config_dir: &Path) -> Vec<(u64, RouteRecord)> {
    route_records(config_dir, BUDGET_SCAN_LINES)
}

type WeekCache = Option<((PathBuf, u64), Week)>;
static WEEK: Mutex<WeekCache> = Mutex::new(None);

/// This week's spend, re-read at most every [`BUDGET_EVERY_MS`].
pub fn load_week(config_dir: &Path, reg: &Registry, now_ms: u64) -> Week {
    let slot = now_ms / BUDGET_EVERY_MS;
    let key = (config_dir.to_path_buf(), slot);
    if let Some((k, w)) = WEEK.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        if *k == key {
            return w.clone();
        }
    }
    // Read without the lock: a send never waits on the cabin's own budget tick.
    let calls: Vec<CallCost> = with_route_records(config_dir, BUDGET_SCAN_LINES, |records| records.map(|(t, r)| call_cost(*t, r, reg)).collect());
    let w = tally(&calls, now_ms);
    *WEEK.lock().unwrap_or_else(|e| e.into_inner()) = Some((key, w.clone()));
    w
}

/// Forget the cached week (a test, or the cabin after Settings changed the cap).
pub fn forget_week() {
    *WEEK.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::route::log::{RouteOutcome, RouteTokens};

    /// Monday 2026-10-05 00:00 UTC.
    const MON: u64 = 20_731 * DAY_MS;

    fn cost(day: u64, usd: f64, background: bool) -> CallCost {
        CallCost { ts_ms: MON + day * DAY_MS + 1_000, usd, background, limit: false }
    }

    #[test]
    fn weeks_start_on_monday_and_spend_sums_per_day_and_week() {
        assert_eq!(week_of(MON), week_of(MON + 6 * DAY_MS + DAY_MS - 1));
        assert_eq!(week_of(MON) + 1, week_of(MON + WEEK_MS));
        assert_eq!(week_of(MON - 1) + 1, week_of(MON), "Sunday night is last week");
        let calls = [cost(0, 1.0, false), cost(0, 0.5, true), cost(3, 2.0, false), cost(7, 9.0, false)];
        let w = tally(&calls, MON + 4 * DAY_MS);
        assert_eq!((w.spent_usd, w.background_usd, w.by_day.len()), (3.5, 0.5, 2));
        assert_eq!(w.by_day[&(MON / DAY_MS)], 1.5);
        assert!(!w.limit_hit);
    }

    #[test]
    fn a_key_reads_against_its_cap_and_the_plan_reads_the_limit_signal() {
        let w = Week { week: 1, spent_usd: 8.5, background_usd: 1.0, ..Week::default() };
        let b = Budget::of(&w, Credential::ApiKey, 10.0);
        assert_eq!((b.pct, b.tight(), b.used_up()), (Some(85), true, false));
        assert_eq!(Budget::of(&w, Credential::ApiKey, 0.0).pct, None, "no cap, no percentage");
        assert_eq!(Budget::of(&w, Credential::Plan, 10.0).pct, None, "the plan pool isn't dollars");
        let hit = Week { limit_hit: true, ..w.clone() };
        assert_eq!(Budget::of(&hit, Credential::Plan, 0.0).pct, Some(100));
        assert_eq!(Budget::of(&Week { spent_usd: 30.0, ..w }, Credential::ApiKey, 10.0).pct, Some(100));
    }

    #[test]
    fn the_eighty_card_shows_once_a_week_and_at_100_user_work_asks_until_you_answer() {
        let mut notes = BudgetNotes::default();
        let tight = Budget { week: 7, pct: Some(81), ..Budget::default() };
        assert!(notes.eighty_due(&tight));
        notes.eighty_week = 7;
        assert!(!notes.eighty_due(&tight), "once a week");
        assert!(notes.eighty_due(&Budget { week: 8, ..tight }), "a new week can show it again");
        assert!(!notes.eighty_due(&Budget { pct: Some(79), ..tight }));
        let full = Budget { week: 7, pct: Some(100), cap_usd: 10.0, ..Budget::default() };
        assert!(notes.must_ask(&full));
        assert!(!full.admits_background(0.0), "at 100% background pauses");
        notes.go_on_week = 7;
        assert!(!notes.must_ask(&full));
        assert!(notes.must_ask(&Budget { week: 8, ..full }));
    }

    /// A fixture week: 400 background calls at $1/128 and 300 chat calls at
    /// $0.02 against a $20 cap. Background is admitted only while it stays
    /// within 15% of the cap ($3): 384 go, 16 wait.
    #[test]
    fn background_stays_within_fifteen_percent_over_a_fixture_week() {
        let cap = 20.0;
        let mut calls: Vec<CallCost> = Vec::new();
        let mut paused = 0;
        for i in 0..700u64 {
            let at = MON + (i * WEEK_MS / 700);
            let background = i % 7 < 4;
            let usd = if background { 1.0 / 128.0 } else { 0.02 };
            let b = Budget::of(&tally(&calls, at), Credential::ApiKey, cap);
            if background && !b.admits_background(usd) {
                paused += 1;
                continue;
            }
            calls.push(CallCost { ts_ms: at, usd, background, limit: false });
        }
        let w = tally(&calls, MON + WEEK_MS - 1);
        assert_eq!((w.background_usd, paused), (3.0, 16));
        assert_eq!(Budget::of(&w, Credential::ApiKey, cap).background_pct, Some(15.0));
    }

    fn rec(episode: &str, rules: &[&str], verify: Option<&str>, limit: bool) -> RouteRecord {
        RouteRecord {
            episode: episode.into(),
            class: "code:multi-file".into(),
            rule_ids: rules.iter().map(|s| s.to_string()).collect(),
            cost_class: "included".into(),
            outcome: Some(RouteOutcome { ok: true, verify: verify.map(str::to_string), tokens: RouteTokens::default(), cost_usd: 0.0, limit }),
            ..RouteRecord::default()
        }
    }

    fn week_of_tasks(fails: usize) -> Vec<(u64, RouteRecord)> {
        let mut out = Vec::new();
        for i in 0..5usize {
            let verify = (i < fails).then_some("reject");
            out.push((MON + i as u64 * DAY_MS, rec(&format!("ep{i}"), &["plan:would_pick"], verify, false)));
        }
        out
    }

    #[test]
    fn the_nudge_needs_its_thresholds_never_runs_mid_task_and_shows_once_in_30_days() {
        let now = MON + 5 * DAY_MS;
        let gate = NudgeGate::default();
        let f = nudge_facts(&week_of_tasks(3), now);
        assert_eq!(f, NudgeFacts { would_upgrade: 5, fails: 3, last_failed: 3, last_of: 5 });
        let mut notes = BudgetNotes::default();
        assert!(nudge_due(&f, &notes, gate, now));
        assert_eq!(
            nudge_text(&f, Some("SuperGrok")),
            "3 of your last 5 big tasks failed the check on your plan's models. SuperGrok Heavy includes more capacity for this kind of work. Want details?"
        );
        assert!(nudge_text(&f, Some("SuperGrok Heavy")).contains("A bigger Grok plan includes"));
        // Thresholds: 4 tasks, or only 1 failure, is not enough.
        let four = nudge_facts(&week_of_tasks(3)[1..], now);
        assert!(!nudge_due(&four, &notes, gate, now));
        assert!(!nudge_due(&nudge_facts(&week_of_tasks(1), now), &notes, gate, now));
        // A usage-limit hit counts as a failure too.
        let mut limits = week_of_tasks(1);
        limits.push((MON + DAY_MS, rec("ep9", &[], None, true)));
        assert_eq!(nudge_facts(&limits, now).fails, 2);
        // Never mid-task, in quiet hours or under the throttle.
        for g in [NudgeGate { in_task: true, ..gate }, NudgeGate { quiet_hours: true, ..gate }, NudgeGate { throttled: true, ..gate }] {
            assert!(!nudge_due(&f, &notes, g, now), "{g:?}");
        }
        // Shown: not again for 30 days.
        notes.nudge_shown_ms = now;
        assert!(!nudge_due(&f, &notes, gate, now + NUDGE_EVERY_MS - 1));
        assert!(nudge_due(&f, &notes, gate, now + NUDGE_EVERY_MS));
        // "Not now" mutes it for 30 days.
        let muted = BudgetNotes { nudge_muted_until_ms: now + NUDGE_EVERY_MS, ..BudgetNotes::default() };
        assert!(!nudge_due(&f, &muted, gate, now + NUDGE_EVERY_MS - 1));
        assert!(nudge_due(&f, &muted, gate, now + NUDGE_EVERY_MS));
        // Old tasks fall out of the 7-day window.
        assert_eq!(nudge_facts(&week_of_tasks(3), now + 2 * WEEK_MS).would_upgrade, 0);
    }

    #[test]
    fn a_record_without_a_reported_cost_is_priced_from_the_registry() {
        use grokhub_core::model_registry::{Listing, ModelMeta, Prices, SourceKind};
        let mut reg = Registry::default();
        let meta = ModelMeta { id: "grok-4.7".into(), context_length: Some(1), prices: Prices { prompt: Some(20_000), cached: Some(5_000), completion: Some(100_000), ..Prices::default() }, ..ModelMeta::default() };
        reg.apply_refresh(&[Listing::new(SourceKind::XaiApi, vec![meta])], &[], 1);
        let mut r = rec("e", &[], None, false);
        r.used.model = "grok-4.7".into();
        r.signals.origin = Some("automation".into());
        r.outcome.as_mut().unwrap().tokens = RouteTokens { input: 10_000, cached: 4_000, out: 500, reasoning: 500 };
        let c = call_cost(5, &r, &reg);
        let want = (6_000.0 * 20_000.0 + 4_000.0 * 5_000.0 + 1_000.0 * 100_000.0) / 1e8 / 100.0;
        assert_eq!((c.usd, c.background), (want, true));
        r.outcome.as_mut().unwrap().cost_usd = 0.25;
        assert_eq!(call_cost(5, &r, &reg).usd, 0.25);
    }
}
