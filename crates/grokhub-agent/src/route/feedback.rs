//! Built-in routing feedback: record when Auto's pick was right or wrong and
//! nudge later attention (escalate bias per class). **No LLM** — pure local
//! weights from product signals (correction, repair failure, redo, success
//! after a cheap Stay).

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use grokhub_core::model_registry::store::{models_dir, read_json, write_json};
use grokhub_core::outcome::{OutcomeResult, TaskOutcome, REASON_CORRECTION};

pub const FEEDBACK_FILE: &str = "attention_feedback.json";
pub const FEEDBACK_SCHEMA: u32 = 1;
/// Bias clamp: −2 (prefer stay) … +2 (prefer escalate).
pub const BIAS_MAX: i8 = 2;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClassFeedback {
    /// Learned escalate preference for this class.
    #[serde(default)]
    pub escalate_bias: i8,
    /// Stay/DeEscalate followed by correction / failure / redo.
    #[serde(default)]
    pub under_care: u32,
    /// Escalate followed by clean success on a light ask (over-care signal).
    #[serde(default)]
    pub over_care: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeedbackStore {
    #[serde(default)]
    pub schema: u32,
    #[serde(default)]
    pub classes: BTreeMap<String, ClassFeedback>,
}

impl FeedbackStore {
    pub fn bias(&self, class: &str) -> i8 {
        self.classes
            .get(class)
            .map(|c| c.escalate_bias.clamp(-BIAS_MAX, BIAS_MAX))
            .unwrap_or(0)
    }

    /// Note a bad Stay/DeEscalate (user correction, failure, redo).
    pub fn note_under_care(&mut self, class: &str) {
        if class == "repair:diagnose" || class == "prepare:hard" {
            return;
        }
        let e = self.classes.entry(class.into()).or_default();
        e.under_care = e.under_care.saturating_add(1);
        e.escalate_bias = (e.escalate_bias + 1).clamp(-BIAS_MAX, BIAS_MAX);
    }

    /// Note an Escalate that looked unnecessary (clean success, light ask).
    pub fn note_over_care(&mut self, class: &str) {
        if class == "repair:diagnose" || class == "prepare:hard" {
            return;
        }
        let e = self.classes.entry(class.into()).or_default();
        e.over_care = e.over_care.saturating_add(1);
        e.escalate_bias = (e.escalate_bias - 1).clamp(-BIAS_MAX, BIAS_MAX);
    }
}

pub fn feedback_path(config_dir: &Path) -> std::path::PathBuf {
    models_dir(config_dir).join(FEEDBACK_FILE)
}

pub fn load(config_dir: &Path) -> FeedbackStore {
    let mut s: FeedbackStore = read_json(&feedback_path(config_dir)).unwrap_or_default();
    if s.schema == 0 {
        s.schema = FEEDBACK_SCHEMA;
    }
    s
}

pub fn save(config_dir: &Path, store: &FeedbackStore) {
    let _ = write_json(&feedback_path(config_dir), store);
}

/// Fold recent outcomes into escalate bias. `attn_by_episode` maps episode id →
/// attention rule that fired (`attn:stay`, `attn:escalate:host`, …).
pub fn absorb_outcomes(
    store: &mut FeedbackStore,
    outcomes: &[TaskOutcome],
    attn_by_episode: &BTreeMap<String, String>,
    class_by_episode: &BTreeMap<String, String>,
    now_ms: u64,
) {
    let _ = now_ms;
    for o in outcomes {
        let ep = if o.episode.is_empty() {
            o.task.as_str()
        } else {
            o.episode.as_str()
        };
        let Some(attn) = attn_by_episode.get(ep) else {
            continue;
        };
        let class = class_by_episode
            .get(ep)
            .map(String::as_str)
            .unwrap_or(o.task.as_str());
        let stayed = attn.starts_with("attn:stay") || attn.starts_with("attn:deescalate");
        let escalated = attn.starts_with("attn:escalate");
        let failed = matches!(o.result, OutcomeResult::Failure)
            || o.reasons
                .iter()
                .any(|r| r == REASON_CORRECTION || r == "redo");
        if stayed && failed {
            store.note_under_care(class);
        } else if escalated && matches!(o.result, OutcomeResult::Success) && !failed {
            if o.reasons.iter().any(|r| r == "verify_ok" || r == "done") {
                store.note_over_care(class);
            }
        }
    }
}

/// Hourly/tick path: load outcomes + route records' attention rules, update weights.
pub fn tick(
    config_dir: &Path,
    outcomes: &[TaskOutcome],
    attn_by_episode: &BTreeMap<String, String>,
    class_by_episode: &BTreeMap<String, String>,
    now_ms: u64,
) {
    let mut store = load(config_dir);
    absorb_outcomes(
        &mut store,
        outcomes,
        attn_by_episode,
        class_by_episode,
        now_ms,
    );
    store.schema = FEEDBACK_SCHEMA;
    save(config_dir, &store);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn under_care_raises_bias_and_repair_is_immune() {
        let mut s = FeedbackStore::default();
        s.note_under_care("chat:default");
        s.note_under_care("chat:default");
        assert_eq!(s.bias("chat:default"), 2);
        s.note_under_care("repair:diagnose");
        assert_eq!(s.bias("repair:diagnose"), 0);
        s.note_over_care("chat:default");
        assert_eq!(s.bias("chat:default"), 1);
    }

    #[test]
    fn absorb_outcomes_learns_from_correction_after_stay() {
        let mut s = FeedbackStore::default();
        let mut attn = BTreeMap::new();
        attn.insert("ep1".into(), "attn:stay".into());
        let mut classes = BTreeMap::new();
        classes.insert("ep1".into(), "chat:default".into());
        let o = TaskOutcome {
            episode: "ep1".into(),
            task: "chat:default".into(),
            result: OutcomeResult::Failure,
            reasons: vec![REASON_CORRECTION.into()],
            finished_at: 1,
            ..TaskOutcome::default()
        };
        absorb_outcomes(&mut s, &[o], &attn, &classes, 2);
        assert!(s.bias("chat:default") > 0);
        assert_eq!(s.classes["chat:default"].under_care, 1);
    }
}
