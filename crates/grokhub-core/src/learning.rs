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
    } else if is_actionable_need(&lower) {
        "need"
    } else {
        "fact"
    };
    format!("{kind}:{slug}")
}

/// Anticipate and `need:` keys require a real reminder, not polite "if you need".
pub fn is_actionable_need(text: &str) -> bool {
    let t = text.to_ascii_lowercase();
    t.contains("need to ") || t.contains("remind me") || t.contains("remember to")
}

pub fn looks_like_user_pref(fact: &str) -> bool {
    let l = fact.to_ascii_lowercase();
    l.contains("prefer")
        || l.contains("my name")
        || l.contains("i use")
        || l.contains("editor")
        || l.contains("project")
}

fn is_greeting_chitchat(fact: &str) -> bool {
    let mut l = fact.trim().to_ascii_lowercase();
    while l.ends_with(['!', '?', '.', ',']) {
        l.pop();
    }
    let l = l.trim();
    matches!(
        l,
        "hi" | "hey"
            | "hello"
            | "yo"
            | "sup"
            | "hiya"
            | "howdy"
            | "hi there"
            | "hey there"
            | "hello there"
            | "how are you"
            | "how are you doing"
            | "how's it going"
            | "hows it going"
            | "what's up"
            | "whats up"
            | "good morning"
            | "good afternoon"
            | "good evening"
            | "hi how are you"
            | "hey how are you"
            | "hello how are you"
            | "say hi"
            | "say hello"
    ) || l.starts_with("say hi ")
        || l.starts_with("say hello ")
}

/// Greeting chit-chat and in-flight redirects are not durable memory.
pub fn is_durable_fact(fact: &str) -> bool {
    let c = fact.trim();
    if c.is_empty() || c.starts_with("New direction:") {
        return false;
    }
    if looks_like_user_pref(c) {
        return true;
    }
    !is_greeting_chitchat(c)
}

pub fn prune_ephemeral_insights(state: &mut LearningState) -> bool {
    let n = state.insights.len();
    state.insights.retain(|i| is_durable_fact(&i.text));
    state.insights.len() != n
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
        if !is_durable_fact(fact) {
            continue;
        }
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

    #[test]
    fn greeting_chitchat_is_not_a_fact() {
        assert!(!is_durable_fact("hi how are you"));
        assert!(!is_durable_fact("say hi in one sentence"));
        assert!(!is_durable_fact("New direction: firefox --remote-debugging-port=9222\n\n(Previous ask: )"));
        assert!(is_durable_fact("prefer nvim"));
        let mut s = LearningState::default();
        extract_insights(
            &mut s,
            &[
                "hi how are you".into(),
                "prefer helix".into(),
                "say hi in one sentence".into(),
            ],
        );
        assert_eq!(s.insights.len(), 1);
        assert!(s.insights[0].text.contains("helix"));
        assert!(
            !insight_key_for_fact("let me know if you need anything").starts_with("need:"),
            "polite 'if you need' is not an anticipate trigger"
        );
        assert!(
            !insight_key_for_fact("I need coffee").starts_with("need:"),
            "bare 'I need X' is not a scheduled reminder"
        );
        assert!(insight_key_for_fact("need to flash the pi tonight").starts_with("need:"));
        assert!(insight_key_for_fact("remind me to check the board").starts_with("need:"));
        upsert_insight(&mut s, "fact:hi-how-are-you", "hi how are you");
        assert!(prune_ephemeral_insights(&mut s));
        assert_eq!(s.insights.len(), 1);
        assert!(s.insights[0].text.contains("helix"));
        assert!(!prune_ephemeral_insights(&mut s));
    }
}
