//! Durable insights. Pin into context. Secrets never here.

use serde::{Deserialize, Serialize};

use crate::is_plain_text;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LearningInsight {
    pub key: String,
    pub text: String,
    pub hits: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct LearningState {
    #[serde(default)]
    pub insights: Vec<LearningInsight>,
    #[serde(default)]
    pub total_turns: u32,
    #[serde(default)]
    pub last_reflection_at: u64,
}

pub fn upsert_insight(state: &mut LearningState, key: &str, text: &str) {
    if !is_plain_text(text) {
        return;
    }
    let key: String = key.chars().take(80).collect();
    let text: String = text.chars().take(280).collect();
    if key.is_empty() || text.len() < 8 {
        return;
    }
    if let Some(i) = state.insights.iter_mut().find(|i| i.key == key) {
        i.text = text;
        i.hits = i.hits.saturating_add(1);
        return;
    }
    state.insights.push(LearningInsight {
        key,
        text,
        hits: 1,
    });
    if state.insights.len() > 40 {
        state.insights.remove(0);
    }
}

pub fn insight_pin(state: &LearningState) -> String {
    state
        .insights
        .iter()
        .take(12)
        .map(|i| format!("- {}", i.text))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn record_turn(state: &mut LearningState) {
    state.total_turns = state.total_turns.saturating_add(1);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upsert_and_redact() {
        let mut s = LearningState::default();
        upsert_insight(&mut s, "pref:editor", "prefer nvim");
        upsert_insight(&mut s, "pref:editor", "prefer helix");
        assert_eq!(s.insights.len(), 1);
        assert!(s.insights[0].text.contains("helix"));
        upsert_insight(&mut s, "secret", "token sk-abcdefghijklmnopqrstuv");
        assert_eq!(s.insights.len(), 1);
        assert!(insight_pin(&s).contains("helix"));
    }
}
