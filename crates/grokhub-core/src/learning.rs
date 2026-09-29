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
    /// `total_turns` the last time a model review was spent. Quiet days stay free.
    #[serde(default)]
    pub reviewed_turns: u32,
    /// What each part has seen for itself. The engine is the only reader that
    /// turns these into directives for other parts.
    #[serde(default)]
    pub part_notes: Vec<crate::cabin_engine::PartNote>,
    #[serde(default)]
    pub directives: Vec<crate::cabin_engine::CabinDirective>,
}

/// A paid nightly review is worth it after several new turns. Otherwise the
/// local chip lessons are the whole improvement loop.
pub fn review_worth_tokens(total_turns: u32, reviewed_turns: u32) -> bool {
    total_turns.saturating_sub(reviewed_turns) >= 8
}

/// Keep lessons the chips still support. Drop habit and skip lines whose evidence is gone.
pub fn apply_local_lessons(state: &mut LearningState, lessons: &[crate::chips::LocalLesson]) {
    let keep: Vec<&crate::chips::LocalLesson> = lessons.iter().filter(|l| !l.drop).collect();
    for lesson in &keep {
        if !crate::is_plain_text(&lesson.text) || lesson.key.is_empty() || lesson.text.len() < 8 {
            continue;
        }
        if let Some(found) = state.insights.iter_mut().find(|i| i.key == lesson.key) {
            if found.text != lesson.text {
                found.text = lesson.text.clone();
                found.hits = found.hits.saturating_add(1);
            }
        } else {
            state.insights.push(LearningInsight {
                key: lesson.key.clone(),
                text: lesson.text.clone(),
                hits: 1,
            });
        }
    }
    let kept: Vec<&str> = keep.iter().map(|l| l.key.as_str()).collect();
    state.insights.retain(|i| {
        let managed = i.key.starts_with("habit:") || i.key.starts_with("skip:");
        !managed || kept.iter().any(|k| *k == i.key)
    });
    if state.insights.len() > 40 {
        let overflow = state.insights.len() - 40;
        state.insights.drain(0..overflow);
    }
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

/// Short brief for every surface that should know this person. Habits and
/// skips first. Capped so a turn does not grow with the whole memory.
pub fn serve_brief(state: &LearningState) -> String {
    let mut rows: Vec<&LearningInsight> = state
        .insights
        .iter()
        .filter(|i| lesson_line_ok(&i.text))
        .collect();
    rows.sort_by_key(|i| {
        let pri = if i.key.starts_with("habit:") || i.key.starts_with("skip:") {
            0
        } else if i.key.starts_with("pref:") {
            1
        } else {
            2
        };
        (pri, std::cmp::Reverse(i.hits))
    });
    let mut out = String::new();
    for row in rows.into_iter().take(4) {
        let line = row.text.trim();
        if out.len() + line.len() + 1 > 360 {
            break;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(line);
    }
    out
}

fn lesson_line_ok(text: &str) -> bool {
    let t = text.trim();
    if t.len() < 8 || !crate::is_plain_text(t) {
        return false;
    }
    let lower = t.to_ascii_lowercase();
    !lower.contains("sk-")
        && !lower.contains("password")
        && !lower.contains("api key")
        && !lower.contains("token ")
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

pub fn is_greeting_chitchat(fact: &str) -> bool {
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

    #[test]
    fn serve_brief_is_short_and_a_skip_blocks_that_idea() {
        let mut s = LearningState::default();
        upsert_insight(&mut s, "habit:21:id:think", "Around 21:00 they use Think Harder.");
        upsert_insight(&mut s, "skip:21:night", "Around 21:00 they skip night.");
        upsert_insight(&mut s, "secret", "token sk-abcdefghijklmnopqrstuvwx");
        let brief = serve_brief(&s);
        assert!(brief.contains("Think Harder"));
        assert!(brief.contains("skip night"));
        assert!(!brief.contains("sk-"));
        assert!(brief.len() <= 360);
        assert!(crate::update_feed::setup_blocked_by_lessons(
            "Nightly wrap-up",
            &brief
        ));
        assert!(!crate::update_feed::setup_blocked_by_lessons(
            "Morning reminder",
            &brief
        ));
        assert_eq!(
            crate::update_feed::lesson_rank_delta("Nightly wrap-up", &brief),
            -800
        );
    }
}
