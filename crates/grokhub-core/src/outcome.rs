//! Per-task outcomes (harness design P7, Spike-7). One record per finished
//! task, keyed by its signature: the skill it ran (`skill:<name>`) or the
//! topic of the ask (`topic:<words>`, the `same_topic` words). Records go to
//! `{config}/outcomes.jsonl`, append-only and rotated like the trajectory
//! file. A late fact (an undo within 24 hours, a correction) is a new line
//! for the same `task`; the newest line wins when the file is read.
//!
//! - `success`: VERIFY_OK, accepted, or no undo within [`UNDO_WINDOW_MS`].
//! - `failure`: a detector finding, a deny, an undo, a correction, or the
//!   retry ladder ran out.
//!
//! Records hold ids, counts and short reason words only: no reply text, no
//! args, no secrets. `signature`, `span_ids`, `episode` and `tokens` are
//! there so the router (§14 R3) can join on them later.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::redact::redact_secrets;

pub const OUTCOMES_FILE: &str = "outcomes.jsonl";
/// The file is cut to its newer half past this size.
pub const OUTCOMES_MAX_BYTES: usize = 2 * 1024 * 1024;
/// A pending task with no undo in this window counts as a success.
pub const UNDO_WINDOW_MS: u64 = 24 * 60 * 60 * 1000;
/// Span ids kept on one record.
pub const SPAN_IDS_CAP: usize = 40;
/// Topic words kept in a `topic:` signature.
const TOPIC_WORDS_CAP: usize = 6;

pub const REASON_VERIFY_OK: &str = "verify_ok";
pub const REASON_ACCEPTED: &str = "accepted";
pub const REASON_NO_UNDO: &str = "no_undo_24h";
pub const REASON_DENY: &str = "deny";
pub const REASON_UNDO: &str = "undo";
pub const REASON_CORRECTION: &str = "correction";
pub const REASON_RETRY_EXHAUSTED: &str = "retry_exhausted";
/// Prefix for a detector finding (`finding:action_loop`).
pub const REASON_FINDING: &str = "finding:";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutcomeResult {
    Success,
    Failure,
    /// Nothing decided yet; becomes a success after [`UNDO_WINDOW_MS`].
    #[default]
    Pending,
}

/// Tokens the task's routed model calls used (`route::CallTokens` summed).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutcomeTokens {
    #[serde(default, rename = "in")]
    pub input: u64,
    #[serde(default)]
    pub cached: u64,
    #[serde(default)]
    pub out: u64,
    #[serde(default)]
    pub reasoning: u64,
    #[serde(default)]
    pub cost_ticks: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskOutcome {
    /// One finished task: `chat:turn`, or the episode id.
    pub task: String,
    pub signature: String,
    pub result: OutcomeResult,
    #[serde(default)]
    pub reasons: Vec<String>,
    #[serde(default)]
    pub span_ids: Vec<String>,
    #[serde(default)]
    pub episode: String,
    pub finished_at: u64,
    #[serde(default)]
    pub tokens: OutcomeTokens,
    /// When this line was written. A later line for the same task wins.
    #[serde(default)]
    pub at: u64,
}

/// What the end of a task saw. Failure signals win over success signals.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OutcomeSignals {
    pub verify_ok: bool,
    pub accepted: bool,
    /// Detector ids that fired on the task's spans.
    pub findings: Vec<String>,
    pub denied: bool,
    pub undone: bool,
    pub corrected: bool,
    pub retry_exhausted: bool,
}

/// The result and its reasons, in a fixed order.
pub fn classify(sig: &OutcomeSignals) -> (OutcomeResult, Vec<String>) {
    let mut fail: Vec<String> = Vec::new();
    for f in &sig.findings {
        let r = format!("{REASON_FINDING}{f}");
        if !fail.contains(&r) {
            fail.push(r);
        }
    }
    for (on, r) in [
        (sig.denied, REASON_DENY),
        (sig.undone, REASON_UNDO),
        (sig.corrected, REASON_CORRECTION),
        (sig.retry_exhausted, REASON_RETRY_EXHAUSTED),
    ] {
        if on {
            fail.push(r.into());
        }
    }
    if !fail.is_empty() {
        return (OutcomeResult::Failure, fail);
    }
    let mut ok = Vec::new();
    if sig.verify_ok {
        ok.push(REASON_VERIFY_OK.to_string());
    }
    if sig.accepted {
        ok.push(REASON_ACCEPTED.to_string());
    }
    if ok.is_empty() {
        (OutcomeResult::Pending, Vec::new())
    } else {
        (OutcomeResult::Success, ok)
    }
}

impl TaskOutcome {
    /// A record for a task that just finished.
    pub fn finished(task: &str, signature: &str, sig: &OutcomeSignals, finished_at: u64) -> Self {
        let (result, reasons) = classify(sig);
        Self {
            task: redact_secrets(task),
            signature: redact_secrets(signature),
            result,
            reasons,
            finished_at,
            at: finished_at,
            ..Self::default()
        }
    }

    pub fn with_spans(mut self, ids: impl IntoIterator<Item = String>) -> Self {
        self.span_ids = ids
            .into_iter()
            .map(|s| redact_secrets(&s))
            .take(SPAN_IDS_CAP)
            .collect();
        self
    }

    pub fn in_episode(mut self, episode: &str) -> Self {
        self.episode = redact_secrets(episode);
        self
    }

    pub fn with_tokens(mut self, tokens: OutcomeTokens) -> Self {
        self.tokens = tokens;
        self
    }

    /// The result as of `now`: a pending task past the undo window is a success.
    pub fn effective(&self, now: u64) -> OutcomeResult {
        match self.result {
            OutcomeResult::Pending if now.saturating_sub(self.finished_at) >= UNDO_WINDOW_MS => {
                OutcomeResult::Success
            }
            r => r,
        }
    }

    /// Reasons as of `now`, with `no_undo_24h` on a pending task past the window.
    pub fn effective_reasons(&self, now: u64) -> Vec<String> {
        if self.result == OutcomeResult::Pending && self.effective(now) == OutcomeResult::Success {
            return vec![REASON_NO_UNDO.into()];
        }
        self.reasons.clone()
    }

    /// The superseding failure line for a late fact (`undo`, `correction`).
    pub fn superseded(&self, reason: &str, at: u64) -> Self {
        let mut next = self.clone();
        next.result = OutcomeResult::Failure;
        next.reasons
            .retain(|r| r.starts_with(REASON_FINDING) || r == REASON_DENY);
        if !next.reasons.iter().any(|r| r == reason) {
            next.reasons.push(reason.into());
        }
        next.at = at;
        next
    }

    pub fn skill(&self) -> Option<&str> {
        self.signature.strip_prefix("skill:")
    }

    pub fn topic(&self) -> Option<&str> {
        self.signature.strip_prefix("topic:")
    }
}

/// `skill:<name>` for a task a skill ran.
pub fn skill_signature(name: &str) -> String {
    format!("skill:{}", redact_secrets(name.trim()))
}

/// `topic:<words>` for an ask no skill ran: its first topic words, so two
/// asks about the same thing share most of the key. Empty when the ask has none.
pub fn topic_signature(ask: &str) -> String {
    let words = crate::ideas::topic_words(&redact_secrets(ask));
    if words.is_empty() {
        return String::new();
    }
    let words: Vec<&str> = words
        .iter()
        .take(TOPIC_WORDS_CAP)
        .map(String::as_str)
        .collect();
    format!("topic:{}", words.join(" "))
}

/// The next message reads as "that was wrong".
pub fn is_correction(text: &str) -> bool {
    let t = text.trim().to_ascii_lowercase();
    const OPENERS: &[&str] = &[
        "no,",
        "no.",
        "no ",
        "nope",
        "wrong",
        "that's wrong",
        "thats wrong",
        "that is wrong",
        "that's not",
        "that is not",
        "not what i",
        "undo",
        "revert",
        "you broke",
        "it didn't",
        "it did not",
        "didn't work",
        "doesn't work",
        "that failed",
    ];
    t == "no" || OPENERS.iter().any(|o| t.starts_with(o))
}

pub fn outcomes_path(config_dir: &Path) -> PathBuf {
    config_dir.join(OUTCOMES_FILE)
}

/// Append one line, then cut the file to its newer half when it outgrew
/// [`OUTCOMES_MAX_BYTES`].
pub fn append_outcome(config_dir: &Path, outcome: &TaskOutcome) -> Result<(), String> {
    let line = serde_json::to_string(outcome).map_err(|e| e.to_string())?;
    let path = outcomes_path(config_dir);
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
    let len = fs::metadata(&path).map(|m| m.len() as usize).unwrap_or(0);
    if len > OUTCOMES_MAX_BYTES {
        let raw = fs::read_to_string(&path).map_err(|e| e.to_string())?;
        let kept = crate::trajectory::rotate_trajectory(&raw, OUTCOMES_MAX_BYTES);
        fs::write(&path, kept).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Every line on disk, oldest first. Bad lines are skipped.
pub fn read_outcome_lines(config_dir: &Path) -> Vec<TaskOutcome> {
    let raw = fs::read_to_string(outcomes_path(config_dir)).unwrap_or_default();
    raw.lines()
        .filter_map(|l| serde_json::from_str(l.trim()).ok())
        .collect()
}

/// One record per task: the newest line wins. Sorted by `finished_at`.
pub fn fold_outcomes(lines: Vec<TaskOutcome>) -> Vec<TaskOutcome> {
    let mut out: Vec<TaskOutcome> = Vec::new();
    for line in lines {
        match out.iter_mut().find(|o| o.task == line.task) {
            Some(prev) if line.at >= prev.at => *prev = line,
            Some(_) => {}
            None => out.push(line),
        }
    }
    out.sort_by(|a, b| a.finished_at.cmp(&b.finished_at).then(a.task.cmp(&b.task)));
    out
}

/// One record per finished task, as of the newest line.
pub fn read_outcomes(config_dir: &Path) -> Vec<TaskOutcome> {
    fold_outcomes(read_outcome_lines(config_dir))
}

/// The lines an undo of `signature` at `undo_at` supersedes: its tasks that
/// finished in the 24 hours before and are not failures already.
pub fn undo_supersedes(
    outcomes: &[TaskOutcome],
    signature: &str,
    undo_at: u64,
) -> Vec<TaskOutcome> {
    outcomes
        .iter()
        .filter(|o| {
            o.signature == signature
                && o.result != OutcomeResult::Failure
                && o.finished_at <= undo_at
                && undo_at - o.finished_at < UNDO_WINDOW_MS
        })
        .map(|o| o.superseded(REASON_UNDO, undo_at))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("grokhub-outcome-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn signals_classify_with_failures_first() {
        let ok = OutcomeSignals {
            verify_ok: true,
            ..Default::default()
        };
        assert_eq!(
            classify(&ok),
            (OutcomeResult::Success, vec!["verify_ok".to_string()])
        );
        let both = OutcomeSignals {
            verify_ok: true,
            findings: vec!["action_loop".into(), "action_loop".into()],
            denied: true,
            ..Default::default()
        };
        assert_eq!(
            classify(&both),
            (
                OutcomeResult::Failure,
                vec!["finding:action_loop".to_string(), "deny".to_string()]
            )
        );
        assert_eq!(
            classify(&OutcomeSignals::default()),
            (OutcomeResult::Pending, Vec::new())
        );
        let ladder = OutcomeSignals {
            retry_exhausted: true,
            accepted: true,
            ..Default::default()
        };
        assert_eq!(
            classify(&ladder),
            (OutcomeResult::Failure, vec!["retry_exhausted".to_string()])
        );
    }

    #[test]
    fn pending_turns_success_after_a_day_without_undo() {
        let o = TaskOutcome::finished(
            "c:1",
            "skill:weekly-report",
            &OutcomeSignals::default(),
            1_000,
        );
        assert_eq!(
            o.effective(1_000 + UNDO_WINDOW_MS - 1),
            OutcomeResult::Pending
        );
        assert_eq!(o.effective(1_000 + UNDO_WINDOW_MS), OutcomeResult::Success);
        assert_eq!(
            o.effective_reasons(1_000 + UNDO_WINDOW_MS),
            vec!["no_undo_24h".to_string()]
        );
    }

    #[test]
    fn one_record_per_task_and_the_superseding_undo_line_wins() {
        let d = dir("fold");
        let sig = OutcomeSignals {
            verify_ok: true,
            ..Default::default()
        };
        let a = TaskOutcome::finished("chat-a:1", "skill:weekly-report", &sig, 10_000)
            .with_spans(["chat-a:9".to_string()]);
        let b = TaskOutcome::finished(
            "chat-a:2",
            "skill:weekly-report",
            &OutcomeSignals::default(),
            20_000,
        );
        let c = TaskOutcome::finished("chat-b:1", "topic:invoice march", &sig, 30_000);
        for o in [&a, &b, &c] {
            append_outcome(&d, o).unwrap();
        }
        let now = 40_000;
        let late = undo_supersedes(&read_outcomes(&d), "skill:weekly-report", now);
        assert_eq!(
            late.len(),
            2,
            "both weekly-report tasks finished inside the window"
        );
        for o in &late {
            append_outcome(&d, o).unwrap();
        }
        assert_eq!(read_outcome_lines(&d).len(), 5);
        let all = read_outcomes(&d);
        let tasks: Vec<(&str, OutcomeResult)> =
            all.iter().map(|o| (o.task.as_str(), o.result)).collect();
        assert_eq!(
            tasks,
            vec![
                ("chat-a:1", OutcomeResult::Failure),
                ("chat-a:2", OutcomeResult::Failure),
                ("chat-b:1", OutcomeResult::Success),
            ]
        );
        assert_eq!(all[0].reasons, vec!["undo".to_string()]);
        assert_eq!(
            all[0].span_ids,
            vec!["chat-a:9".to_string()],
            "the late line keeps the spans"
        );
        assert_eq!(all[0].finished_at, 10_000);
        assert!(
            undo_supersedes(&all, "skill:weekly-report", now + 5).is_empty(),
            "already failures"
        );
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn an_undo_after_the_window_supersedes_nothing() {
        let o = TaskOutcome::finished("c:1", "skill:x", &OutcomeSignals::default(), 0);
        assert!(undo_supersedes(&[o], "skill:x", UNDO_WINDOW_MS).is_empty());
    }

    #[test]
    fn signatures_and_corrections() {
        assert_eq!(skill_signature(" weekly-report "), "skill:weekly-report");
        assert_eq!(
            topic_signature("Please summarize the March invoices"),
            "topic:summarize march invoic"
        );
        assert_eq!(topic_signature("ok"), "");
        assert!(is_correction("No, that's the wrong sheet"));
        assert!(is_correction("that's wrong"));
        assert!(!is_correction("now do the April ones"));
        assert!(!is_correction("nothing else, thanks"));
    }

    #[test]
    fn records_hold_no_secrets() {
        let o = TaskOutcome::finished(
            "chat:1",
            "topic:sk-abcdefghijklmnopqrstuv",
            &OutcomeSignals::default(),
            1,
        )
        .with_spans(["sk-abcdefghijklmnopqrstuv:2".to_string()]);
        let line = serde_json::to_string(&o).unwrap();
        assert!(!line.contains("sk-abcdefghijklmnopqrstuv"), "{line}");
    }

    #[test]
    fn rotation_keeps_the_newest_lines() {
        let d = dir("rotate");
        let big = "x".repeat(4096);
        let mut n = 0u64;
        while fs::metadata(outcomes_path(&d))
            .map(|m| m.len() as usize)
            .unwrap_or(0)
            <= OUTCOMES_MAX_BYTES - 8_000
        {
            let mut o =
                TaskOutcome::finished(&format!("t:{n}"), "skill:x", &OutcomeSignals::default(), n);
            o.episode = big.clone();
            append_outcome(&d, &o).unwrap();
            n += 1;
        }
        let mut last = TaskOutcome::finished("t:last", "skill:x", &OutcomeSignals::default(), n);
        last.episode = big.repeat(3);
        append_outcome(&d, &last).unwrap();
        let len = fs::metadata(outcomes_path(&d)).unwrap().len() as usize;
        assert!(len <= OUTCOMES_MAX_BYTES, "{len}");
        let lines = read_outcome_lines(&d);
        assert_eq!(lines.last().map(|o| o.task.as_str()), Some("t:last"));
        assert!(
            lines.iter().all(|o| o.task != "t:0"),
            "the oldest half went"
        );
        let _ = fs::remove_dir_all(&d);
    }
}
