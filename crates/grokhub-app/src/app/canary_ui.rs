//! Card 32 PR B in the cabin: a skill change from the weekly pass runs as a
//! trial (`harness::canary`). One run in five that uses the skill follows the
//! new steps, the rest follow the version before; each run's metrics line
//! names its side. When the trial is due, the cabin keeps or rolls back the
//! change and posts one tell-only Home note naming it and the numbers. No
//! Undo or Keep prompt; `/skills undo` still steps a kept change back.

use grokhub_agent::harness as hx;
use grokhub_core::run_metrics::{self, Arm, RunMetrics};
use grokhub_core::SkillMd;

use super::Cabin;
use crate::config;

/// The trial side this chat's skill run is on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TrialPick {
    pub class: String,
    pub version: String,
    pub arm: Arm,
}

/// The tell-only note a trial's start or end posts.
pub(super) fn trial_card(v: &hx::Version) -> grokhub_core::UpdateCard {
    let (title, body) = match v.status {
        hx::VersionStatus::Canary => (
            format!("Trying: {}", v.label),
            format!(
                "{}. One run in {} uses it; it stays if it takes fewer {} over {} runs.",
                v.diff,
                100 / u32::from(v.share_pct.max(1)),
                trial_noun(v.target),
                v.min_runs
            ),
        ),
        hx::VersionStatus::Kept => (format!("Kept: {}", v.label), v.note.clone()),
        _ => (format!("Rolled back: {}", v.label), v.note.clone()),
    };
    let source = format!("canary:{}:{}", v.version, v.status.label());
    grokhub_core::router_update_card(&source, &title, &body, v.at)
}

fn trial_noun(m: hx::Metric) -> &'static str {
    match m {
        hx::Metric::Steps => "steps",
        hx::Metric::WallMs => "seconds",
        hx::Metric::Tokens => "tokens",
        hx::Metric::Cost => "dollars",
    }
}

impl Cabin {
    /// The steps a skill run follows: the new version on the canary side of
    /// an open trial, else the version before it. Remembers the side for
    /// this run's metrics line.
    pub(super) fn trial_follow(&mut self, sk: &SkillMd) -> SkillMd {
        self.harness.self_review.trial = None;
        let dir = config::config_dir();
        let class = grokhub_core::outcome::skill_signature(&sk.name);
        let Some(v) = hx::open_trial(&dir, &class) else {
            return sk.clone();
        };
        let run_key = format!("{}:{}", self.trace_id(), self.turn_no() + 1);
        let arm = hx::pick_arm(&run_key, &v.version, v.share_pct);
        let parent = (arm == Arm::Control)
            .then(|| parent_skill(&dir, &v))
            .flatten();
        if arm == Arm::Control && parent.is_none() {
            return sk.clone();
        }
        self.harness.self_review.trial = Some(TrialPick { class, version: v.version.clone(), arm });
        parent.unwrap_or_else(|| sk.clone())
    }

    /// Name the trial side on this run's metrics line.
    pub(super) fn tag_trial(&self, m: &mut RunMetrics) {
        if let Some(pick) = self.harness.self_review.trial.as_ref().filter(|p| p.class == m.class) {
            m.version = pick.version.clone();
            m.arm = Some(pick.arm);
        }
    }

    /// Judge the open trials on the runs on disk; post one note per trial
    /// that closed with a decision.
    pub(super) fn settle_trials_now(&mut self, now: u64) {
        let dir = config::config_dir();
        if !hx::read_versions(&dir).iter().any(hx::Version::is_open) {
            return;
        }
        let runs = run_metrics::read_runs(&dir);
        let closed = hx::settle_trials(&dir, &crate::skills::skills_dir(), &runs, now);
        if closed.is_empty() {
            return;
        }
        let mut rolled = false;
        for v in &closed {
            if v.status == hx::VersionStatus::RolledBack {
                rolled = true;
            }
            if !v.note.is_empty() {
                self.post_trial_note(v);
            }
        }
        if rolled {
            self.skill_list = crate::skills::list_skills();
            self.harness.skill_rows = None;
        }
    }

    pub(super) fn post_trial_note(&mut self, v: &hx::Version) {
        self.post_feed_card(trial_card(v));
    }

    /// Start a trial for the skill change the weekly pass just wrote.
    pub(super) fn start_skill_trial(&mut self, change: &hx::Change, skill: &str, title: &str, before: &str, after: &str) {
        let dir = config::config_dir();
        let class = grokhub_core::outcome::skill_signature(skill);
        let label = format!("{} in {}", title.trim(), skill);
        if let Ok(v) = hx::start_trial(&dir, change, &class, &label, hx::Metric::Steps, before, after) {
            self.post_trial_note(&v);
        }
    }

    /// True when ledger line `seq` on a skill is on trial: its note already
    /// went out, so it gets no Work-tree row with Undo and Keep.
    pub(super) fn on_trial(&self, kind: hx::ChangeKind, c: &hx::Change) -> bool {
        kind == hx::ChangeKind::Skill
            && hx::read_versions(&config::config_dir()).iter().any(|v| v.kind == c.kind && v.id == c.id && v.seq == c.seq)
    }
}

/// The trial's parent `SKILL.md`, parsed. `None` when it was not kept.
fn parent_skill(dir: &std::path::Path, v: &hx::Version) -> Option<SkillMd> {
    let ledger = hx::ChangeLedger::load_kind(dir, hx::ChangeKind::Skill);
    let line = ledger.all().iter().find(|c| c.seq == v.seq && c.id == v.id)?;
    if line.before_hash.is_empty() {
        return None;
    }
    let bytes = hx::kept_bytes(dir, hx::ChangeKind::Skill, &v.id, &line.before_hash).ok()?;
    Some(grokhub_core::parse_skill_md(&String::from_utf8_lossy(&bytes)))
}
