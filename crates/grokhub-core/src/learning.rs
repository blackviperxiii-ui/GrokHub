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

pub fn insight_key_for_fact(fact: &str) -> String {
    let lower = fact.to_ascii_lowercase();
    let slug: String = lower
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c
            } else {
                '-'
            }
        })
        .collect();
    let slug: String = slug
        .split('-')
        .filter(|w| w.len() >= 2)
        .take(4)
        .collect::<Vec<_>>()
        .join("-");
    let kind = if looks_like_user_pref(&lower) {
        "pref"
    } else if lower.contains("need") || lower.contains("remind") {
        "need"
    } else {
        "fact"
    };
    format!("{kind}:{slug}")
}

pub fn looks_like_user_pref(fact: &str) -> bool {
    let l = fact.to_ascii_lowercase();
    l.contains("prefer")
        || l.contains("my name")
        || l.contains("i use")
        || l.contains("editor")
        || l.contains("project")
}

pub fn user_pref_facts(facts: &[String]) -> Vec<String> {
    facts
        .iter()
        .filter(|f| looks_like_user_pref(f))
        .cloned()
        .collect()
}

pub fn extract_insights(state: &mut LearningState, facts: &[String]) {
    for fact in facts {
        upsert_insight(state, &insight_key_for_fact(fact), fact);
    }
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
        let mut learned = LearningState::default();
        extract_insights(
            &mut learned,
            &[
                "prefer nvim".into(),
                "need to flash the pi tonight".into(),
                "token sk-abcdefghijklmnopqrstuv".into(),
            ],
        );
        assert!(learned.insights.iter().any(|i| i.key.starts_with("pref:")));
        assert!(learned.insights.iter().any(|i| i.key.starts_with("need:")));
        assert!(!insight_pin(&learned).contains("sk-"));
        assert_eq!(
            user_pref_facts(&["prefer nvim".into(), "need wifi".into()]),
            vec!["prefer nvim".to_string()]
        );
    }
}
