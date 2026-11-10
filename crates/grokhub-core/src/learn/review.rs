//! What the engine measured about the app itself, for the two slower levels
//! of the self-improvement loop.
//!
//! Tuning moves settings, chips and model choices as beliefs firm up. This
//! is for what tuning can't fix: a skill or a model that keeps failing, and
//! an area whose own changes are kept less than they were.
//!
//! - [`digest`] goes into the weekly self-review's input, so its skill
//!   patches and code ideas rest on these numbers too.
//! - [`report`] is a fault report for the developers. The app never sends
//!   it: `/learned report` shows it and the user decides what to do with it.
//!   It holds ids the app ships with and counts. Skill names are the user's
//!   own, so a failing skill is in the digest and never in the report.

use super::belief::{Belief, Beliefs};
use super::decide::Area;
use super::score::{Board, Trend};

/// Runs behind a rate before it counts as a fault.
pub const MIN_EVIDENCE: f32 = 10.0;
/// A skill or a model that goes well less often than this is a fault.
pub const WEAK_RATE: f32 = 0.5;
/// Faults listed at most, the worst first.
pub const FAULTS_CAP: usize = 8;

/// One thing the app measured going wrong.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fault {
    /// A skill or a model went well in `good` of `runs` runs.
    Unreliable {
        subject: String,
        good: u32,
        runs: u32,
    },
    /// The share of this area's changes that were kept fell from `before`
    /// to `now` (0 to 100). `resting`: it only offers for now.
    Slipped {
        area: Area,
        now: u8,
        before: u8,
        resting: bool,
    },
}

impl Fault {
    /// 0 to 100: how well the thing goes. Lower is worse.
    fn health(&self) -> u32 {
        match self {
            Self::Unreliable { good, runs, .. } => good * 100 / (*runs).max(1),
            Self::Slipped { now, .. } => u32::from(*now),
        }
    }

    /// Whether the developers can do something about it without knowing
    /// anything the user made.
    fn for_developers(&self) -> bool {
        match self {
            Self::Unreliable { subject, .. } => subject.starts_with("model:"),
            Self::Slipped { .. } => true,
        }
    }

    /// One line, in ids and numbers.
    pub fn line(&self) -> String {
        match self {
            Self::Unreliable {
                subject,
                good,
                runs,
            } => format!(
                "`{subject}` went well in {good} of its last {runs} runs ({}%).",
                self.health()
            ),
            Self::Slipped {
                area,
                now,
                before,
                resting,
            } => format!(
                "{}: {now}% of the app's own changes were kept in the last four weeks, down from {before}%.{}",
                area.label(),
                if *resting {
                    " It only offers for now."
                } else {
                    ""
                }
            ),
        }
    }
}

fn unreliable(subject: &str, b: &Belief, now_ms: u64) -> Option<Fault> {
    if !matches!(subject.split(':').next(), Some("skill" | "model")) {
        return None;
    }
    let (runs, rate) = (b.evidence(now_ms), b.rate()?);
    (runs >= MIN_EVIDENCE && rate < WEAK_RATE).then(|| Fault::Unreliable {
        subject: subject.to_string(),
        good: (rate * runs).round() as u32,
        runs: runs.round() as u32,
    })
}

/// What is going wrong by the app's own numbers, the worst first.
pub fn faults(beliefs: &Beliefs, board: &Board, now_ms: u64) -> Vec<Fault> {
    let mut out: Vec<Fault> = beliefs
        .iter()
        .filter_map(|(s, b)| unreliable(s.as_str(), b, now_ms))
        .collect();
    for area in Area::ALL {
        let score = board.score(area, now_ms);
        if let (Trend::Worse, Some(now), Some(before)) = (score.trend, score.now, score.before) {
            out.push(Fault::Slipped {
                area,
                now,
                before,
                resting: board.resting(area, now_ms),
            });
        }
    }
    out.sort_by_key(Fault::health);
    out.truncate(FAULTS_CAP);
    out
}

/// The lines the weekly self-review reads after its own. Empty with no
/// faults, so a good week costs no tokens.
pub fn digest(faults: &[Fault]) -> String {
    if faults.is_empty() {
        return String::new();
    }
    let mut out = String::from("What the app measured about itself (ids and counts only):\n");
    for f in faults {
        out.push_str(&format!("- {}\n", f.line()));
    }
    out
}

pub const REPORT_TITLE: &str = "What the app measured going wrong";
pub const NOTHING_TO_REPORT: &str =
    "Nothing to report. By its own numbers the app has no fault that needs its developers.";
pub const REPORT_NOTE: &str = "Nothing was sent. This is the report: it holds ids and counts, and nothing you typed. Copy it into an issue for the developers if you want them to see it.";

/// The fault report, as an issue body the user copies by hand. `None` when
/// nothing needs the developers.
pub fn report(faults: &[Fault], version: &str) -> Option<String> {
    let lines: Vec<String> = faults
        .iter()
        .filter(|f| f.for_developers())
        .map(|f| format!("- {}", f.line()))
        .collect();
    (!lines.is_empty()).then(|| {
        crate::self_review::redact_for_issue(
            &format!(
                "## {REPORT_TITLE}\n\n{}\n\n---\nMeasured by {} on one PC. Ids and counts only.\n",
                lines.join("\n"),
                version.trim()
            ),
            None,
        )
    })
}
