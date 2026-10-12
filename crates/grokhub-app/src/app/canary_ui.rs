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
        self.harness.self_review.trial = Some(TrialPick {
            class,
            version: v.version.clone(),
            arm,
        });
        parent.unwrap_or_else(|| sk.clone())
    }

    /// Name the trial side on this run's metrics line.
    pub(super) fn tag_trial(&self, m: &mut RunMetrics) {
        let skill = self
            .harness
            .self_review
            .trial
            .clone()
            .filter(|p| p.class == m.class);
        if let Some(pick) = skill
            .or_else(|| brain_trial_pick(&m.brain))
            .or_else(|| self.note_trial_pick())
        {
            m.version = pick.version;
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
        self.harness.self_review.history = None;
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
    pub(super) fn start_skill_trial(
        &mut self,
        change: &hx::Change,
        skill: &str,
        title: &str,
        before: &str,
        after: &str,
    ) {
        let dir = config::config_dir();
        let class = grokhub_core::outcome::skill_signature(skill);
        let label = format!("{} in {}", title.trim(), skill);
        if let Ok(v) = hx::start_trial(
            &dir,
            change,
            &class,
            &label,
            hx::Metric::Steps,
            before,
            after,
        ) {
            self.harness.self_review.history = None;
            self.post_trial_note(&v);
        }
    }

    /// True when this ledger line is on trial: its note already went out, so
    /// it gets no Work-tree row with Undo and Keep.
    pub(super) fn on_trial(&self, kind: hx::ChangeKind, c: &hx::Change) -> bool {
        hx::read_versions(&config::config_dir())
            .iter()
            .any(|v| v.kind == kind.as_str() && v.id == c.id && v.seq == c.seq)
    }
}

/// Brain 6: an open trial on the run's policy row or playbook. The tweak is
/// in the file every matching run reads, so the run is on its canary side.
fn brain_trial_pick(b: &run_metrics::RunBrain) -> Option<TrialPick> {
    let dir = config::config_dir();
    let classes = [
        (!b.policy.is_empty()).then(|| format!("policy:{}", b.policy)),
        (!b.playbook.is_empty()).then(|| format!("playbook:{}", b.playbook)),
    ];
    classes.into_iter().flatten().find_map(|class| {
        let v = hx::open_trial(&dir, &class)?;
        Some(TrialPick {
            class,
            version: v.version,
            arm: Arm::Canary,
        })
    })
}

/// The trial's parent `SKILL.md`, parsed. `None` when it was not kept.
fn parent_skill(dir: &std::path::Path, v: &hx::Version) -> Option<SkillMd> {
    let ledger = hx::ChangeLedger::load_kind(dir, hx::ChangeKind::Skill);
    let line = ledger
        .all()
        .iter()
        .find(|c| c.seq == v.seq && c.id == v.id)?;
    if line.before_hash.is_empty() {
        return None;
    }
    let bytes = hx::kept_bytes(dir, hx::ChangeKind::Skill, &v.id, &line.before_hash).ok()?;
    Some(grokhub_core::parse_skill_md(&String::from_utf8_lossy(
        &bytes,
    )))
}

/// Chat notes kept at once, in name order.
const NOTES_MAX: usize = 5;
/// The line above the chat notes in the cabin rules.
pub(super) const NOTES_HEAD: &str =
    "Notes from GrokHub's weekly review of its own runs. They shape how you work; the cabin safety and approval rules always win:";

impl Cabin {
    /// A chat-note proposal from the weekly pass: one Suggestion card whose
    /// Apply writes `tunable/<name>.md` and puts it on trial.
    pub(super) fn post_snippet_card(
        &mut self,
        p: &grokhub_core::self_review::RankedProposal,
        now: u64,
    ) {
        let details = format!(
            "{}.\n\nApply adds this note to the instructions of every chat; the cabin safety and approval rules still win:\n\n“{}”\n\nOne chat in five tries it first; GrokHub keeps it only if those runs take fewer steps.",
            p.numbers, p.steps
        );
        let card = super::self_review_ui::ProposalCard {
            source: grokhub_core::self_review::snippet_source(&p.skill),
            skill: &p.skill,
            title: &p.title,
            body: &p.numbers,
            details,
            md: &p.steps,
        };
        self.post_self_review_card(card, now);
    }

    /// Apply on a chat-note card: the guard checks the name and text again,
    /// the ledger keeps the version, and the note goes on trial in every chat.
    pub(super) fn apply_snippet(&mut self, name: &str, title: &str, text: &str) -> bool {
        use grokhub_core::protected_surface::{ProtectedSurface, TuneTarget};
        let dir = config::config_dir();
        let now = grokhub_core::now_ms();
        let checked = ProtectedSurface::check(&format!("snippet:{name}")).and_then(|t| match t {
            TuneTarget::Snippet(n) => ProtectedSurface::check_text(text).map(|text| (n, text)),
            _ => Err("not a chat note".into()),
        });
        let (name, text) = match checked {
            Ok(ok) => ok,
            Err(why) => {
                let _ =
                    hx::append_span(&dir, &hx::guard_span(&format!("snippet:{name}"), &why, now));
                self.status = why;
                return true;
            }
        };
        let target = hx::SnippetTarget {
            config_dir: &dir,
            name: &name,
        };
        let before = target_text(&target);
        let reason = format!("weekly self-review: {}", title.trim());
        let written = hx::record_change(&dir, &target, hx::Origin::SelfManage, &reason, || {
            hx::ChangeTarget::put(&target, Some(format!("{text}\n").as_bytes()))
        });
        match written {
            Ok(Some(change)) => {
                let label = format!("{} (chat note)", title.trim());
                if let Ok(v) = hx::start_trial(
                    &dir,
                    &change,
                    hx::ALL_RUNS,
                    &label,
                    hx::Metric::Steps,
                    &before,
                    &format!("{text}\n"),
                ) {
                    self.harness.self_review.history = None;
                    self.post_trial_note(&v);
                }
                self.status = format!("Chat note {name} added: one chat in five tries it first.");
                true
            }
            Ok(None) => {
                self.status = format!("Chat note {name} already says that.");
                true
            }
            Err(e) => {
                self.status = e;
                false
            }
        }
    }

    /// The chat notes this chat follows, as one block for the cabin rules.
    /// On the control side of an open note trial, the note's version before.
    pub(super) fn tuned_notes(&self) -> String {
        let dir = config::config_dir();
        let notes_dir = dir.join(grokhub_core::protected_surface::TUNABLE_DIR);
        let Ok(entries) = std::fs::read_dir(&notes_dir) else {
            return String::new();
        };
        let mut names: Vec<String> = entries
            .flatten()
            .filter_map(|e| {
                e.file_name()
                    .to_str()?
                    .strip_suffix(".md")
                    .map(str::to_string)
            })
            .collect();
        names.sort();
        let trial = hx::open_trial(&dir, hx::ALL_RUNS);
        let mut lines = Vec::new();
        for name in names.into_iter().take(NOTES_MAX) {
            let target = hx::SnippetTarget {
                config_dir: &dir,
                name: &name,
            };
            let mut text = target_text(&target);
            if let Some(v) = trial.as_ref().filter(|v| v.id == name) {
                if hx::pick_arm(&self.trace_id(), &v.version, v.share_pct) == Arm::Control {
                    text = trial_parent_text(&dir, v).unwrap_or_default();
                }
            }
            // A note edited by hand that the guard would refuse is left out.
            if let Ok(text) = grokhub_core::protected_surface::ProtectedSurface::check_text(&text) {
                lines.push(format!("- {text}"));
            }
        }
        if lines.is_empty() {
            return String::new();
        }
        format!("{NOTES_HEAD}\n{}", lines.join("\n"))
    }

    /// The open note trial's side for this chat, for its runs' metrics.
    pub(super) fn note_trial_pick(&self) -> Option<TrialPick> {
        let v = hx::open_trial(&config::config_dir(), hx::ALL_RUNS)?;
        let arm = hx::pick_arm(&self.trace_id(), &v.version, v.share_pct);
        Some(TrialPick {
            class: hx::ALL_RUNS.into(),
            version: v.version,
            arm,
        })
    }
}

fn target_text(t: &dyn hx::ChangeTarget) -> String {
    t.read()
        .ok()
        .flatten()
        .map(|b| String::from_utf8_lossy(&b).trim().to_string())
        .unwrap_or_default()
}

/// The text a note trial replaced. Empty when the note was new.
fn trial_parent_text(dir: &std::path::Path, v: &hx::Version) -> Option<String> {
    let ledger = hx::ChangeLedger::load_kind(dir, hx::ChangeKind::Snippet);
    let line = ledger
        .all()
        .iter()
        .find(|c| c.seq == v.seq && c.id == v.id)?;
    if line.before_hash.is_empty() {
        return Some(String::new());
    }
    let bytes = hx::kept_bytes(dir, hx::ChangeKind::Snippet, &v.id, &line.before_hash).ok()?;
    Some(String::from_utf8_lossy(&bytes).trim().to_string())
}
