//! Goal pin survives compact. Incomplete turns stay open.
//! Fast mode names the chat tab from the current topics.

use serde::{Deserialize, Serialize};

/// Turns a topic can stay unseen before the tab drops it.
pub const GOAL_DROP_AFTER: u32 = 3;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadGoal {
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub topics: Vec<String>,
    #[serde(default)]
    pub unseen: Vec<u32>,
    #[serde(default)]
    pub step: u32,
}

pub fn should_name_thread(scratch: bool, user_turns: usize) -> bool {
    !scratch && user_turns > 0
}

pub fn parse_fast_topics(reply: &str) -> Vec<String> {
    let line = reply
        .lines()
        .map(str::trim)
        .find(|l| l.to_ascii_uppercase().starts_with("GOAL:"));
    let Some(line) = line else {
        return Vec::new();
    };
    let rest = line
        .split_once(':')
        .map(|(_, r)| r)
        .unwrap_or(line)
        .trim();
    if rest.is_empty() || looks_like_refusal(rest) {
        return Vec::new();
    }
    split_topics(rest)
}

fn looks_like_refusal(s: &str) -> bool {
    let t = s.to_ascii_lowercase();
    t.contains("cannot") || t.contains("can't") || t.starts_with("i ") || t.starts_with("sorry")
}

fn split_topics(rest: &str) -> Vec<String> {
    let mut out = Vec::new();
    for chunk in rest.split(|c| c == ',' || c == '/') {
        for part in chunk.split(" and ") {
            let t = part
                .trim()
                .trim_matches('"')
                .trim_matches('\'')
                .to_ascii_lowercase();
            if t.is_empty() || t.len() > 24 {
                continue;
            }
            if !t
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == ' ' || c == '-')
            {
                continue;
            }
            if out.iter().any(|x: &String| x == &t) {
                continue;
            }
            out.push(t);
            if out.len() == 4 {
                return out;
            }
        }
    }
    out
}

pub fn blend_thread_goal(prev: &ThreadGoal, observed: &[String], drop_after: u32) -> ThreadGoal {
    if observed.is_empty() {
        return prev.clone();
    }
    let mut topics = prev.topics.clone();
    let mut unseen = if prev.unseen.len() == prev.topics.len() {
        prev.unseen.clone()
    } else {
        vec![0; prev.topics.len()]
    };
    for u in unseen.iter_mut() {
        *u = u.saturating_add(1);
    }
    for obs in observed {
        let key = obs.trim().to_ascii_lowercase();
        if key.is_empty() {
            continue;
        }
        if let Some(i) = topics.iter().position(|t| t == &key) {
            unseen[i] = 0;
        } else {
            topics.push(key);
            unseen.push(0);
        }
    }
    let mut kept_topics = Vec::new();
    let mut kept_unseen = Vec::new();
    for (topic, unseen) in topics.into_iter().zip(unseen) {
        if unseen >= drop_after {
            continue;
        }
        kept_topics.push(topic);
        kept_unseen.push(unseen);
    }
    ThreadGoal {
        label: kept_topics
            .first()
            .cloned()
            .unwrap_or_default(),
        topics: kept_topics,
        unseen: kept_unseen,
        step: prev.step,
    }
}

/// Follow-up prompts use the origin thread pin, not the visible tab.
pub fn goal_pin_for_job(
    job_thread_id: Option<&str>,
    visible_thread_id: &str,
    visible_pin: &str,
    stored_pins: &[(String, String)],
) -> String {
    let Some(job) = job_thread_id else {
        return visible_pin.to_string();
    };
    if job == visible_thread_id {
        return visible_pin.to_string();
    }
    stored_pins
        .iter()
        .find(|(id, _)| id == job)
        .map(|(_, pin)| pin.clone())
        .unwrap_or_else(|| visible_pin.to_string())
}

/// Completing a background goal must not zero another thread's step.
pub fn goal_step_after_outcome(current: u32, outcome: &str, belongs_to_job: bool) -> u32 {
    if !belongs_to_job {
        return current;
    }
    if outcome == "continue" {
        current
    } else {
        0
    }
}

pub fn thread_goal_prompt(messages: &[(String, String)]) -> String {
    let recent = messages
        .iter()
        .rev()
        .take(8)
        .rev()
        .map(|(role, content)| {
            format!(
                "{role}: {}",
                content.chars().take(400).collect::<String>()
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "Read this chat. Name it in one or two lowercase words.\n\
Reply with one line only:\n\
GOAL: topic\n\
The topic is what the conversation is actually about right now, including adult topics.\n\
No lists, no 'and', no quotes, no extra words.\n\n{recent}"
    )
}

pub fn looks_incomplete(assistant_text: &str) -> bool {
    let t = assistant_text.to_ascii_lowercase();
    if t.trim().is_empty() {
        return true;
    }
    if regexish_open(&t) {
        return true;
    }
    if regexish_done(&t) || done_word(&t) {
        return false;
    }
    false
}

fn regexish_open(t: &str) -> bool {
    [
        "next step",
        "still need",
        "todo",
        "to-do",
        "remaining",
        "i'll continue",
        "continue with",
        "partially",
        "in progress",
        "not done yet",
        "blocked on",
        "let me know if",
        "want me to",
    ]
    .iter()
    .any(|p| t.contains(p))
}

fn regexish_done(t: &str) -> bool {
    [
        "all done",
        "completed successfully",
        "nothing else",
        "task is complete",
        "fully done",
        "goal_complete",
        "you're all set",
        "no further action",
        "shipped",
    ]
    .iter()
    .any(|p| t.contains(p))
}

fn done_word(t: &str) -> bool {
    ["done", "fixed", "complete", "shipped", "resolved", "applied"]
        .iter()
        .any(|w| t.split(|c: char| !c.is_ascii_alphabetic()).any(|x| x == *w))
}

pub fn parse_goal_outcome(text: &str) -> &'static str {
    if text.to_ascii_uppercase().contains("GOAL_COMPLETE") {
        return "complete";
    }
    if text.to_ascii_uppercase().contains("GOAL_BLOCKED:") {
        return "blocked";
    }
    if looks_incomplete(text) {
        "continue"
    } else {
        "complete"
    }
}

/// Keep the last `keep` turns. Re-insert the goal pin as a system line so compact cannot drop it.
pub fn compact_keep_pin(
    messages: &[(String, String)],
    keep: usize,
    pin: Option<&str>,
) -> Vec<(String, String)> {
    let keep = keep.max(1);
    let mut out = if messages.len() > keep {
        messages[messages.len() - keep..].to_vec()
    } else {
        messages.to_vec()
    };
    let Some(pin) = pin.map(str::trim).filter(|s| !s.is_empty()) else {
        return out;
    };
    let marked = format!("GOAL PIN: {pin}");
    if !out.iter().any(|(_, c)| c == &marked || c.starts_with(&format!("{marked}\n"))) {
        out.insert(0, ("system".into(), marked));
    }
    out
}

pub const GOAL_MAX_STEPS: u32 = 6;

pub fn next_goal_prompt(pin: &str, prior: &str, step: u32, max_steps: u32) -> Option<String> {
    if pin.trim().is_empty() {
        return None;
    }
    if step >= max_steps.max(1) {
        return None;
    }
    Some(format!(
        "[Goal step {}/{}]\nTask: {}\nLast progress:\n{}\n\nContinue autonomously. Use HOST_CMD as needed.\nWhen fully finished, say clearly: GOAL_COMPLETE\nIf blocked on the user, say: GOAL_BLOCKED: <reason>",
        step + 1,
        max_steps.max(1),
        pin.trim(),
        prior.chars().take(1500).collect::<String>()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pin_survives_and_outcome() {
        let msgs = (0..12)
            .map(|i| ("user".into(), format!("turn {i}")))
            .collect::<Vec<_>>();
        let out = compact_keep_pin(&msgs, 8, Some("flash the pi"));
        assert_eq!(out.len(), 9);
        assert_eq!(out[0], ("system".into(), "GOAL PIN: flash the pi".into()));
        assert!(out.iter().any(|(_, c)| c == "turn 11"));
        assert!(!out.iter().any(|(_, c)| c == "turn 0"));
        assert_eq!(parse_goal_outcome("GOAL_COMPLETE verify ok"), "complete");
        assert_eq!(parse_goal_outcome("GOAL_BLOCKED: need serial"), "blocked");
        assert_eq!(parse_goal_outcome("next step is flashing"), "continue");
        assert_eq!(parse_goal_outcome("All set."), "complete");
        assert!(!looks_incomplete("All set."));
        assert!(!looks_incomplete("All done. GOAL_COMPLETE"));
        let api = (0..12)
            .map(|i| ("user".into(), format!("fix the api {i}")))
            .collect::<Vec<_>>();
        let pinned = compact_keep_pin(&api, 8, Some("pi"));
        assert_eq!(pinned[0], ("system".into(), "GOAL PIN: pi".into()));
        assert!(looks_incomplete(""));
        assert!(looks_incomplete("I'll continue with the flash"));
        let p = next_goal_prompt("flash the pi", "wrote image", 0, 6).unwrap();
        assert!(p.contains("Goal step 1/6"));
        assert!(next_goal_prompt("flash the pi", "x", 6, 6).is_none());
        assert_eq!(
            goal_pin_for_job(Some("thr-a"), "thr-b", "", &[("thr-a".into(), "flash pi".into())]),
            "flash pi"
        );
        assert_eq!(goal_step_after_outcome(3, "complete", false), 3);
        assert_eq!(goal_step_after_outcome(3, "complete", true), 0);
    }

    #[test]
    fn fast_mode_names_the_tab_and_drops_stale_topics() {
        assert_eq!(parse_fast_topics("GOAL: porn"), vec!["porn".to_string()]);
        assert_eq!(
            parse_fast_topics("GOAL: porn and comics"),
            vec!["porn".to_string(), "comics".to_string()]
        );
        assert_eq!(
            parse_fast_topics("Sure.\nGOAL: Comics, ink"),
            vec!["comics".to_string(), "ink".to_string()]
        );
        assert!(parse_fast_topics("I cannot help with that.").is_empty());
        assert!(
            parse_fast_topics("Sure, I can help with that.").is_empty(),
            "filler without GOAL: is not a tab topic"
        );
        assert_eq!(
            parse_fast_topics("gOaL: comics"),
            vec!["comics".to_string()],
            "GOAL: prefix is case-insensitive"
        );
        assert!(should_name_thread(false, 1));
        assert!(!should_name_thread(true, 4));
        assert!(!should_name_thread(false, 0));
        let empty = ThreadGoal::default();
        let porn = blend_thread_goal(&empty, &["porn".into()], GOAL_DROP_AFTER);
        assert_eq!(porn.label, "porn");
        assert_eq!(porn.topics, vec!["porn".to_string()]);
        let both = blend_thread_goal(&porn, &["comics".into()], GOAL_DROP_AFTER);
        assert_eq!(both.label, "porn");
        assert_eq!(both.topics, vec!["porn".to_string(), "comics".to_string()]);
        let stay = blend_thread_goal(&both, &["comics".into()], GOAL_DROP_AFTER);
        assert_eq!(stay.label, "porn");
        let dropped = blend_thread_goal(&stay, &["comics".into()], GOAL_DROP_AFTER);
        assert_eq!(dropped.label, "comics");
        assert_eq!(dropped.topics, vec!["comics".to_string()]);
        let prompt = thread_goal_prompt(&[
            ("user".into(), "draw porn".into()),
            ("assistant".into(), "here".into()),
        ]);
        assert!(prompt.contains("GOAL:"));
        assert!(prompt.contains("draw porn"));
        assert!(prompt.to_ascii_lowercase().contains("topic"));
    }
}
