//! Per-run metrics (card 32, step 1 of the bounded self-improvement loop).
//! One record per finished run in `{config}/metrics/runs.jsonl`: whether it
//! worked and was verified, how many steps and how long, the tokens and a
//! cost estimate, user corrections and undos, recoveries and replans, the
//! model and the task class. Built from the run's [`TaskOutcome`] plus counts
//! from its spans.
//!
//! Local only: nothing here touches the network. A late fact (a correction,
//! an undo) is a new line for the same `run`; the newest line wins when the
//! file is read. Lines whose run finished before the retention window
//! (Settings → Memory retention) are dropped on the next write.

use std::fs;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::outcome::{OutcomeResult, OutcomeTokens, TaskOutcome, REASON_VERIFY_OK};
use crate::redact::redact_secrets;

pub const METRICS_DIR: &str = "metrics";
pub const RUNS_FILE: &str = "runs.jsonl";
/// The file is cut to its newer half past this size.
pub const RUNS_MAX_BYTES: usize = 2 * 1024 * 1024;
/// xAI cost ticks per US dollar (a tick is 1e-10 USD).
pub const TICKS_PER_USD: f64 = 1e10;
const DAY_MS: u64 = 24 * 60 * 60 * 1000;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RunMetrics {
    /// The run: `chat:turn`, same as the outcome's `task`.
    pub run: String,
    /// Task class: the outcome signature (`skill:<name>` or `topic:<words>`).
    pub class: String,
    pub result: OutcomeResult,
    pub verified: bool,
    /// Tool steps that ran (desktop inputs and other allowed steps).
    pub steps: u32,
    /// First span to last span of the run.
    pub wall_ms: u64,
    pub tokens: OutcomeTokens,
    /// `tokens.cost_ticks` in US dollars.
    pub cost_usd: f64,
    #[serde(default)]
    pub corrections: u32,
    #[serde(default)]
    pub undos: u32,
    /// Recovery ladder steps (retry, backtrack, replan, pause).
    #[serde(default)]
    pub recoveries: u32,
    /// The replans among `recoveries`.
    #[serde(default)]
    pub replans: u32,
    /// The model of the run's last routed call. Empty when it made none.
    #[serde(default)]
    pub model: String,
    /// The self-change version on trial for this run's class (card 32 PR B),
    /// and which side of the trial the run was on. Empty and `None` otherwise.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arm: Option<Arm>,
    /// Brain 6: the run's Brief and what it picked. Empty on older lines.
    #[serde(default, skip_serializing_if = "RunBrain::is_empty")]
    pub brain: RunBrain,
    pub finished_at: u64,
    /// When this line was written. A later line for the same run wins.
    pub at: u64,
}

/// The run's Brief (intent, care, domain), the policy row it matched
/// ("repair × critical") and the playbook it picked, by id.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunBrain {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub intent: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub care: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub domain: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub policy: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub playbook: String,
}

impl RunBrain {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// True when a trial on `class` (`policy:<row>` or `playbook:<id>`)
    /// covers this run.
    pub fn in_class(&self, class: &str) -> bool {
        match class.split_once(':') {
            Some(("policy", row)) => !row.is_empty() && self.policy == row,
            Some(("playbook", id)) => !id.is_empty() && self.playbook == id,
            _ => false,
        }
    }
}

/// The side of a canary trial a run was on: the new version, or its parent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Arm {
    Canary,
    Control,
}

impl RunMetrics {
    /// The outcome's share of the record; span counts come from the caller.
    pub fn from_outcome(o: &TaskOutcome) -> Self {
        Self {
            run: o.task.clone(),
            class: o.signature.clone(),
            result: o.result,
            verified: o.reasons.iter().any(|r| r == REASON_VERIFY_OK),
            tokens: o.tokens,
            cost_usd: o.tokens.cost_ticks as f64 / TICKS_PER_USD,
            finished_at: o.finished_at,
            at: o.at,
            ..Self::default()
        }
    }

    pub fn with_model(mut self, model: &str) -> Self {
        self.model = redact_secrets(model);
        self
    }

    /// The user said the run was wrong.
    pub fn corrected(&self, at: u64) -> Self {
        let mut next = self.clone();
        next.corrections += 1;
        next.result = OutcomeResult::Failure;
        next.at = at;
        next
    }

    /// The change the run made was undone.
    pub fn undone(&self, at: u64) -> Self {
        let mut next = self.clone();
        next.undos += 1;
        next.result = OutcomeResult::Failure;
        next.at = at;
        next
    }
}

pub fn runs_path(config_dir: &Path) -> PathBuf {
    config_dir.join(METRICS_DIR).join(RUNS_FILE)
}

/// Runs that finished before this are past retention. `days == 0` keeps all.
pub fn retention_cutoff(now: u64, days: u32) -> u64 {
    if days == 0 {
        0
    } else {
        now.saturating_sub(u64::from(days) * DAY_MS)
    }
}

/// Append one line. Then, when the oldest line is past `retention_days`,
/// drop every line that is; and cut the file to its newer half when it
/// outgrew [`RUNS_MAX_BYTES`].
pub fn append_run(config_dir: &Path, m: &RunMetrics, retention_days: u32) -> Result<(), String> {
    let line = serde_json::to_string(m).map_err(|e| e.to_string())?;
    let path = runs_path(config_dir);
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let mut f = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| e.to_string())?;
    f.write_all(format!("{line}\n").as_bytes())
        .map_err(|e| e.to_string())?;
    drop(f);
    let cutoff = retention_cutoff(m.at, retention_days);
    if oldest_finished_at(&path).is_some_and(|t| t < cutoff) {
        prune_runs(config_dir, cutoff)?;
    }
    let len = fs::metadata(&path).map(|m| m.len() as usize).unwrap_or(0);
    if len > RUNS_MAX_BYTES {
        let raw = fs::read_to_string(&path).map_err(|e| e.to_string())?;
        let kept = crate::trajectory::rotate_trajectory(&raw, RUNS_MAX_BYTES);
        fs::write(&path, kept).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// `finished_at` of the file's first readable line.
fn oldest_finished_at(path: &Path) -> Option<u64> {
    let f = fs::File::open(path).ok()?;
    std::io::BufReader::new(f)
        .lines()
        .map_while(Result::ok)
        .find_map(|l| serde_json::from_str::<RunMetrics>(l.trim()).ok())
        .map(|m| m.finished_at)
}

/// Drop the lines of runs that finished before `cutoff`. Returns how many went.
pub fn prune_runs(config_dir: &Path, cutoff: u64) -> Result<usize, String> {
    let path = runs_path(config_dir);
    let Ok(raw) = fs::read_to_string(&path) else {
        return Ok(0);
    };
    let mut kept = String::new();
    let mut dropped = 0;
    for l in raw.lines() {
        match serde_json::from_str::<RunMetrics>(l.trim()) {
            Ok(m) if m.finished_at < cutoff => dropped += 1,
            Ok(_) => {
                kept.push_str(l);
                kept.push('\n');
            }
            Err(_) => dropped += 1,
        }
    }
    if dropped > 0 {
        fs::write(&path, kept).map_err(|e| e.to_string())?;
    }
    Ok(dropped)
}

/// Every line on disk, oldest first. Bad lines are skipped.
pub fn read_run_lines(config_dir: &Path) -> Vec<RunMetrics> {
    let raw = fs::read_to_string(runs_path(config_dir)).unwrap_or_default();
    raw.lines()
        .filter_map(|l| serde_json::from_str(l.trim()).ok())
        .collect()
}

/// One record per run: the newest line wins. Sorted by `finished_at`.
pub fn fold_runs(lines: Vec<RunMetrics>) -> Vec<RunMetrics> {
    let mut out: Vec<RunMetrics> = Vec::new();
    for line in lines {
        match out.iter_mut().find(|o| o.run == line.run) {
            Some(prev) if line.at >= prev.at => *prev = line,
            Some(_) => {}
            None => out.push(line),
        }
    }
    out.sort_by(|a, b| a.finished_at.cmp(&b.finished_at).then(a.run.cmp(&b.run)));
    out
}

/// One record per finished run, as of the newest line.
pub fn read_runs(config_dir: &Path) -> Vec<RunMetrics> {
    fold_runs(read_run_lines(config_dir))
}

/// The newest record of `run`, if any.
pub fn find_run(config_dir: &Path, run: &str) -> Option<RunMetrics> {
    read_runs(config_dir).into_iter().find(|m| m.run == run)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::outcome::OutcomeSignals;

    fn dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("grokhub-run-metrics-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn run(task: &str, finished_at: u64) -> RunMetrics {
        let sig = OutcomeSignals {
            verify_ok: true,
            ..Default::default()
        };
        let o = TaskOutcome::finished(task, "skill:weekly-report", &sig, finished_at).with_tokens(OutcomeTokens {
            input: 1_200,
            cached: 300,
            out: 250,
            reasoning: 40,
            cost_ticks: 25_000_000,
        });
        let mut m = RunMetrics::from_outcome(&o).with_model("grok-4-fast");
        m.steps = 4;
        m.wall_ms = 9_500;
        m.recoveries = 2;
        m.replans = 1;
        m
    }

    #[test]
    fn a_finished_run_writes_one_line_with_every_field() {
        let d = dir("fields");
        let now = 1_000 * DAY_MS;
        append_run(&d, &run("chat-a:3", now), 90).unwrap();
        let raw = fs::read_to_string(d.join("metrics").join("runs.jsonl")).unwrap();
        assert_eq!(raw.lines().count(), 1);
        let v: serde_json::Value = serde_json::from_str(raw.trim()).unwrap();
        assert_eq!(v["run"], "chat-a:3");
        assert_eq!(v["class"], "skill:weekly-report");
        assert_eq!(v["result"], "success");
        assert_eq!(v["verified"], true);
        assert_eq!(v["steps"], 4);
        assert_eq!(v["wall_ms"], 9_500);
        assert_eq!(v["tokens"]["in"], 1_200);
        assert_eq!(v["tokens"]["out"], 250);
        assert_eq!(v["tokens"]["cost_ticks"], 25_000_000);
        assert_eq!(v["cost_usd"], 0.0025);
        assert_eq!(v["corrections"], 0);
        assert_eq!(v["undos"], 0);
        assert_eq!(v["recoveries"], 2);
        assert_eq!(v["replans"], 1);
        assert_eq!(v["model"], "grok-4-fast");
        assert_eq!(v["finished_at"], now);
        assert_eq!(read_runs(&d), vec![run("chat-a:3", now)]);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_correction_and_an_undo_are_newer_lines_for_the_same_run() {
        let d = dir("late");
        let now = 1_000 * DAY_MS;
        let first = run("chat-a:3", now);
        append_run(&d, &first, 90).unwrap();
        append_run(&d, &run("chat-a:4", now + 5), 90).unwrap();
        append_run(&d, &first.corrected(now + 10), 90).unwrap();
        let undone = find_run(&d, "chat-a:3").unwrap().undone(now + 20);
        append_run(&d, &undone, 90).unwrap();
        assert_eq!(read_run_lines(&d).len(), 4);
        let runs = read_runs(&d);
        assert_eq!(runs.len(), 2);
        assert_eq!(
            (runs[0].run.as_str(), runs[0].result, runs[0].corrections, runs[0].undos, runs[0].at),
            ("chat-a:3", OutcomeResult::Failure, 1, 1, now + 20)
        );
        assert_eq!(
            (runs[1].run.as_str(), runs[1].result, runs[1].corrections, runs[1].undos),
            ("chat-a:4", OutcomeResult::Success, 0, 0)
        );
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn runs_past_retention_are_dropped_on_the_next_write_and_zero_keeps_all() {
        let d = dir("retention");
        let now = 1_000 * DAY_MS;
        append_run(&d, &run("old:1", now - 31 * DAY_MS), 30).unwrap();
        append_run(&d, &run("mid:1", now - 29 * DAY_MS), 30).unwrap();
        assert_eq!(read_run_lines(&d).len(), 2, "nothing past 30 days yet");
        append_run(&d, &run("new:1", now), 0).unwrap();
        assert_eq!(read_run_lines(&d).len(), 3, "0 keeps everything");
        append_run(&d, &run("new:2", now + 1), 30).unwrap();
        let left: Vec<String> = read_runs(&d).into_iter().map(|m| m.run).collect();
        assert_eq!(left, vec!["mid:1", "new:1", "new:2"]);
        assert_eq!(retention_cutoff(now, 0), 0);
        assert_eq!(retention_cutoff(now, 30), now - 30 * DAY_MS);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_line_carries_the_brief_policy_and_playbook_and_an_old_line_still_parses() {
        let d = dir("brain");
        let now = 1_000 * DAY_MS;
        let mut m = run("chat-a:5", now);
        m.brain = RunBrain {
            intent: "repair".into(),
            care: "critical".into(),
            domain: "system".into(),
            policy: "repair × critical".into(),
            playbook: "repair-linux".into(),
        };
        append_run(&d, &m, 90).unwrap();
        let raw = fs::read_to_string(runs_path(&d)).unwrap();
        let v: serde_json::Value = serde_json::from_str(raw.trim()).unwrap();
        assert_eq!(
            v["brain"],
            serde_json::json!({
                "intent": "repair",
                "care": "critical",
                "domain": "system",
                "policy": "repair × critical",
                "playbook": "repair-linux"
            })
        );
        assert_eq!(read_runs(&d)[0].brain.playbook, "repair-linux");
        assert!(m.brain.in_class("policy:repair × critical"));
        assert!(m.brain.in_class("playbook:repair-linux"));
        assert!(!m.brain.in_class("playbook:repair-windows"));
        assert!(!m.brain.in_class("skill:repair-linux"));
        assert!(!RunBrain::default().in_class("playbook:"));
        // A line from before Brain 6 has no `brain` and reads with it empty.
        let old = r#"{"run":"chat-a:1","class":"skill:weekly-report","result":"success","verified":true,"steps":3,"wall_ms":10,"tokens":{"in":1,"out":2},"cost_usd":0.0,"finished_at":5,"at":5}"#;
        let parsed: RunMetrics = serde_json::from_str(old).unwrap();
        assert_eq!((parsed.run.as_str(), parsed.steps, parsed.brain.clone()), ("chat-a:1", 3, RunBrain::default()));
        let again: serde_json::Value = serde_json::from_str(&serde_json::to_string(&parsed).unwrap()).unwrap();
        assert!(again.get("brain").is_none(), "an empty brain is not written");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn an_unverified_run_with_no_calls_costs_nothing() {
        let o = TaskOutcome::finished("chat-b:1", "topic:open notes", &OutcomeSignals::default(), 7);
        let m = RunMetrics::from_outcome(&o);
        assert_eq!(
            (m.result, m.verified, m.cost_usd, m.model.as_str(), m.at),
            (OutcomeResult::Pending, false, 0.0, "", 7)
        );
    }
}
