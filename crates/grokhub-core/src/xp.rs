//! Level, XP and weeks in a row, worked out from what the user already did.
//!
//! Every point is one line in a small ledger (`xp-ledger.json`) under a
//! unique key: a finished task, a checked step, a Home card run or finished,
//! a goal milestone or a goal done. The ledger is filled from the outcome
//! log, the card signal log and the goals (`objectives.json`) each time it
//! is read, so it can't drift from what really happened, and it
//! keeps a line after those logs rotate, so XP never goes down on its own.
//! Only an undo (a newer failure line for the same task) takes a task back.
//!
//! Card and active-day XP is soft-capped per day: past [`DAILY_SOFT_CAP`]
//! the rest counts half, so a long session is never "capped" and tapping
//! cards over and over still can't farm levels.
//!
//! Weeks run Monday to Sunday on the user's local clock. A week with any XP
//! keeps the run going; two freezes are banked and used on their own when a
//! week is missed. The weekly target (active days) tunes itself from the
//! weeks before, so there is nothing to set.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::card_signals::{CardEvent, CardSignal};
use crate::mission::{MissionCheckItem, MissionCheckStatus};
use crate::outcome::{OutcomeResult, TaskOutcome};

pub const LEDGER_FILE: &str = "xp-ledger.json";

pub const XP_PER_TASK: u32 = 100;
pub const XP_PER_STEP: u32 = 25;
/// A Home card whose action the user ran (an idea filed, an offer automated).
pub const XP_PER_CARD_RAN: u32 = 25;
/// A Home card whose work went on to finish.
pub const XP_PER_CARD_DONE: u32 = 50;
/// Each day the user did any of the above.
pub const XP_PER_ACTIVE_DAY: u32 = 10;
/// Card and active-day XP one day adds at full value; the rest counts half.
pub const DAILY_SOFT_CAP: u32 = 150;
/// The week's target of active days met, before the weeks-in-a-row boost.
pub const XP_WEEK_TARGET: u32 = 100;

/// XP from level 1 to level 2. Each level after needs [`LEVEL_STEP`] more,
/// up to [`LEVEL_MAX_NEED`] a level.
pub const FIRST_LEVEL_NEED: u32 = 200;
pub const LEVEL_STEP: u32 = 50;
/// The most one level ever needs. It matches the old flat 500 a level, so
/// any total reads as the same level or higher than it did before.
pub const LEVEL_MAX_NEED: u32 = 500;

/// Freezes banked at the start and the most ever held.
pub const FREEZES_MAX: u32 = 2;
/// Weeks kept in a row that earn one freeze back.
pub const WEEKS_PER_FREEZE: u32 = 4;
/// The first week's target of active days.
pub const TARGET_START: u32 = 2;
pub const TARGET_MIN: u32 = 1;
pub const TARGET_MAX: u32 = 6;
/// Targets met in a row before the target goes up one.
const HITS_TO_RAISE: u32 = 3;
/// Targets missed in a row before it goes down one.
const MISSES_TO_LOWER: u32 = 2;

/// What earns XP and the daily soft cap, in one line for a hover or a tip.
pub const XP_HOW: &str = "XP comes from what you do: 100 per finished task, 25 per checked step, \
50 for each quarter of the way to one of your goals and 200 or more when it's done, 25 per Home \
card you run and 50 when its work finishes, 10 for each day you're active, and 100 or more when \
you reach your days for the week. Past 150 a day, card and day XP counts half.";

const MINUTE_MS: u64 = 60_000;
const DAY_MIN: i64 = 1440;

/// What a ledger line was earned for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum XpKind {
    Task,
    Step,
    CardRan,
    CardDone,
    /// A goal's 25, 50 or 75% milestone.
    Milestone,
    /// A goal done.
    GoalDone,
}

impl XpKind {
    /// Card XP shares the day's soft cap with the active-day bonus.
    fn capped(self) -> bool {
        matches!(self, Self::CardRan | Self::CardDone)
    }
}

/// One earned line: the local day it landed on and its points.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Earned {
    pub day: i64,
    pub xp: u32,
    pub kind: XpKind,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct XpLedger {
    /// Unique key (`task:<id>`, `step:<chat>:<step>`, `ran:<card>`,
    /// `done:<card>`) to the line it earned.
    #[serde(default)]
    pub earned: BTreeMap<String, Earned>,
}

/// The local day number (days since 1 Jan 1970 on the local clock).
pub fn local_day(ms: u64, offset_min: i32) -> i64 {
    ((ms / MINUTE_MS) as i64 + i64::from(offset_min)).div_euclid(DAY_MIN)
}

/// Monday-first weekday of a local day: 0 is Monday, 6 is Sunday.
/// 1 Jan 1970 was a Thursday.
pub fn weekday_mon(day: i64) -> usize {
    (day + 3).rem_euclid(7) as usize
}

fn week_of(day: i64) -> i64 {
    (day + 3).div_euclid(7)
}

impl XpLedger {
    /// Folds the logs in. A task counts once it finished or is pending, and
    /// is taken back only when its newest line is a failure (undo or
    /// correction). Cards pay once each. True when anything changed.
    pub fn ingest(
        &mut self,
        outcomes: &[TaskOutcome],
        signals: &[CardSignal],
        offset_min: i32,
    ) -> bool {
        let before = self.earned.clone();
        let mut newest: BTreeMap<&str, &TaskOutcome> = BTreeMap::new();
        for o in outcomes {
            let slot = newest.entry(o.task.as_str()).or_insert(o);
            if (o.at, o.finished_at) >= (slot.at, slot.finished_at) {
                *slot = o;
            }
        }
        for (task, o) in newest {
            let key = format!("task:{task}");
            if o.result == OutcomeResult::Failure {
                self.earned.remove(&key);
                // Undo of Accept as done also takes back the steps it checked.
                if o.reasons.iter().any(|r| r == crate::outcome::REASON_UNDO) {
                    let steps = format!("step:{task}:");
                    self.earned.retain(|k, _| !k.starts_with(&steps));
                }
            } else {
                self.earned.entry(key).or_insert(Earned {
                    day: local_day(o.finished_at, offset_min),
                    xp: XP_PER_TASK,
                    kind: XpKind::Task,
                });
            }
        }
        for s in signals {
            let (key, xp, kind) = match s.event {
                CardEvent::Ran => (
                    format!("ran:{}", s.card_id),
                    XP_PER_CARD_RAN,
                    XpKind::CardRan,
                ),
                CardEvent::Completed => (
                    format!("done:{}", s.card_id),
                    XP_PER_CARD_DONE,
                    XpKind::CardDone,
                ),
                _ => continue,
            };
            self.earned.entry(key).or_insert(Earned {
                day: local_day(s.ts, offset_min),
                xp,
                kind,
            });
        }
        self.earned != before
    }

    /// Folds in what the goals paid, and each season's surprise bonus for
    /// its first goal done. Goals keep their own payments, so this only ever
    /// adds. True when anything changed.
    pub fn ingest_goals(&mut self, goals: &[crate::objectives::Goal], offset_min: i32) -> bool {
        let mut changed = false;
        let firsts = crate::objectives::season_firsts(goals, offset_min);
        for (key, line) in goals
            .iter()
            .flat_map(|g| g.earned(offset_min))
            .chain(firsts)
        {
            if let std::collections::btree_map::Entry::Vacant(slot) = self.earned.entry(key) {
                slot.insert(line);
                changed = true;
            }
        }
        changed
    }

    /// Banks a chat's checked steps, so switching chats never changes XP.
    /// A step is banked once and stays banked.
    pub fn bank_steps(
        &mut self,
        chat: &str,
        items: &[MissionCheckItem],
        now_ms: u64,
        offset_min: i32,
    ) -> bool {
        let day = local_day(now_ms, offset_min);
        let mut changed = false;
        for i in items
            .iter()
            .filter(|i| i.status == MissionCheckStatus::Verified)
        {
            let key = format!("step:{chat}:{}", i.id);
            if let std::collections::btree_map::Entry::Vacant(slot) = self.earned.entry(key) {
                slot.insert(Earned {
                    day,
                    xp: XP_PER_STEP,
                    kind: XpKind::Step,
                });
                changed = true;
            }
        }
        changed
    }
}

pub fn ledger_path(dir: &Path) -> PathBuf {
    dir.join(LEDGER_FILE)
}

/// The saved ledger; a missing or broken file reads as empty (the logs
/// fill it again).
pub fn load_ledger(dir: &Path) -> XpLedger {
    std::fs::read_to_string(ledger_path(dir))
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

/// Saves through a temp file and a rename. Both apps write it; a line lost
/// to a race comes back from the logs on the next read.
pub fn save_ledger(dir: &Path, ledger: &XpLedger) -> Result<(), String> {
    let raw = serde_json::to_string(ledger).map_err(|e| e.to_string())?;
    crate::pulse_writer::write_atomic(&ledger_path(dir), &raw)
}

/// The open chat's checklist, to bank its checked steps on the way.
pub struct OpenChat<'a> {
    pub id: &'a str,
    pub items: &'a [MissionCheckItem],
}

/// Reads the logs, folds them into the saved ledger (saving it when it
/// changed) and sums it up. A save error still returns the right numbers.
pub fn refresh(dir: &Path, open: Option<OpenChat<'_>>, now_ms: u64, offset_min: i32) -> XpSummary {
    let mut ledger = load_ledger(dir);
    let mut changed = ledger.ingest(
        &crate::outcome::read_outcomes(dir),
        &crate::card_signals::read_signals(dir),
        offset_min,
    );
    changed |= ledger.ingest_goals(&crate::objectives::load(dir).goals, offset_min);
    if let Some(c) = open {
        changed |= ledger.bank_steps(c.id, c.items, now_ms, offset_min);
    }
    if changed {
        let _ = save_ledger(dir, &ledger);
    }
    summarize(&ledger, now_ms, offset_min)
}

/// This week on the local clock, Monday first.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WeekView {
    /// Monday to Sunday: true for a day with any XP.
    pub days: [bool; 7],
    /// Today's place in `days`.
    pub today: usize,
    /// Days with XP so far this week.
    pub active: u32,
    /// Active days the week asks for (tuned from the weeks before).
    pub target: u32,
    /// Weeks with XP in a row, this one included once it has XP. A week a
    /// freeze covered keeps the run without adding to it.
    pub in_a_row: u32,
    /// Every week ever with XP; never goes down.
    pub played: u32,
    /// Freezes banked for a missed week.
    pub freezes: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct XpSummary {
    pub level: u32,
    /// Level title, every five levels.
    pub title: &'static str,
    pub xp: u32,
    /// Total XP at which the next level starts.
    pub next_level_xp: u32,
    /// Progress into the level, 0 to 1.
    pub frac: f32,
    /// XP earned today, after the soft cap, with the week's bonus on the day
    /// the target was reached.
    pub today: u32,
    pub week: WeekView,
}

/// XP one level needs to reach the next.
pub fn level_need(level: u32) -> u32 {
    (FIRST_LEVEL_NEED + LEVEL_STEP * level.saturating_sub(1)).min(LEVEL_MAX_NEED)
}

/// Level, XP into it and what the level needs, for a total.
pub fn level_for(xp: u32) -> (u32, u32, u32) {
    let mut level = 1;
    let mut left = xp;
    loop {
        let need = level_need(level);
        if left < need {
            return (level, left, need);
        }
        left -= need;
        level += 1;
    }
}

/// A title for every five levels, for the line next to the level.
pub fn level_title(level: u32) -> &'static str {
    match level {
        0..=4 => "Rookie",
        5..=9 => "Regular",
        10..=14 => "Grinder",
        15..=19 => "Veteran",
        20..=29 => "Elite",
        _ => "Legend",
    }
}

/// The weekly target bonus after the weeks-in-a-row boost: 1x, then 1.25x,
/// 1.5x and at most 2x from four weeks in a row.
pub fn week_bonus(in_a_row: u32) -> u32 {
    match in_a_row {
        0 | 1 => XP_WEEK_TARGET,
        2 => XP_WEEK_TARGET * 5 / 4,
        3 => XP_WEEK_TARGET * 3 / 2,
        _ => XP_WEEK_TARGET * 2,
    }
}

/// A day's card and active-day XP after the soft cap.
fn soft_capped(raw: u32) -> u32 {
    if raw <= DAILY_SOFT_CAP {
        raw
    } else {
        DAILY_SOFT_CAP + (raw - DAILY_SOFT_CAP) / 2
    }
}

/// Sums the ledger as of `now_ms`.
pub fn summarize(ledger: &XpLedger, now_ms: u64, offset_min: i32) -> XpSummary {
    let today = local_day(now_ms, offset_min);
    // Per day: uncapped XP and the capped pool (cards plus the active day).
    let mut days: BTreeMap<i64, (u32, u32)> = BTreeMap::new();
    for e in ledger.earned.values().filter(|e| e.day <= today) {
        let d = days.entry(e.day).or_insert((0, XP_PER_ACTIVE_DAY));
        if e.kind.capped() {
            d.1 += e.xp;
        } else {
            d.0 += e.xp;
        }
    }
    let day_xp = |(free, pool): (u32, u32)| free + soft_capped(pool);
    let mut xp: u32 = days.values().map(|&d| day_xp(d)).sum();
    let mut today_xp = days.get(&today).map_or(0, |&d| day_xp(d));

    let active: BTreeSet<i64> = days.keys().copied().collect();
    let weeks = weeks(&active, today);
    xp += weeks.bonus;
    if weeks.hit_day == Some(today) {
        today_xp += weeks.this_bonus;
    }

    let (level, into, need) = level_for(xp);
    XpSummary {
        level,
        title: level_title(level),
        xp,
        next_level_xp: xp - into + need,
        frac: into as f32 / need as f32,
        today: today_xp,
        week: weeks.view,
    }
}

struct Weeks {
    view: WeekView,
    /// Every weekly bonus earned, this week's included once reached.
    bonus: u32,
    /// This week's bonus, when the target is reached.
    this_bonus: u32,
    /// The day this week's target was reached.
    hit_day: Option<i64>,
}

/// Walks every week from the first with XP to this one.
fn weeks(active: &BTreeSet<i64>, today: i64) -> Weeks {
    let this_week = week_of(today);
    let mut per_week: BTreeMap<i64, Vec<i64>> = BTreeMap::new();
    for &d in active {
        per_week.entry(week_of(d)).or_default().push(d);
    }
    let mut run = 0;
    let mut freezes = FREEZES_MAX;
    let mut kept = 0;
    let mut target = TARGET_START;
    let (mut hits, mut misses) = (0, 0);
    let mut bonus = 0;
    let mut played = 0;
    let first = per_week.keys().next().copied().unwrap_or(this_week);
    for w in first..this_week {
        let n = per_week.get(&w).map_or(0, |d| d.len() as u32);
        if n > 0 {
            run += 1;
            played += 1;
            kept += 1;
            if kept == WEEKS_PER_FREEZE {
                freezes = (freezes + 1).min(FREEZES_MAX);
                kept = 0;
            }
        } else if run > 0 && freezes > 0 {
            freezes -= 1;
        } else {
            run = 0;
            kept = 0;
        }
        if n >= target {
            bonus += week_bonus(run);
            hits += 1;
            misses = 0;
            if hits == HITS_TO_RAISE {
                target = (target + 1).min(TARGET_MAX);
                hits = 0;
            }
        } else {
            misses += 1;
            hits = 0;
            if misses == MISSES_TO_LOWER {
                target = target.saturating_sub(1).max(TARGET_MIN);
                misses = 0;
            }
        }
    }
    let now = per_week.get(&this_week).cloned().unwrap_or_default();
    let n = now.len() as u32;
    let in_a_row = if n > 0 { run + 1 } else { run };
    let mut days = [false; 7];
    for &d in &now {
        days[weekday_mon(d)] = true;
    }
    let (this_bonus, hit_day) = if n >= target {
        (
            week_bonus(in_a_row),
            now.get(target.saturating_sub(1) as usize).copied(),
        )
    } else {
        (0, None)
    };
    Weeks {
        view: WeekView {
            days,
            today: weekday_mon(today),
            active: n,
            target,
            in_a_row,
            played: played + u32::from(n > 0),
            freezes,
        },
        bonus: bonus + this_bonus,
        this_bonus,
        hit_day,
    }
}

/// Counted and verified checklist items: (done, total). Cancelled ones don't count.
pub fn objective_progress(items: &[MissionCheckItem]) -> (usize, usize) {
    let counted = items
        .iter()
        .filter(|i| i.status != MissionCheckStatus::Cancelled);
    let total = counted.clone().count();
    let done = counted
        .filter(|i| i.status == MissionCheckStatus::Verified)
        .count();
    (done, total)
}

/// "3 weeks in a row", or the first-week line before any week has XP.
pub fn weeks_line(w: &WeekView) -> String {
    match w.in_a_row {
        0 => "Your first week starts now".into(),
        1 => "1 week in a row".into(),
        n => format!("{n} weeks in a row"),
    }
}

/// "This week: 2 of 3 days".
pub fn week_target_line(w: &WeekView) -> String {
    let days = if w.target == 1 { "day" } else { "days" };
    format!(
        "This week: {} of {} {days}",
        w.active.min(w.target),
        w.target
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY_MS: u64 = 86_400_000;
    /// Monday 2 Nov 2026, 12:00 UTC (local day 20759).
    const MON: u64 = 1_793_620_800_000;

    fn task(id: &str, result: OutcomeResult, finished_at: u64) -> TaskOutcome {
        TaskOutcome {
            task: id.into(),
            result,
            finished_at,
            at: finished_at,
            ..TaskOutcome::default()
        }
    }

    fn signal(card: &str, event: CardEvent, ts: u64) -> CardSignal {
        CardSignal {
            ts,
            card_id: card.into(),
            kind: "suggestion".into(),
            group: String::new(),
            source_id: String::new(),
            event,
            after_open: None,
        }
    }

    fn step(id: &str, status: MissionCheckStatus) -> MissionCheckItem {
        MissionCheckItem {
            id: id.into(),
            text: id.into(),
            status,
        }
    }

    fn ledger(outcomes: &[TaskOutcome], signals: &[CardSignal]) -> XpLedger {
        let mut l = XpLedger::default();
        l.ingest(outcomes, signals, 0);
        l
    }

    #[test]
    fn the_test_monday_is_a_monday() {
        assert_eq!(local_day(MON, 0), 20759);
        assert_eq!(weekday_mon(local_day(MON, 0)), 0);
        assert_eq!(weekday_mon(local_day(MON + 6 * DAY_MS, 0)), 6);
        // 23:30 UTC on Sunday is already Monday at UTC+1.
        let late_sunday = MON - 12 * 3_600_000 - 30 * 60_000;
        assert_eq!(weekday_mon(local_day(late_sunday, 60)), 0);
        assert_eq!(weekday_mon(local_day(late_sunday, 0)), 6);
    }

    #[test]
    fn levels_start_fast_and_settle_at_500() {
        assert_eq!(level_for(0), (1, 0, 200));
        assert_eq!(level_for(199), (1, 199, 200));
        assert_eq!(level_for(200), (2, 0, 250));
        assert_eq!(level_for(1950), (7, 0, 500));
        assert_eq!(level_for(2450), (8, 0, 500));
        assert_eq!(level_need(30), 500);
        assert_eq!(level_title(1), "Rookie");
        assert_eq!(level_title(14), "Grinder");
        assert_eq!(level_title(40), "Legend");
    }

    #[test]
    fn no_total_reads_as_a_lower_level_than_the_old_flat_500() {
        for xp in (0..20_000).step_by(37) {
            let old = xp / 500 + 1;
            assert!(
                level_for(xp).0 >= old,
                "{xp} XP: {} < {old}",
                level_for(xp).0
            );
        }
    }

    #[test]
    fn a_task_counts_at_once_and_an_undone_one_is_taken_back() {
        let t = [
            task("a:1", OutcomeResult::Pending, MON),
            task("a:2", OutcomeResult::Success, MON),
            task("a:3", OutcomeResult::Failure, MON),
        ];
        let mut l = ledger(&t, &[]);
        let s = summarize(&l, MON, 0);
        // Two tasks plus the active day; one active day is below the target of 2.
        assert_eq!(s.xp, 2 * XP_PER_TASK + XP_PER_ACTIVE_DAY);
        assert_eq!(s.today, 210);

        let undo = TaskOutcome {
            at: MON + 5,
            ..task("a:2", OutcomeResult::Failure, MON)
        };
        assert!(l.ingest(&[undo], &[], 0));
        assert_eq!(summarize(&l, MON, 0).xp, XP_PER_TASK + XP_PER_ACTIVE_DAY);
    }

    #[test]
    fn xp_stays_after_the_logs_rotate() {
        let mut l = ledger(&[task("a:1", OutcomeResult::Success, MON)], &[]);
        assert!(!l.ingest(&[], &[], 0));
        assert_eq!(summarize(&l, MON, 0).xp, 110);
    }

    #[test]
    fn cards_pay_once_each_and_a_big_day_counts_half_past_the_cap() {
        let mut s = vec![
            signal("c1", CardEvent::Ran, MON),
            signal("c1", CardEvent::Ran, MON + 1),
            signal("c1", CardEvent::Completed, MON + 2),
            signal("c2", CardEvent::Opened, MON + 3),
        ];
        assert_eq!(summarize(&ledger(&[], &s), MON, 0).xp, 25 + 50 + 10);
        for i in 0..20 {
            s.push(signal(&format!("x{i}"), CardEvent::Ran, MON + 10));
        }
        // 75 + 500 + 10 = 585 raw: 150 full, then (585 - 150) / 2 = 217.
        let sum = summarize(&ledger(&[], &s), MON, 0);
        assert_eq!(sum.xp, 367);
        assert_eq!(sum.today, 367);
    }

    #[test]
    fn steps_bank_once_and_stay_after_switching_chats() {
        let mut l = XpLedger::default();
        let items = [
            step("1", MissionCheckStatus::Verified),
            step("2", MissionCheckStatus::Unverified),
            step("3", MissionCheckStatus::Cancelled),
        ];
        assert!(l.bank_steps("chat-a", &items, MON, 0));
        assert!(!l.bank_steps("chat-a", &items, MON + 9, 0));
        assert!(!l.bank_steps("chat-b", &[], MON, 0));
        // One step, uncapped, plus the active day.
        assert_eq!(summarize(&l, MON, 0).xp, 25 + 10);

        // Undo of Accept as done on chat-a takes its steps back; other chats keep theirs.
        assert!(l.bank_steps("chat-ab", &items, MON, 0));
        let undo = TaskOutcome {
            reasons: vec![crate::outcome::REASON_UNDO.into()],
            ..task("chat-a", OutcomeResult::Failure, MON + 1)
        };
        assert!(l.ingest(&[undo], &[], 0));
        assert_eq!(summarize(&l, MON, 0).xp, 25 + 10);
        assert!(l.earned.contains_key("step:chat-ab:1"));
        assert!(!l.earned.contains_key("step:chat-a:1"));
    }

    #[test]
    fn this_week_shows_its_days_and_the_target_pays_on_the_day_it_is_reached() {
        let t = [
            task("a:1", OutcomeResult::Success, MON),
            task("a:2", OutcomeResult::Success, MON + 2 * DAY_MS),
        ];
        let l = ledger(&t, &[]);
        let wed = summarize(&l, MON + 2 * DAY_MS, 0);
        assert_eq!(
            wed.week.days,
            [true, false, true, false, false, false, false]
        );
        assert_eq!(wed.week.today, 2);
        assert_eq!((wed.week.active, wed.week.target), (2, 2));
        assert_eq!(wed.week.in_a_row, 1);
        assert_eq!(wed.week.played, 1);
        // Two days of 110, plus the week's 100.
        assert_eq!(wed.xp, 320);
        assert_eq!(wed.today, 210);
        assert_eq!(week_target_line(&wed.week), "This week: 2 of 2 days");
        assert_eq!(weeks_line(&wed.week), "1 week in a row");
        let thu = summarize(&l, MON + 3 * DAY_MS, 0);
        assert_eq!(thu.today, 0);
        assert_eq!(thu.xp, 320);
    }

    #[test]
    fn a_missed_week_uses_a_freeze_and_the_run_keeps_going() {
        let week = 7 * DAY_MS;
        let t = [
            task("w1", OutcomeResult::Success, MON),
            task("w3", OutcomeResult::Success, MON + 2 * week),
        ];
        let s = summarize(&ledger(&t, &[]), MON + 2 * week, 0);
        assert_eq!(s.week.in_a_row, 2);
        assert_eq!(s.week.freezes, 1);
        assert_eq!(s.week.played, 2);
        // Four empty weeks after that use the last freeze and end the run,
        // but weeks played never goes down.
        let later = summarize(&ledger(&t, &[]), MON + 7 * week, 0);
        assert_eq!(later.week.in_a_row, 0);
        assert_eq!(later.week.played, 2);
        assert_eq!(weeks_line(&later.week), "Your first week starts now");
    }

    #[test]
    fn the_target_rises_after_three_met_weeks_and_the_boost_grows_to_2x() {
        let week = 7 * DAY_MS;
        let mut t = Vec::new();
        for w in 0..4u64 {
            for d in 0..3u64 {
                t.push(task(
                    &format!("w{w}d{d}"),
                    OutcomeResult::Success,
                    MON + w * week + d * DAY_MS,
                ));
            }
        }
        let s = summarize(&ledger(&t, &[]), MON + 3 * week + 2 * DAY_MS, 0);
        // Target 2 met three weeks, so week four asks for 3.
        assert_eq!(s.week.target, 3);
        assert_eq!(s.week.in_a_row, 4);
        // 12 days of 110, weekly bonuses 100 + 125 + 150, and 200 this week.
        assert_eq!(s.xp, 12 * 110 + 100 + 125 + 150 + 200);
        assert_eq!(s.today, 110 + 200);
    }

    #[test]
    fn the_target_drops_after_two_missed_weeks() {
        let week = 7 * DAY_MS;
        let t = [
            task("w0", OutcomeResult::Success, MON),
            task("w1", OutcomeResult::Success, MON + week),
        ];
        let s = summarize(&ledger(&t, &[]), MON + 2 * week, 0);
        assert_eq!(s.week.target, 1);
        assert_eq!(week_target_line(&s.week), "This week: 0 of 1 day");
    }

    #[test]
    fn the_ledger_round_trips_and_a_broken_file_reads_empty() {
        let dir = std::env::temp_dir().join(format!("grokhub-xp-ledger-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(load_ledger(&dir), XpLedger::default());
        let l = ledger(&[task("a:1", OutcomeResult::Success, MON)], &[]);
        save_ledger(&dir, &l).unwrap();
        assert_eq!(load_ledger(&dir), l);
        std::fs::write(ledger_path(&dir), "{nope").unwrap();
        assert_eq!(load_ledger(&dir), XpLedger::default());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn goal_payments_reach_the_summary_and_survive_a_lost_ledger() {
        use crate::objectives::{self, GameRef, GoalKind, NewGoal, Origin};
        let dir = std::env::temp_dir().join(format!("grokhub-xp-goals-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let new = NewGoal {
            game: GameRef {
                name: "Old School RuneScape".into(),
                ..GameRef::default()
            },
            text: "99 Fishing".into(),
            kind: GoalKind::Skill,
            current: 91,
            target: 99,
            origin: Origin::Typed,
            chat: None,
        };
        objectives::change(&dir, |o| {
            let id = o.add(new, MON).unwrap();
            o.set_progress(&id, 93, true, MON);
            o.set_progress(&id, 99, true, MON);
        })
        .unwrap();
        // A milestone (50), done (300) and the season's first goal (50),
        // uncapped, plus the active day.
        let paid = 50 + 300 + objectives::XP_SEASON_FIRST + 10;
        let s = refresh(&dir, None, MON, 0);
        assert_eq!(s.today, paid);
        std::fs::remove_file(ledger_path(&dir)).unwrap();
        assert_eq!(refresh(&dir, None, MON, 0).today, paid);
        assert!(XP_HOW.contains("200 or more when it's done"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn progress_counts_verified_and_skips_cancelled() {
        let items = [
            step("1", MissionCheckStatus::Verified),
            step("2", MissionCheckStatus::Verified),
            step("3", MissionCheckStatus::Attempted),
            step("4", MissionCheckStatus::Planned),
            step("5", MissionCheckStatus::Cancelled),
        ];
        assert_eq!(objective_progress(&items), (2, 4));
        assert_eq!(objective_progress(&[]), (0, 0));
    }
}
