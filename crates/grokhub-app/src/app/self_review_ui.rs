//! Spike-7: the cabin side of self-improvement. One outcome record per
//! finished task (turn end, a correction, an undo), the weekly self-review on
//! the review slot, and the Pulse Suggestion cards it posts. Accepting a card
//! is a click; the change goes through the ChangeLedger (`origin:
//! self_manage`) after the replay gate, and a revert goes through
//! `undo_skill_change` with a click `UndoAsk`. Nothing here runs a step.

use std::sync::mpsc;

use grokhub_agent::harness as hx;
use grokhub_core::outcome::{self as oc, TaskOutcome};
use grokhub_core::self_review::{self as sr, CardTarget};
use serde::{Deserialize, Serialize};

use super::Cabin;
use crate::{config, skills};

/// Weekly pass state in the cabin config dir.
pub(super) const SELF_REVIEW_FILE: &str = "self_review.json";

/// One card waiting on the user: the `SKILL.md` Apply writes (empty for a revert).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct PendingProposal {
    pub source: String,
    pub skill: String,
    #[serde(default)]
    pub md: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct SelfReviewFile {
    #[serde(default)]
    pub last_day: Option<String>,
    #[serde(default)]
    pub pending: Vec<PendingProposal>,
}

/// What one card says and what its Apply writes.
struct ProposalCard<'a> {
    source: String,
    skill: &'a str,
    title: &'a str,
    body: &'a str,
    details: String,
    md: &'a str,
}

#[derive(Debug, Default)]
pub(super) struct SelfReviewState {
    /// The newest outcome in this cabin, so a correction can supersede it.
    pub last_outcome: Option<TaskOutcome>,
    /// The weekly model call on its way.
    pub rx: Option<mpsc::Receiver<Result<hx::WeeklyPass, String>>>,
    /// Cards this pass may still post (the 5-card budget).
    pub budget: usize,
}

fn state_path() -> std::path::PathBuf {
    config::config_dir().join(SELF_REVIEW_FILE)
}

pub(super) fn load_state() -> SelfReviewFile {
    std::fs::read_to_string(state_path())
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn save_state(s: &SelfReviewFile) {
    if let Ok(text) = serde_json::to_string_pretty(s) {
        let _ = std::fs::write(state_path(), text);
    }
}

/// A draft skill from the newest run's steps (tool names only, no args).
fn draft_md(cand: &sr::DraftCandidate, spans: &[hx::Span]) -> String {
    let mut steps: Vec<String> = Vec::new();
    for s in spans
        .iter()
        .filter(|s| matches!(hx::span_kind(s), hx::SpanKind::Act | hx::SpanKind::Step))
    {
        if steps.last() != Some(&s.tool) {
            steps.push(s.tool.clone());
        }
    }
    let steps: Vec<String> = steps
        .iter()
        .enumerate()
        .map(|(i, t)| format!("{}. {t}", i + 1))
        .collect();
    grokhub_core::render_skill_md(&grokhub_core::SkillMd {
        name: cand.name.clone(),
        description: format!(
            "Done the same way {} times in two weeks: {}",
            cand.runs, cand.topic
        ),
        trigger: cand.topic.clone(),
        instructions: steps.join("\n"),
        ..Default::default()
    })
}

impl Cabin {
    /// The task this turn ran: its skill, else the topic of the user's ask.
    fn task_signature(&self) -> String {
        if !self.skill_name.trim().is_empty() {
            return oc::skill_signature(&self.skill_name);
        }
        let ask = self
            .messages
            .iter()
            .rev()
            .find(|m| m.0 == "user")
            .map(|m| m.1.as_str())
            .unwrap_or("");
        oc::topic_signature(ask)
    }

    /// One outcome line for a turn that ran steps. `audit` is this turn's
    /// audit; `paused` means the retry ladder ran out.
    pub(super) fn record_turn_outcome(
        &mut self,
        trace: &str,
        turn: u32,
        audit: Option<&hx::Audit>,
        paused: bool,
    ) {
        let signature = self.task_signature();
        if signature.is_empty() {
            return;
        }
        let dir = config::config_dir();
        let (spans, _) = hx::read_spans_tail(&dir, trace, 4_000);
        let spans: Vec<hx::Span> = spans.into_iter().filter(|s| s.turn == turn).collect();
        let mut findings: Vec<String> = Vec::new();
        for w in audit.map(|a| a.flagged.as_slice()).unwrap_or_default() {
            if w.window.turn != turn {
                continue;
            }
            for f in &w.findings {
                if !findings.contains(&f.detector) {
                    findings.push(f.detector.clone());
                }
            }
        }
        let task = format!("{trace}:{turn}");
        let outcome = hx::outcome_from_spans(
            &task,
            &signature,
            &spans,
            &findings,
            paused,
            grokhub_core::now_ms(),
        );
        let _ = oc::append_outcome(&dir, &outcome);
        self.harness.self_review.last_outcome = Some(outcome);
    }

    /// The user's next typed line reads as "that was wrong": the last task
    /// in this chat becomes a failure (a superseding line).
    pub(super) fn outcome_user_said(&mut self, text: &str) {
        if !oc::is_correction(text) {
            return;
        }
        let trace = self.trace_id();
        let Some(last) = self.harness.self_review.last_outcome.take() else {
            return;
        };
        if !last.task.starts_with(&format!("{trace}:")) {
            self.harness.self_review.last_outcome = Some(last);
            return;
        }
        let late = last.superseded(oc::REASON_CORRECTION, grokhub_core::now_ms());
        let _ = oc::append_outcome(&config::config_dir(), &late);
        self.harness.self_review.last_outcome = Some(late);
    }

    /// A skill change was undone: its tasks from the last 24 hours fail.
    pub(super) fn outcomes_after_undo(&mut self, skill: &str) {
        let dir = config::config_dir();
        let now = grokhub_core::now_ms();
        let sig = oc::skill_signature(&grokhub_core::skill_dir_name(skill));
        for late in oc::undo_supersedes(&oc::read_outcomes(&dir), &sig, now) {
            let _ = oc::append_outcome(&dir, &late);
        }
    }

    /// Sunday night on the review slot: post revert and draft cards, then
    /// ask the model for skill changes off the UI thread.
    pub(super) fn tick_self_review(&mut self) {
        let mut state = load_state();
        let today = Self::local_day();
        if self.harness.self_review.rx.is_some()
            || !sr::self_review_due(
                state.last_day.as_deref(),
                &today,
                &Self::local_clock(),
                grokhub_core::REVIEW_NIGHT_HOUR,
            )
        {
            return;
        }
        state.last_day = Some(today);
        save_state(&state);
        self.run_self_review_now(grokhub_core::now_ms());
    }

    /// The weekly pass as of `now` (the tick, or a test).
    pub(super) fn run_self_review_now(&mut self, now: u64) {
        let dir = config::config_dir();
        let outcomes = oc::read_outcomes(&dir);
        self.harness.self_review.budget = sr::SELF_REVIEW_CAP;
        for r in sr::revert_candidates(&outcomes, &hx::patch_marks(&dir), now) {
            let details = format!(
                "{}.\n\nRevert puts back v{} of the skill. /skills changes lists every version.",
                r.numbers(),
                r.version
            );
            let (title, body) = (r.title(), r.numbers());
            let card = ProposalCard {
                source: sr::revert_source(&r.skill),
                skill: &r.skill,
                title: &title,
                body: &body,
                details,
                md: "",
            };
            self.post_self_review_card(card, now);
        }
        let have: Vec<String> = self
            .skill_list
            .iter()
            .map(|s| grokhub_core::skill_dir_name(&s.name))
            .collect();
        let have: Vec<&str> = have.iter().map(String::as_str).collect();
        for d in sr::draft_candidates(&outcomes, now, &have) {
            let spans = hx::spans_for(&dir, &d.span_ids);
            let md = draft_md(&d, &spans);
            let numbers = format!("You did this {} times in the last two weeks", d.runs);
            let details = format!(
                "{numbers}. Apply saves it as a new skill you can undo.\n\n{}",
                sr::line_diff("", &md)
            );
            let title = format!("Make a skill: {}", d.name);
            let card = ProposalCard {
                source: sr::draft_source(&d.name),
                skill: &d.name,
                title: &title,
                body: &numbers,
                details,
                md: &md,
            };
            self.post_self_review_card(card, now);
        }
        self.post_router_review(now);
        let stats = sr::skill_stats(&outcomes, now, sr::WEEK_MS);
        let client = if self.harness.self_review.budget > 0 && stats.iter().any(|s| s.failures > 0)
        {
            self.self_review_client().ok()
        } else {
            None
        };
        let Some(client) = client else {
            // No model call follows, so the cards posted so far are saved now.
            if self.harness.self_review.budget < sr::SELF_REVIEW_CAP {
                self.persist_updates();
            }
            return;
        };
        let model = grokhub_core::model_for_mode("balanced").to_string();
        let (tx, rx) = mpsc::channel();
        self.harness.self_review.rx = Some(rx);
        std::thread::spawn(move || {
            let pass = hx::run_weekly(
                &dir,
                client.as_ref(),
                &model,
                &stats,
                now,
                &grokhub_agent::CancelToken::new(),
            )
            .map_err(|e| format!("{e:?}"));
            let _ = tx.send(pass);
        });
    }

    pub(super) fn poll_self_review(&mut self) {
        let Some(rx) = self.harness.self_review.rx.take() else {
            return;
        };
        let pass = match rx.try_recv() {
            Ok(Ok(pass)) => pass,
            Ok(Err(e)) => {
                self.status = format!("Weekly self-review held: {e}");
                self.persist_updates();
                return;
            }
            Err(mpsc::TryRecvError::Empty) => {
                self.harness.self_review.rx = Some(rx);
                return;
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.persist_updates();
                return;
            }
        };
        let now = grokhub_core::now_ms();
        for p in pass.kept {
            let Some(existing) = self
                .skill_list
                .iter()
                .find(|s| grokhub_core::skill_dir_name(&s.name) == p.skill)
                .cloned()
            else {
                continue;
            };
            let mut patched = existing.clone();
            patched.instructions = sr::split_steps(&p.steps);
            if existing.instructions.ends_with('\n') {
                patched.instructions.push('\n');
            }
            let (old, new) = (
                grokhub_core::render_skill_md(&existing),
                grokhub_core::render_skill_md(&patched),
            );
            let stats = format!("{}.", p.numbers);
            let details = format!(
                "{stats} Apply changes the skill's steps; you can undo it.\n\n{}",
                sr::diff_excerpt(&old, &new, 2)
            );
            let card = ProposalCard {
                source: sr::patch_source(&p.skill),
                skill: &p.skill,
                title: &p.title,
                body: &p.numbers,
                details,
                md: &new,
            };
            self.post_self_review_card(card, now);
        }
        self.persist_updates();
    }

    /// Router R3a in the weekly pass: at most two cost-raising tuning cards
    /// (inside the five) and one Home line on what the router changed.
    fn post_router_review(&mut self, now: u64) {
        let dir = config::config_dir();
        let mut posted = 0;
        for (source, title, body) in grokhub_agent::route::learn::cards_due(&dir, now) {
            let before = self.harness.self_review.budget;
            let candidate = match sr::card_target(&source) {
                Some(CardTarget::Router(id)) => id,
                _ => continue,
            };
            let details = format!("{body}\n\nAccept applies it through the change list, and Undo puts it back. Nothing changes without your click.");
            let card = ProposalCard { source, skill: &candidate, title: &title, body: &body, details, md: "" };
            self.post_self_review_card(card, now);
            posted += usize::from(self.harness.self_review.budget < before);
        }
        grokhub_agent::route::learn::cards_posted(&dir, posted, now);
        let line = grokhub_agent::route::learn::review_line(&dir, now);
        let card = grokhub_core::router_update_card("router-review", "Weekly router review", &line, now);
        self.post_feed_card(card);
    }

    /// Post one Suggestion card inside the pass's budget and remember what
    /// Apply does. A live card for the same source is left alone. The caller
    /// saves the feed once: one save thread per card could land out of order.
    fn post_self_review_card(&mut self, card: ProposalCard<'_>, now: u64) {
        let ProposalCard {
            source,
            skill,
            title,
            body,
            details,
            md,
        } = card;
        if self.harness.self_review.budget == 0
            || self
                .updates
                .iter()
                .any(|c| c.source_id == source && c.status != grokhub_core::UpdateStatus::Dismissed)
        {
            return;
        }
        self.harness.self_review.budget -= 1;
        let mut card = grokhub_core::suggestion_card(&source, title, body, now);
        card.details = Some(details);
        grokhub_core::post_update(&mut self.updates, card);
        let mut state = load_state();
        state.pending.retain(|p| p.source != source);
        state.pending.push(PendingProposal {
            source,
            skill: skill.to_string(),
            md: md.to_string(),
        });
        save_state(&state);
    }

    /// The card's Apply (or Revert) item, clicked on the Pulse row menu.
    /// Returns false when the card is not a self-review card.
    pub(super) fn apply_self_review_card(&mut self, id: &str) -> bool {
        let Some(card) = self.updates.iter().find(|c| c.id == id).cloned() else {
            return false;
        };
        let Some(target) = sr::card_target(&card.source_id) else {
            return false;
        };
        let mut state = load_state();
        let Some(pending) = state
            .pending
            .iter()
            .find(|p| p.source == card.source_id)
            .cloned()
        else {
            return false;
        };
        let (dir, skills_dir) = (config::config_dir(), skills::skills_dir());
        let done = match target {
            CardTarget::Router(candidate) => match grokhub_agent::route::learn::accept_card(&dir, &candidate, grokhub_core::now_ms()) {
                Ok(msg) => {
                    self.status = msg;
                    true
                }
                Err(e) => {
                    self.status = e;
                    false
                }
            },
            CardTarget::Revert(name) => {
                let res =
                    hx::undo_skill_change(&dir, &skills_dir, &name, hx::UndoAsk::from_click());
                let ok = res.is_ok();
                self.finish_skill_revert(&name, res);
                ok
            }
            CardTarget::Patch(name) | CardTarget::Draft(name) => {
                let old = self
                    .skill_list
                    .iter()
                    .find(|s| grokhub_core::skill_dir_name(&s.name) == name)
                    .map(grokhub_core::render_skill_md)
                    .unwrap_or_default();
                let gated = if old.is_empty() {
                    Ok(())
                } else {
                    hx::replay_gate(&dir, &name, &old, &pending.md, grokhub_core::now_ms())
                };
                match gated {
                    Err(note) => {
                        if let Some(c) = self.updates.iter_mut().find(|c| c.id == id) {
                            c.details = Some(format!(
                                "{note}\n\n{}",
                                c.details.clone().unwrap_or_default()
                            ));
                        }
                        self.persist_updates();
                        self.status = note;
                        return true;
                    }
                    Ok(()) => {
                        let skill = grokhub_core::parse_skill_md(&pending.md);
                        let reason = format!("weekly self-review: {}", card.title);
                        match skills::save_skill_logged(&skill, hx::Origin::SelfManage, &reason) {
                            Ok(()) => {
                                self.remember_skill(skill);
                                self.harness.skill_rows = None;
                                self.status =
                                    format!("{name} changed. /skills changes can undo it.");
                                true
                            }
                            Err(e) => {
                                self.status = e;
                                false
                            }
                        }
                    }
                }
            }
        };
        if done {
            state.pending.retain(|p| p.source != card.source_id);
            save_state(&state);
            grokhub_core::dismiss_update_at(&mut self.updates, id, grokhub_core::now_ms());
            self.persist_updates();
        }
        true
    }
}

// Test hooks sit last: the UndoAsk source scan reads up to the first `#[cfg(test)]`.
impl Cabin {
    #[cfg(not(test))]
    fn self_review_client(&mut self) -> Result<Box<dyn grokhub_agent::ModelClient + Send>, String> {
        let (bearer, kind) = self.native_cred()?;
        Ok(Box::new(grokhub_agent::XaiClient::new(
            bearer,
            kind,
            std::time::Duration::from_secs(120),
        )))
    }

    #[cfg(test)]
    fn self_review_client(&mut self) -> Result<Box<dyn grokhub_agent::ModelClient + Send>, String> {
        FAKE.with(|f| f.borrow_mut().take())
            .ok_or_else(|| "no fake".to_string())
    }
}

#[cfg(test)]
thread_local! {
    static FAKE: std::cell::RefCell<Option<Box<dyn grokhub_agent::ModelClient + Send>>> = const { std::cell::RefCell::new(None) };
}

/// Tests hand the weekly pass a fake model client.
#[cfg(test)]
pub(super) fn set_self_review_fake(client: Box<dyn grokhub_agent::ModelClient + Send>) {
    FAKE.with(|f| *f.borrow_mut() = Some(client));
}
