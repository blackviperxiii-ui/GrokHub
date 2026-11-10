//! What each local chat model is good at, so the router can pick the best
//! model on disk for each task and the picker can recommend a fast, capable
//! everyday model.
//!
//! Pure. The catalog (`data/local_catalog.toml`) carries a `skills` row per
//! chat model: priors from public results for the model's family, 0 to 100.
//! A score left out (or a row without `skills`) is worked out from the tier,
//! the reasoning badge and the context size, so a new catalog row or a test
//! model still gets sensible numbers. The router lowers them per model and
//! task with learned marks (`grokhub_agent::route::local_task`).

use serde::Deserialize;

use crate::local_setup::LocalModel;

/// A model's scores, 0 to 100. A 0 means "not set": [`resolved`] fills it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(default)]
pub struct Skills {
    /// How capable it is overall.
    pub quality: u8,
    /// Everyday chat and quick answers.
    pub chat: u8,
    /// Writing, reading and fixing code.
    pub code: u8,
    /// Math, logic, planning and design.
    pub reason: u8,
    /// Long-form and creative writing.
    pub write: u8,
    /// Working through a big paste or a long chat.
    pub long: u8,
    /// 18+ chat kept on the device: writing that doesn't refuse.
    pub adult: u8,
}

/// The kind of task a turn is, for picking a model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum TaskKind {
    Chat,
    Code,
    Reason,
    Write,
    Long,
    Adult,
}

impl TaskKind {
    pub const ALL: [TaskKind; 6] = [
        TaskKind::Chat,
        TaskKind::Code,
        TaskKind::Reason,
        TaskKind::Write,
        TaskKind::Long,
        TaskKind::Adult,
    ];

    /// The word the running-model row uses: "Running X for {words}".
    pub fn words(self) -> &'static str {
        match self {
            TaskKind::Chat => "everyday chat",
            TaskKind::Code => "coding",
            TaskKind::Reason => "reasoning and planning",
            TaskKind::Write => "writing",
            TaskKind::Long => "long pastes",
            TaskKind::Adult => "18+ chat",
        }
    }

    /// The key it is saved under (learned marks).
    pub fn key(self) -> &'static str {
        match self {
            TaskKind::Chat => "chat",
            TaskKind::Code => "code",
            TaskKind::Reason => "reason",
            TaskKind::Write => "write",
            TaskKind::Long => "long",
            TaskKind::Adult => "adult",
        }
    }

    pub fn from_key(key: &str) -> Option<TaskKind> {
        TaskKind::ALL.into_iter().find(|k| k.key() == key)
    }
}

/// Overall quality for a model without a catalog score, by its tier.
fn tier_quality(tier: &str) -> u8 {
    match tier {
        "T1" => 40,
        "T2" => 55,
        "T3" => 65,
        "T4" | "T5" => 72,
        _ => 50,
    }
}

/// `m`'s scores with every unset one worked out.
pub fn resolved(m: &LocalModel) -> Skills {
    let s = m.skills;
    let q = if s.quality == 0 {
        tier_quality(m.tier)
    } else {
        s.quality
    };
    let or = |v: u8, d: u8| if v == 0 { d } else { v };
    let reasoning = m.badges.contains(&"reasoning");
    let reason_d = if reasoning { q.saturating_add(5).min(100) } else { q };
    // A model that holds more context reads a long paste better.
    let long_d = match m.ctx_default {
        c if c >= 32_768 => q.saturating_add(5).min(100),
        c if c >= 16_384 => q,
        _ => q.saturating_sub(15),
    };
    let write = or(s.write, q);
    Skills {
        quality: q,
        chat: or(s.chat, q),
        code: or(s.code, q),
        reason: or(s.reason, reason_d),
        write,
        long: or(s.long, long_d),
        adult: or(s.adult, write),
    }
}

/// `m`'s score for `kind`.
pub fn skill(m: &LocalModel, kind: TaskKind) -> u8 {
    let s = resolved(m);
    match kind {
        TaskKind::Chat => s.chat,
        TaskKind::Code => s.code,
        TaskKind::Reason => s.reason,
        TaskKind::Write => s.write,
        TaskKind::Long => s.long,
        TaskKind::Adult => s.adult,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local_setup::{self, model_by_id};

    #[test]
    fn every_chat_model_has_a_score_for_every_kind() {
        for m in local_setup::local_models().iter().filter(|m| m.is_chat()) {
            for k in TaskKind::ALL {
                let v = skill(m, k);
                assert!((1..=100).contains(&v), "{} {k:?} = {v}", m.id);
            }
        }
    }

    #[test]
    fn catalog_scores_win_and_unset_ones_are_worked_out() {
        let qwen = model_by_id("qwen3.6-35b-a3b-heretic-q4_k_m").expect("catalog model");
        assert_eq!(skill(qwen, TaskKind::Code), 85);
        let mut bare = *qwen;
        bare.skills = Skills::default();
        // T4, reasoning badge, 32k context.
        assert_eq!(
            resolved(&bare),
            Skills {
                quality: 72,
                chat: 72,
                code: 72,
                reason: 77,
                write: 72,
                long: 77,
                adult: 72,
            }
        );
    }

    #[test]
    fn a_coder_beats_a_writer_at_code_and_loses_at_writing() {
        let glm = model_by_id("zai-glm4.7-flash-30b-a3b-heretic-q4_k_m").expect("catalog model");
        let hermes = model_by_id("nous-hermes4-14b-abliterated-q4_k_m").expect("catalog model");
        assert!(skill(glm, TaskKind::Code) > skill(hermes, TaskKind::Code));
        assert!(skill(hermes, TaskKind::Adult) > skill(glm, TaskKind::Adult));
    }

    #[test]
    fn kinds_round_trip_through_their_keys() {
        for k in TaskKind::ALL {
            assert_eq!(TaskKind::from_key(k.key()), Some(k));
        }
        assert_eq!(TaskKind::from_key("vision"), None);
    }
}
