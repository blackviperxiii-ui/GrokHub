//! Router R1 accuracy guard. A 10% internal holdout (picked from the episode
//! id, so the same episode always lands the same way; never shown to the user)
//! runs eligible user-facing work at today's fixed `high`. Per class, once both
//! arms have at least [`GUARD_MIN_N`] finished episodes, Auto's success rate
//! more than [`GUARD_GAP_POINTS`] below the holdout's reverts that class's start
//! and floor one rung up, and the cabin posts a Pulse Suggestion saying so.
//!
//! Success is the Spike-7 outcome (#556): VERIFY_OK, no correction and no undo
//! within 24 h. Until any outcome exists the guard only logs "guard waiting on
//! outcomes", once a day.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use grokhub_core::model_registry::store::{append_line, models_dir, read_json, write_json};
use grokhub_core::outcome::{read_outcomes, OutcomeResult, TaskOutcome};

use super::ladder::{user_facing, PREPARE_HARD};
use super::log::{RouteRecord, ROUTE_TRACE};
use super::policy::class_row;
use crate::harness::read_spans_tail;

/// One episode in ten runs at the fixed control effort.
pub const HOLDOUT_EVERY: u64 = 10;
/// The control arm's effort (today's fixed default).
pub const HOLDOUT_EFFORT: &str = "high";
/// Finished episodes each arm needs before the guard judges a class.
pub const GUARD_MIN_N: u32 = 50;
/// Auto may trail the holdout by this many percentage points.
pub const GUARD_GAP_POINTS: f64 = 2.0;
/// Route records the guard reads from the end of the model-call log.
pub const GUARD_SCAN_LINES: usize = 20_000;

pub const OVERRIDES_FILE: &str = "route_overrides.json";
pub const NOTES_FILE: &str = "router_notes.jsonl";

/// Deterministic by episode id: FNV-1a, one bucket in [`HOLDOUT_EVERY`].
pub fn in_holdout(episode: &str) -> bool {
    !episode.is_empty() && super::ladder::turn_hash(episode).is_multiple_of(HOLDOUT_EVERY)
}

/// Holdout applies to listed, user-facing classes that are not `prepare:hard`.
pub fn holdout_eligible(class: &str) -> bool {
    class_row(class).is_some_and(|r| user_facing(r.class) && r.class != PREPARE_HARD)
}

/// A class the guard reverted: its start and floor sit `steps` rungs higher since `at_ms`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Override {
    pub steps: u8,
    pub at_ms: u64,
}

pub fn overrides_path(config_dir: &Path) -> PathBuf {
    models_dir(config_dir).join(OVERRIDES_FILE)
}

pub fn load_overrides(config_dir: &Path) -> BTreeMap<String, Override> {
    read_json(&overrides_path(config_dir)).unwrap_or_default()
}

/// Successes and finished episodes in one arm.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Arm {
    pub n: u32,
    pub ok: u32,
}

impl Arm {
    fn rate(self) -> f64 {
        if self.n == 0 {
            0.0
        } else {
            self.ok as f64 * 100.0 / self.n as f64
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ClassStats {
    pub auto: Arm,
    pub control: Arm,
}

/// Classes whose Auto arm trails the holdout by more than the gap, with both arms at n ≥ 50.
pub fn judge(stats: &BTreeMap<String, ClassStats>) -> Vec<String> {
    stats
        .iter()
        .filter(|(_, s)| s.auto.n >= GUARD_MIN_N && s.control.n >= GUARD_MIN_N)
        .filter(|(_, s)| s.auto.rate() < s.control.rate() - GUARD_GAP_POINTS)
        .map(|(c, _)| c.clone())
        .collect()
}

/// Group route records by episode and count each arm's outcomes per class.
/// Records before a class's last revert don't count (that data judged the old start).
pub fn tally(
    records: &[RouteRecord],
    ts: &[u64],
    outcomes: &[TaskOutcome],
    overrides: &BTreeMap<String, Override>,
    now_ms: u64,
) -> BTreeMap<String, ClassStats> {
    let mut episodes: BTreeMap<&str, (&str, bool, Vec<&str>)> = BTreeMap::new();
    for (i, r) in records.iter().enumerate() {
        if r.episode.is_empty() || !holdout_eligible(&r.class) {
            continue;
        }
        let since = overrides.get(&r.class).map(|o| o.at_ms).unwrap_or(0);
        if ts.get(i).copied().unwrap_or(0) < since {
            continue;
        }
        let e = episodes.entry(r.episode.as_str()).or_insert((r.class.as_str(), r.holdout, Vec::new()));
        e.2.push(r.span_id.as_str());
    }
    let mut stats: BTreeMap<String, ClassStats> = BTreeMap::new();
    for (episode, (class, holdout, spans)) in episodes {
        let found = outcomes
            .iter()
            .rev()
            .find(|o| o.episode == episode || o.span_ids.iter().any(|s| spans.contains(&s.as_str())));
        let ok = match found.map(|o| o.effective(now_ms)) {
            Some(OutcomeResult::Success) => true,
            Some(OutcomeResult::Failure) => false,
            Some(OutcomeResult::Pending) | None => continue,
        };
        let s = stats.entry(class.to_string()).or_default();
        let arm = if holdout { &mut s.control } else { &mut s.auto };
        arm.n += 1;
        arm.ok += ok as u32;
    }
    stats
}

/// One router note line (`models/router_notes.jsonl`), written once per `key`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Note {
    pub key: String,
    pub ts_ms: u64,
    pub text: String,
}

pub fn notes_path(config_dir: &Path) -> PathBuf {
    models_dir(config_dir).join(NOTES_FILE)
}

/// Append `text` unless a note with `key` is already there. True when written.
pub fn note_once(config_dir: &Path, key: &str, text: &str, now_ms: u64) -> bool {
    let seen = std::fs::read_to_string(notes_path(config_dir))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str::<Note>(l).ok())
        .any(|n| n.key == key);
    if seen {
        return false;
    }
    append_line(&notes_path(config_dir), &Note { key: key.into(), ts_ms: now_ms, text: text.into() }).is_ok()
}

/// What one guard run did: the classes it reverted, each with the Pulse Suggestion text.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GuardRun {
    pub reverted: Vec<(String, String)>,
    pub waiting: bool,
}

const DAY_MS: u64 = 86_400_000;

/// Read route records and outcomes, revert any class that trails, and save the overrides.
pub fn run_guard(config_dir: &Path, now_ms: u64) -> GuardRun {
    let outcomes = read_outcomes(config_dir);
    if outcomes.is_empty() {
        let day = now_ms / DAY_MS;
        note_once(config_dir, &format!("guard-waiting:{day}"), "guard waiting on outcomes", now_ms);
        return GuardRun { reverted: Vec::new(), waiting: true };
    }
    let (spans, _) = read_spans_tail(config_dir, ROUTE_TRACE, GUARD_SCAN_LINES);
    let (ts, records): (Vec<u64>, Vec<RouteRecord>) = spans.into_iter().filter_map(|s| s.route.map(|r| (s.ts_ms, *r))).unzip();
    let mut overrides = load_overrides(config_dir);
    let stats = tally(&records, &ts, &outcomes, &overrides, now_ms);
    let mut run = GuardRun::default();
    for class in judge(&stats) {
        let Some(row) = class_row(&class) else {
            continue;
        };
        let o = overrides.entry(class.clone()).or_default();
        o.steps = o.steps.saturating_add(1);
        o.at_ms = now_ms;
        let s = stats[&class];
        let text = format!(
            "Auto was succeeding less on {} ({:.0}% vs {:.0}% at a fixed High, {} and {} tasks), so it now starts that work one step higher.",
            row.plain,
            s.auto.rate(),
            s.control.rate(),
            s.auto.n,
            s.control.n
        );
        let _ = note_once(config_dir, &format!("guard-revert:{class}:{now_ms}"), &text, now_ms);
        run.reverted.push((class, text));
    }
    if !run.reverted.is_empty() {
        let _ = write_json(&overrides_path(config_dir), &overrides);
    }
    run
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn holdout_is_about_one_in_ten_and_deterministic() {
        let ids: Vec<String> = (0..1000).map(|i| format!("ep-{i}")).collect();
        let picked: Vec<bool> = ids.iter().map(|id| in_holdout(id)).collect();
        let n = picked.iter().filter(|b| **b).count();
        assert!((80..=120).contains(&n), "holdout share {n}/1000");
        assert_eq!(picked, ids.iter().map(|id| in_holdout(id)).collect::<Vec<_>>());
        assert!(!in_holdout(""));
        assert!(holdout_eligible("chat:default") && holdout_eligible("chat:turn"));
        assert!(!holdout_eligible(PREPARE_HARD) && !holdout_eligible("background:judge") && !holdout_eligible("eval:item"));
    }

    fn stats(auto_ok: u32, auto_n: u32, ctl_ok: u32, ctl_n: u32) -> BTreeMap<String, ClassStats> {
        let mut m = BTreeMap::new();
        m.insert(
            "chat:default".to_string(),
            ClassStats { auto: Arm { n: auto_n, ok: auto_ok }, control: Arm { n: ctl_n, ok: ctl_ok } },
        );
        m
    }

    #[test]
    fn guard_reverts_at_a_three_point_gap_and_not_at_one_point_or_below_n_50() {
        // Auto 80% (40/50) vs control 83% (83/100): 3 points.
        assert_eq!(judge(&stats(40, 50, 83, 100)), vec!["chat:default".to_string()]);
        // Auto 80% vs control 81%: 1 point.
        assert!(judge(&stats(40, 50, 81, 100)).is_empty());
        // A 3-point gap with only 49 Auto episodes waits.
        assert!(judge(&stats(39, 49, 83, 100)).is_empty());
    }

    fn rec(episode: &str, holdout: bool, span: &str) -> RouteRecord {
        RouteRecord { episode: episode.into(), class: "chat:default".into(), holdout, span_id: span.into(), ..RouteRecord::default() }
    }

    fn outcome(episode: &str, result: OutcomeResult) -> TaskOutcome {
        TaskOutcome { task: episode.into(), episode: episode.into(), result, ..TaskOutcome::default() }
    }

    #[test]
    fn run_guard_reverts_the_class_writes_the_override_and_logs_waiting_once_a_day() {
        let dir = crate::harness::test_dir("route-guard");
        let now = grokhub_core::now_ms();
        assert!(run_guard(&dir, now).waiting);
        assert!(run_guard(&dir, now + 1000).waiting);
        let notes = std::fs::read_to_string(notes_path(&dir)).unwrap();
        assert_eq!(notes.lines().count(), 1, "{notes}");
        assert!(notes.contains("guard waiting on outcomes"));
        // 50 Auto episodes at 80%, 100 holdout episodes at 83%.
        let mut lines = String::new();
        for i in 0..150u32 {
            let holdout = i >= 50;
            let ok = if holdout { i - 50 < 83 } else { i < 40 };
            let ep = format!("ep-{i}");
            let mut span = crate::harness::Span::soft_allow(ROUTE_TRACE, "model.call", "{}", "ok", "", crate::harness::AccessMode::Supervised, "xai");
            span.route = Some(Box::new(rec(&ep, holdout, &format!("s{i}"))));
            crate::harness::append_span(&dir, &span).unwrap();
            let o = outcome(&ep, if ok { OutcomeResult::Success } else { OutcomeResult::Failure });
            lines.push_str(&serde_json::to_string(&o).unwrap());
            lines.push('\n');
        }
        std::fs::write(grokhub_core::outcome::outcomes_path(&dir), lines).unwrap();
        let run = run_guard(&dir, now + 2000);
        assert_eq!(run.reverted.len(), 1);
        assert_eq!(run.reverted[0].0, "chat:default");
        assert_eq!(
            run.reverted[0].1,
            "Auto was succeeding less on everyday chat (80% vs 83% at a fixed High, 50 and 100 tasks), so it now starts that work one step higher."
        );
        assert_eq!(load_overrides(&dir).get("chat:default"), Some(&Override { steps: 1, at_ms: now + 2000 }));
        // The records that judged the old start no longer count.
        assert!(run_guard(&dir, now + 3000).reverted.is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }
}
