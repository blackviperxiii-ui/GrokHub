//! Card 32 PR B: every self-change is a version on trial. A self-change
//! GrokHub makes on its own (a skill patch from the weekly pass) gets a
//! version id, its parent, a diff stat and a target metric in
//! `{config}/changes/changes.jsonl`, beside the ChangeLedger it cites by seq.
//!
//! While the trial runs, a share of the matching runs ([`CANARY_SHARE_PCT`],
//! picked by a hash of the run key so a run always lands on the same side)
//! use the new version; the rest use the parent. After [`CANARY_RUNS`]
//! canary runs, or [`CANARY_MAX_MS`], [`judge`] compares the two sides on
//! the target metric, how many runs worked, and how many were undone. Better
//! on the target and no worse on the rest keeps it; anything else rolls it
//! back through the ledger (an `undo` line by GrokHub). Either way the
//! version line carries one quiet note naming the change and the numbers.
//!
//! Local only: no network, no model call. This module is the evaluator and
//! rollback; nothing the weekly pass proposes may change it.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use grokhub_core::outcome::OutcomeResult;
use grokhub_core::run_metrics::{Arm, RunMetrics};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::harness::changes::{
    rollback_change, Change, ChangeKind, ChangeLedger, ChangeTarget, SkillTarget, SnippetTarget, CHANGES_DIR,
};

pub const VERSIONS_FILE: &str = "changes.jsonl";
/// Share of matching runs that use the new version, in percent.
pub const CANARY_SHARE_PCT: u8 = 20;
/// Canary runs before the trial is judged.
pub const CANARY_RUNS: u32 = 10;
/// The longest a trial runs before it is judged on what it has.
pub const CANARY_MAX_MS: u64 = 7 * 24 * 60 * 60 * 1000;
/// The class of a trial that applies to every chat run (a chat note).
pub const ALL_RUNS: &str = "*";
/// Runs before the trial that stand in for the control side when it has none.
const BASELINE_RUNS: usize = 20;
const HASH_SHOWN: usize = 12;

/// What a trial must improve.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Metric {
    #[default]
    Steps,
    WallMs,
    Tokens,
    Cost,
}

impl Metric {
    fn of(self, m: &RunMetrics) -> f64 {
        match self {
            Self::Steps => f64::from(m.steps),
            Self::WallMs => m.wall_ms as f64,
            Self::Tokens => (m.tokens.input + m.tokens.out + m.tokens.reasoning) as f64,
            Self::Cost => m.cost_usd,
        }
    }

    fn noun(self) -> &'static str {
        match self {
            Self::Steps => "steps",
            Self::WallMs => "time",
            Self::Tokens => "tokens",
            Self::Cost => "cost",
        }
    }

    /// "18% fewer steps" / "18% more steps".
    fn phrase(self, pct: u32, better: bool) -> String {
        let (good, bad) = match self {
            Self::Steps | Self::Tokens => ("fewer", "more"),
            Self::WallMs => ("less", "more"),
            Self::Cost => ("lower", "higher"),
        };
        format!("{pct}% {} {}", if better { good } else { bad }, self.noun())
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VersionStatus {
    #[default]
    Canary,
    Kept,
    RolledBack,
    /// The target changed again before the trial ended (an edit or an undo).
    Ended,
}

impl VersionStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Canary => "canary",
            Self::Kept => "kept",
            Self::RolledBack => "rolled back",
            Self::Ended => "ended",
        }
    }
}

/// One version line. A later line for the same `version` wins.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Version {
    /// `<kind>:<id>@<hash12>` of the new bytes.
    pub version: String,
    /// The same for the bytes it replaced. Empty when it created the target.
    pub parent: String,
    pub kind: String,
    pub id: String,
    /// The ledger line that made it.
    pub seq: u64,
    /// What notes call it ("Wait for Export in weekly-report").
    pub label: String,
    /// Lines added and removed against the parent ("+2 -1 lines").
    pub diff: String,
    pub target: Metric,
    /// The run class it applies to (`skill:<name>`).
    pub class: String,
    pub share_pct: u8,
    pub min_runs: u32,
    pub max_ms: u64,
    pub status: VersionStatus,
    pub started_at: u64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
    pub at: u64,
}

impl Version {
    pub fn is_open(&self) -> bool {
        self.status == VersionStatus::Canary
    }

    fn closed(&self, status: VersionStatus, note: String, at: u64) -> Self {
        Self {
            status,
            note,
            at,
            ..self.clone()
        }
    }
}

pub fn versions_path(config_dir: &Path) -> PathBuf {
    config_dir.join(CHANGES_DIR).join(VERSIONS_FILE)
}

fn version_id(kind: &str, id: &str, hash: &str) -> String {
    if hash.is_empty() {
        String::new()
    } else {
        format!("{kind}:{id}@{}", &hash[..hash.len().min(HASH_SHOWN)])
    }
}

/// Lines added and removed, counted as multisets of lines.
pub fn diff_stat(before: &str, after: &str) -> String {
    let mut gone: Vec<&str> = before.lines().collect();
    let mut added = 0;
    for l in after.lines() {
        match gone.iter().position(|g| *g == l) {
            Some(i) => {
                gone.remove(i);
            }
            None => added += 1,
        }
    }
    format!("+{added} -{} lines", gone.len())
}

fn append_version(config_dir: &Path, v: &Version) -> Result<(), String> {
    let path = versions_path(config_dir);
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let line = serde_json::to_string(v).map_err(|e| e.to_string())?;
    let mut f = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| e.to_string())?;
    writeln!(f, "{line}").map_err(|e| e.to_string())
}

/// Every version, newest line per version, oldest trial first.
pub fn read_versions(config_dir: &Path) -> Vec<Version> {
    let raw = fs::read_to_string(versions_path(config_dir)).unwrap_or_default();
    let mut out: Vec<Version> = Vec::new();
    for v in raw
        .lines()
        .filter_map(|l| serde_json::from_str::<Version>(l.trim()).ok())
    {
        match out.iter_mut().find(|o| o.version == v.version) {
            Some(prev) if v.at >= prev.at => *prev = v,
            Some(_) => {}
            None => out.push(v),
        }
    }
    out.sort_by_key(|v| (v.started_at, v.seq));
    out
}

/// The open trial for a run class, if any.
pub fn open_trial(config_dir: &Path, class: &str) -> Option<Version> {
    read_versions(config_dir)
        .into_iter()
        .rev()
        .find(|v| v.is_open() && v.class == class)
}

/// Start a trial for the ledger line `change` made, with the default share,
/// run count and window. `before`/`after` are the bytes for the diff stat.
/// An open trial on the same class ends first: the new change replaced it.
pub fn start_trial(
    config_dir: &Path,
    change: &Change,
    class: &str,
    label: &str,
    target: Metric,
    before: &str,
    after: &str,
) -> Result<Version, String> {
    let at = change.at;
    if let Some(open) = open_trial(config_dir, class) {
        append_version(
            config_dir,
            &open.closed(VersionStatus::Ended, String::new(), at),
        )?;
    }
    let v = Version {
        version: version_id(&change.kind, &change.id, &change.after_hash),
        parent: version_id(&change.kind, &change.id, &change.before_hash),
        kind: change.kind.clone(),
        id: change.id.clone(),
        seq: change.seq,
        label: grokhub_core::redact_secrets(label.trim()),
        diff: diff_stat(before, after),
        target,
        class: class.to_string(),
        share_pct: CANARY_SHARE_PCT,
        min_runs: CANARY_RUNS,
        max_ms: CANARY_MAX_MS,
        status: VersionStatus::Canary,
        started_at: at,
        note: String::new(),
        at,
    };
    if v.version.is_empty() {
        return Err("a removal has no version to try".into());
    }
    append_version(config_dir, &v)?;
    Ok(v)
}

/// Which side of `version`'s trial the run `run_key` is on. The same key
/// always gets the same side.
pub fn pick_arm(run_key: &str, version: &str, share_pct: u8) -> Arm {
    let h = Sha256::digest(format!("{version}\u{1f}{run_key}").as_bytes());
    let n = u16::from_be_bytes([h[0], h[1]]) % 100;
    if n < u16::from(share_pct) {
        Arm::Canary
    } else {
        Arm::Control
    }
}

/// What [`judge`] decided. Only this module makes one, so only the evaluator
/// can roll a change back without the user's click.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verdict(());

/// The decision on one trial and the note that names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Judged {
    pub keep: bool,
    pub note: String,
}

fn rate(runs: &[&RunMetrics], hit: impl Fn(&RunMetrics) -> bool) -> (usize, usize) {
    (runs.iter().filter(|m| hit(m)).count(), runs.len())
}

fn mean(runs: &[&RunMetrics], metric: Metric) -> f64 {
    runs.iter().map(|m| metric.of(m)).sum::<f64>() / runs.len().max(1) as f64
}

fn runs_word(n: usize) -> &'static str {
    if n == 1 {
        "run"
    } else {
        "runs"
    }
}

/// Judge a trial on `runs` (the folded metrics) as of `now`. `None` while it
/// still runs. The control side is the trial's own parent runs, else the
/// class's last runs before the trial.
pub fn judge(v: &Version, runs: &[RunMetrics], now: u64) -> Option<Judged> {
    let on = |arm: Arm| -> Vec<&RunMetrics> {
        runs.iter()
            .filter(|m| m.version == v.version && m.arm == Some(arm))
            .collect()
    };
    let canary = on(Arm::Canary);
    let mut base = on(Arm::Control);
    if base.is_empty() {
        let before: Vec<&RunMetrics> = runs
            .iter()
            .filter(|m| (v.class == ALL_RUNS || m.class == v.class) && m.finished_at < v.started_at && m.version.is_empty())
            .collect();
        base = before.into_iter().rev().take(BASELINE_RUNS).collect();
    }
    let due = canary.len() >= v.min_runs as usize || now.saturating_sub(v.started_at) >= v.max_ms;
    if !due {
        return None;
    }
    let label = &v.label;
    let n = canary.len();
    if canary.is_empty() || base.is_empty() {
        let note =
            format!("Rolled back: {label}, too few runs to compare ({n} on the new version)");
        return Some(Judged { keep: false, note });
    }
    let worked = |m: &RunMetrics| m.result != OutcomeResult::Failure;
    let (cw, _) = rate(&canary, worked);
    let (bw, bn) = rate(&base, worked);
    // cw/n < bw/bn, without floats.
    if cw * bn < bw * n {
        let note = format!(
            "Rolled back: {label}, {cw} of {n} {} worked against {bw} of {bn} before",
            runs_word(n)
        );
        return Some(Judged { keep: false, note });
    }
    let undone = |m: &RunMetrics| m.undos > 0;
    let (cu, _) = rate(&canary, undone);
    let (bu, _) = rate(&base, undone);
    if cu * bn > bu * n {
        let note = format!(
            "Rolled back: {label}, undone in {cu} of {n} {} against {bu} of {bn} before",
            runs_word(n)
        );
        return Some(Judged { keep: false, note });
    }
    let (cm, bm) = (mean(&canary, v.target), mean(&base, v.target));
    let pct = if bm > 0.0 {
        ((bm - cm).abs() / bm * 100.0).round() as u32
    } else {
        0
    };
    let runs = format!("over {n} {}", runs_word(n));
    if cm < bm {
        let note = format!("Kept: {label}, {} {runs}", v.target.phrase(pct, true));
        return Some(Judged { keep: true, note });
    }
    let what = if cm > bm {
        v.target.phrase(pct, false)
    } else {
        format!("no change in {}", v.target.noun())
    };
    Some(Judged {
        keep: false,
        note: format!("Rolled back: {label}, {what} {runs}"),
    })
}

/// The ledger target a trial changes, when this build can put it back.
fn target_of<'a>(v: &'a Version, config_dir: &'a Path, skills_dir: &'a Path) -> Option<Box<dyn ChangeTarget + 'a>> {
    match ChangeKind::parse(&v.kind)? {
        ChangeKind::Skill => Some(Box::new(SkillTarget {
            skills_dir,
            name: &v.id,
        })),
        ChangeKind::Snippet => Some(Box::new(SnippetTarget { config_dir, name: &v.id })),
        _ => None,
    }
}

/// Judge every open trial and close the ones that are due: keep, or roll
/// back through the ledger. A trial whose target changed again since ends
/// with no note. Returns the closing lines, newest state.
pub fn settle_trials(
    config_dir: &Path,
    skills_dir: &Path,
    runs: &[RunMetrics],
    now: u64,
) -> Vec<Version> {
    let mut closed = Vec::new();
    for v in read_versions(config_dir)
        .into_iter()
        .filter(Version::is_open)
    {
        let Some(kind) = ChangeKind::parse(&v.kind) else {
            continue;
        };
        let ledger = ChangeLedger::load_kind(config_dir, kind);
        if ledger.undo_target(&v.id).map(|c| c.seq) != Some(v.seq) {
            let line = v.closed(VersionStatus::Ended, String::new(), now);
            if append_version(config_dir, &line).is_ok() {
                closed.push(line);
            }
            continue;
        }
        let Some(j) = judge(&v, runs, now) else {
            continue;
        };
        let line = if j.keep {
            v.closed(VersionStatus::Kept, j.note, now)
        } else {
            let Some(target) = target_of(&v, config_dir, skills_dir) else {
                continue;
            };
            match rollback_change(config_dir, target.as_ref(), v.seq, &j.note, &Verdict(())) {
                Ok(_) => v.closed(VersionStatus::RolledBack, j.note, now),
                Err(_) => v.closed(VersionStatus::Ended, String::new(), now),
            }
        };
        if append_version(config_dir, &line).is_ok() {
            closed.push(line);
        }
    }
    closed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::changes::record_skill_change;
    use crate::harness::span::Origin;

    const DAY: u64 = 24 * 60 * 60 * 1000;
    const V1: &str = "---\nname: save-page\n---\n\n## Steps\nopen the menu\nclick Save Page As\npick the folder\nclick Save\n";
    const V2: &str = "---\nname: save-page\n---\n\n## Steps\npress ctrl+s\nclick Save\n";

    fn dirs(label: &str) -> (PathBuf, PathBuf) {
        let root = crate::harness::test_dir(label);
        let skills = root.join("skills");
        fs::create_dir_all(&skills).unwrap();
        (root, skills)
    }

    fn put(skills: &Path, body: &str) -> Result<(), String> {
        let dir = skills.join("save-page");
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        fs::write(dir.join("SKILL.md"), body).map_err(|e| e.to_string())
    }

    fn md(skills: &Path) -> String {
        fs::read_to_string(skills.join("save-page").join("SKILL.md")).unwrap()
    }

    /// V1 by the user, then V2 by GrokHub with a trial on it.
    fn trial(root: &Path, skills: &Path) -> Version {
        record_skill_change(root, skills, "save-page", Origin::User, "made it", || {
            put(skills, V1)
        })
        .unwrap();
        let c = record_skill_change(
            root,
            skills,
            "save-page",
            Origin::SelfManage,
            "weekly self-review: Use ctrl+s",
            || put(skills, V2),
        )
        .unwrap()
        .unwrap();
        start_trial(
            root,
            &c,
            "skill:save-page",
            "Use ctrl+s in save-page",
            Metric::Steps,
            V1,
            V2,
        )
        .unwrap()
    }

    fn run(
        v: &Version,
        i: usize,
        arm: Option<Arm>,
        steps: u32,
        result: OutcomeResult,
        at: u64,
    ) -> RunMetrics {
        RunMetrics {
            run: format!("chat:{i}"),
            class: v.class.clone(),
            result,
            steps,
            version: if arm.is_some() {
                v.version.clone()
            } else {
                String::new()
            },
            arm,
            finished_at: at,
            at,
            ..Default::default()
        }
    }

    #[test]
    fn a_trial_records_version_parent_diff_and_target() {
        let (root, skills) = dirs("canary-start");
        let v = trial(&root, &skills);
        let after = crate::harness::changes::content_hash(V2.as_bytes());
        let before = crate::harness::changes::content_hash(V1.as_bytes());
        assert_eq!(v.version, format!("skill:save-page@{}", &after[..12]));
        assert_eq!(v.parent, format!("skill:save-page@{}", &before[..12]));
        assert_eq!(
            (v.seq, v.diff.as_str(), v.target, v.status),
            (2, "+1 -3 lines", Metric::Steps, VersionStatus::Canary)
        );
        assert_eq!((v.share_pct, v.min_runs, v.max_ms), (20, 10, 7 * DAY));
        let raw = fs::read_to_string(root.join("changes").join("changes.jsonl")).unwrap();
        assert_eq!(raw.lines().count(), 1);
        assert_eq!(open_trial(&root, "skill:save-page"), Some(v));
        assert_eq!(open_trial(&root, "skill:other"), None);
    }

    #[test]
    fn the_arm_is_fixed_by_the_run_key_and_near_the_share() {
        let v = "skill:save-page@0123456789ab";
        assert_eq!(pick_arm("chat-a:3", v, 20), pick_arm("chat-a:3", v, 20));
        let canary = (0..1000)
            .filter(|i| pick_arm(&format!("chat:{i}"), v, 20) == Arm::Canary)
            .count();
        assert!(
            (150..=250).contains(&canary),
            "{canary} of 1000 on the canary"
        );
        assert!((0..50).all(|i| pick_arm(&format!("chat:{i}"), v, 0) == Arm::Control));
        assert!((0..50).all(|i| pick_arm(&format!("chat:{i}"), v, 100) == Arm::Canary));
    }

    #[test]
    fn a_better_version_is_kept_with_a_named_note() {
        let (root, skills) = dirs("canary-keep");
        let v = trial(&root, &skills);
        let t = v.started_at;
        let mut runs: Vec<RunMetrics> = (0..12)
            .map(|i| {
                run(
                    &v,
                    i,
                    Some(Arm::Canary),
                    9,
                    OutcomeResult::Success,
                    t + 1 + i as u64,
                )
            })
            .collect();
        runs.extend((20..30).map(|i| {
            run(
                &v,
                i,
                Some(Arm::Control),
                11,
                OutcomeResult::Success,
                t + 1 + i as u64,
            )
        }));
        assert_eq!(
            judge(&v, &runs[..9], t + 100),
            None,
            "9 canary runs and a day left: still running"
        );
        let closed = settle_trials(&root, &skills, &runs, t + 100);
        assert_eq!(closed.len(), 1);
        assert_eq!(closed[0].status, VersionStatus::Kept);
        assert_eq!(
            closed[0].note,
            "Kept: Use ctrl+s in save-page, 18% fewer steps over 12 runs"
        );
        assert_eq!(md(&skills), V2, "kept: the new bytes stay");
        assert_eq!(read_versions(&root)[0].status, VersionStatus::Kept);
        assert_eq!(open_trial(&root, "skill:save-page"), None);
    }

    #[test]
    fn a_worse_version_is_rolled_back_through_the_ledger() {
        let (root, skills) = dirs("canary-rollback");
        let v = trial(&root, &skills);
        let t = v.started_at;
        let mut runs: Vec<RunMetrics> = (0..10)
            .map(|i| run(&v, i, Some(Arm::Canary), 13, OutcomeResult::Success, t + 1))
            .collect();
        runs.extend(
            (20..30).map(|i| run(&v, i, Some(Arm::Control), 10, OutcomeResult::Success, t + 1)),
        );
        let closed = settle_trials(&root, &skills, &runs, t + 100);
        assert_eq!(closed[0].status, VersionStatus::RolledBack);
        assert_eq!(
            closed[0].note,
            "Rolled back: Use ctrl+s in save-page, 30% more steps over 10 runs"
        );
        assert_eq!(md(&skills), V1, "the parent bytes are back");
        let ledger = ChangeLedger::load_kind(&root, ChangeKind::Skill);
        let last = ledger.all().last().unwrap();
        assert_eq!(
            (last.op, last.origin, last.undoes),
            (
                crate::harness::changes::ChangeOp::Undo,
                Origin::SelfManage,
                Some(2)
            )
        );
        assert_eq!(
            last.reason,
            "canary rollback #2: Rolled back: Use ctrl+s in save-page, 30% more steps over 10 runs"
        );
        assert!(
            ledger.undone_by_user(
                "save-page",
                &crate::harness::changes::content_hash(V2.as_bytes())
            ),
            "the weekly pass won't write it again"
        );
        assert!(
            settle_trials(&root, &skills, &runs, t + 200).is_empty(),
            "closed once"
        );
    }

    #[test]
    fn fewer_steps_but_more_failures_or_undos_still_rolls_back() {
        let v = Version {
            version: "skill:x@1".into(),
            class: "skill:x".into(),
            label: "Shorter x".into(),
            min_runs: 10,
            max_ms: 7 * DAY,
            ..Default::default()
        };
        let mut runs: Vec<RunMetrics> = (0..10)
            .map(|i| {
                run(
                    &v,
                    i,
                    Some(Arm::Canary),
                    5,
                    if i < 3 {
                        OutcomeResult::Failure
                    } else {
                        OutcomeResult::Pending
                    },
                    1,
                )
            })
            .collect();
        runs.extend((20..30).map(|i| {
            run(
                &v,
                i,
                Some(Arm::Control),
                9,
                if i < 21 {
                    OutcomeResult::Failure
                } else {
                    OutcomeResult::Success
                },
                1,
            )
        }));
        let j = judge(&v, &runs, 2).unwrap();
        assert_eq!(
            j,
            Judged {
                keep: false,
                note: "Rolled back: Shorter x, 7 of 10 runs worked against 9 of 10 before".into()
            }
        );
        for m in runs.iter_mut() {
            m.result = OutcomeResult::Success;
        }
        runs[0].undos = 1;
        let j = judge(&v, &runs, 2).unwrap();
        assert_eq!(
            j.note,
            "Rolled back: Shorter x, undone in 1 of 10 runs against 0 of 10 before"
        );
        runs[0].undos = 0;
        for m in runs.iter_mut() {
            m.steps = 7;
        }
        assert_eq!(
            judge(&v, &runs, 2).unwrap().note,
            "Rolled back: Shorter x, no change in steps over 10 runs"
        );
    }

    #[test]
    fn with_no_control_runs_the_runs_before_the_trial_are_the_baseline() {
        let v = Version {
            version: "skill:x@1".into(),
            class: "skill:x".into(),
            label: "Shorter x".into(),
            min_runs: 10,
            max_ms: 7 * DAY,
            started_at: 1_000,
            ..Default::default()
        };
        let mut runs: Vec<RunMetrics> = (0..4)
            .map(|i| run(&v, i, None, 20, OutcomeResult::Success, 500))
            .collect();
        runs.extend(
            (10..13).map(|i| run(&v, i, Some(Arm::Canary), 15, OutcomeResult::Success, 1_500)),
        );
        assert_eq!(
            judge(&v, &runs, 1_000 + 6 * DAY),
            None,
            "3 canary runs after 6 days: still running"
        );
        assert_eq!(
            judge(&v, &runs, 1_000 + 7 * DAY).unwrap(),
            Judged {
                keep: true,
                note: "Kept: Shorter x, 25% fewer steps over 3 runs".into()
            }
        );
        let none: Vec<RunMetrics> = runs.iter().filter(|m| m.arm.is_none()).cloned().collect();
        assert_eq!(
            judge(&v, &none, 1_000 + 7 * DAY).unwrap().note,
            "Rolled back: Shorter x, too few runs to compare (0 on the new version)"
        );
    }

    #[test]
    fn a_worse_chat_note_is_removed_and_its_baseline_is_every_class() {
        let (root, skills) = dirs("canary-note");
        let target = SnippetTarget { config_dir: &root, name: "lead" };
        let c = crate::harness::changes::record_change(&root, &target, Origin::SelfManage, "weekly self-review: Lead", || {
            target.put(Some(b"Lead with the answer.\n"))
        })
        .unwrap()
        .unwrap();
        let v = start_trial(&root, &c, ALL_RUNS, "Lead (chat note)", Metric::Steps, "", "Lead with the answer.\n").unwrap();
        assert_eq!((v.parent.as_str(), v.diff.as_str()), ("", "+1 -0 lines"));
        let mut runs: Vec<RunMetrics> = ["topic:inbox", "skill:report"]
            .iter()
            .enumerate()
            .map(|(i, class)| RunMetrics { run: format!("old:{i}"), class: class.to_string(), steps: 4, finished_at: 0, ..Default::default() })
            .collect();
        runs.extend((0..10).map(|i| run(&v, i, Some(Arm::Canary), 5, OutcomeResult::Success, v.started_at + 1)));
        let closed = settle_trials(&root, &skills, &runs, v.started_at + 10);
        assert_eq!(
            (closed[0].status, closed[0].note.as_str()),
            (VersionStatus::RolledBack, "Rolled back: Lead (chat note), 25% more steps over 10 runs")
        );
        assert!(!root.join("tunable").join("lead.md").exists(), "a new note rolled back is gone");
    }

    #[test]
    fn a_trial_whose_skill_changed_again_ends_without_touching_it() {
        let (root, skills) = dirs("canary-ended");
        let v = trial(&root, &skills);
        record_skill_change(&root, &skills, "save-page", Origin::User, "my edit", || {
            put(&skills, "mine\n")
        })
        .unwrap();
        let runs: Vec<RunMetrics> = (0..10)
            .map(|i| {
                run(
                    &v,
                    i,
                    Some(Arm::Canary),
                    30,
                    OutcomeResult::Success,
                    v.started_at + 1,
                )
            })
            .collect();
        let closed = settle_trials(&root, &skills, &runs, v.started_at + 10);
        assert_eq!(
            (closed[0].status, closed[0].note.as_str()),
            (VersionStatus::Ended, "")
        );
        assert_eq!(md(&skills), "mine\n");
    }

    #[test]
    fn a_new_trial_on_the_same_class_ends_the_open_one() {
        let (root, skills) = dirs("canary-replace");
        let first = trial(&root, &skills);
        let c = record_skill_change(
            &root,
            &skills,
            "save-page",
            Origin::SelfManage,
            "weekly self-review: again",
            || put(&skills, V1),
        )
        .unwrap()
        .unwrap();
        let second = start_trial(
            &root,
            &c,
            "skill:save-page",
            "Back to the menu",
            Metric::Steps,
            V2,
            V1,
        )
        .unwrap();
        let all = read_versions(&root);
        assert_eq!(all.len(), 2);
        assert_eq!(
            (all[0].version.as_str(), all[0].status),
            (first.version.as_str(), VersionStatus::Ended)
        );
        assert_eq!(open_trial(&root, "skill:save-page"), Some(second));
    }
}
