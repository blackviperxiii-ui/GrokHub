//! Goal pin survives compact. Incomplete turns stay open.
//! Fast mode names the chat tab from the current topics.

use crate::attach::bound_scan;
use crate::chat_view::{assistant_prose, is_workload_user};
use crate::recipe::{user_asks_desktop_hands, user_asks_gui_help};
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
    for chunk in rest.split([',', '/']) {
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

/// Visible tab step after a continue hop. Background jobs must not bump it.
pub fn visible_goal_step_on_continue(visible: u32, job_step: u32, here: bool) -> u32 {
    if here {
        job_step.saturating_add(1)
    } else {
        visible
    }
}

pub fn thread_goal_prompt(messages: &[(String, String)]) -> String {
    let kept: Vec<&(String, String)> = messages
        .iter()
        .filter(|(role, content)| role != "user" || !is_workload_user(content))
        .collect();
    let recent = kept
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
    let t = assistant_prose(assistant_text).to_ascii_lowercase();
    if t.trim().is_empty() {
        return false;
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
        "want me to",
        "not found",
        "not installed",
        "not running",
        "sudo apt",
        "apt install",
        "please run",
        "you can run",
        "proposed fix",
        "if this fails",
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

/// Phone GET /v1/results must not report `done` when the cabin is blocked.
pub fn hub_dispatch_ok(text: &str) -> bool {
    parse_goal_outcome(text) != "blocked"
}

pub fn parse_goal_outcome(text: &str) -> &'static str {
    let text = bound_scan(text);
    let text = text.as_ref();
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
    let start = compact_keep_start_from(
        messages.iter().map(|(r, c)| (r.as_str(), c.as_str())),
        keep,
    );
    let mut out = messages[start..].to_vec();
    let Some(pin) = pin.map(str::trim).filter(|s| !s.is_empty()) else {
        return out;
    };
    let marked = format!("GOAL PIN: {pin}");
    if !out.iter().any(|(_, c)| c == &marked || c.starts_with(&format!("{marked}\n"))) {
        out.insert(0, ("system".into(), marked));
    }
    out
}

/// Index of the first kept turn. Scan borrowed role/content so /compact
/// can drain the dropped prefix without cloning an 8MB HOST_RESULT.
pub fn compact_keep_start_from<'a, I>(messages: I, keep: usize) -> usize
where
    I: IntoIterator<Item = (&'a str, &'a str)>,
{
    let keep = keep.max(1);
    let items: Vec<(&str, &str)> = messages.into_iter().collect();
    let visible_users: Vec<usize> = items
        .iter()
        .enumerate()
        .filter(|(_, (role, content))| *role == "user" && !is_workload_user(content))
        .map(|(i, _)| i)
        .collect();
    if visible_users.len() > keep {
        visible_users[visible_users.len() - keep]
    } else if visible_users.is_empty() && items.len() > keep {
        items.len() - keep
    } else {
        0
    }
}

pub const GOAL_MAX_STEPS: u32 = 6;
pub const FOLLOWUP_MAX_STEPS: u32 = 4;
pub const FOLLOWUP_PROMPT: &str =
    "FOLLOWUP: Finish the incomplete work from your last reply. Act now with Grok Build tools or computer-use if needed. End with status.";

fn reply_has_work_lines(assistant: &str) -> bool {
    assistant.lines().any(|line| {
        let t = line.trim();
        t.starts_with("HOST_CMD") || t.starts_with("COMPUTER_CMD") || t.starts_with("IMAGINE:")
    })
}

fn user_asked_for_work(user: &str) -> bool {
    let t = user.to_ascii_lowercase();
    [
        "fix",
        "check",
        "implement",
        "take over",
        "take control",
        "run ",
        "install",
        "debug",
        "investigate",
        "patch",
        "build ",
        "add ",
        "wire ",
        "close ",
        "click",
        "tools",
        "mouse",
        "hands",
        "make sure",
    ]
    .iter()
    .any(|p| t.contains(p))
}

fn handed_work_to_user(assistant: &str) -> bool {
    let t = assistant_prose(assistant).to_ascii_lowercase();
    [
        "sudo apt",
        "apt install",
        "not found",
        "not installed",
        "not running",
        "please run",
        "you can run",
        "you should run",
        "proposed fix",
        "run this command",
        "if this fails",
        "command not in path",
    ]
    .iter()
    .any(|p| t.contains(p))
}

/// Pin the live user task when the goal namer has not filled `goal_pin` yet.
pub fn goal_continue_pin(pin: &str, last_user: &str) -> String {
    let pin = pin.trim();
    if !pin.is_empty() {
        return pin.to_string();
    }
    last_user
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !is_auto_continue_prompt(l))
        .unwrap_or("")
        .chars()
        .take(120)
        .collect()
}

pub fn is_auto_continue_prompt(content: &str) -> bool {
    let t = content.trim_start();
    t.starts_with("FOLLOWUP:") || t.starts_with("[Goal step ")
}

pub fn should_auto_continue_goal(
    outcome: &str,
    pin: &str,
    running: bool,
    step: u32,
    max_steps: u32,
) -> bool {
    outcome == "continue" && !pin.trim().is_empty() && !running && step < max_steps.max(1)
}

fn promised_action(assistant: &str) -> bool {
    let t = assistant_prose(assistant).to_ascii_lowercase();
    ["i'll", "i will", "let me ", "next i", "going to", "about to"]
        .iter()
        .any(|p| t.contains(p))
}

fn asked_user_a_question(assistant: &str) -> bool {
    let prose = assistant_prose(assistant);
    let last = prose
        .lines()
        .rev()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    last.ends_with('?') && !promised_action(last)
}

fn polite_closer(assistant: &str) -> bool {
    let t = assistant_prose(assistant).to_ascii_lowercase();
    [
        "let me know if you need",
        "let me know if you want",
        "you're all set",
        "you are all set",
    ]
    .iter()
    .any(|p| t.contains(p))
}

fn desktop_thought_needs_followup(user: &str, assistant: &str) -> bool {
    if reply_has_work_lines(assistant) {
        return false;
    }
    if !user_asks_gui_help(user) && !user_asks_desktop_hands(user) {
        return false;
    }
    let low = assistant.to_ascii_lowercase();
    assistant.contains("<tool_call")
        || low.contains("i'll")
        || low.contains("i will")
        || low.contains("let me ")
        || low.contains("windshield")
}

/// Stream-end check: continue when the reply was cut off, promised work,
/// or handed the next command back to the user instead of running it.
pub fn reply_needs_followup(user: &str, assistant: &str, truncated: bool) -> bool {
    if truncated {
        return true;
    }
    let assistant = bound_scan(assistant);
    let assistant = assistant.as_ref();
    if assistant_prose(assistant).is_empty() {
        return desktop_thought_needs_followup(user, assistant);
    }
    if reply_has_work_lines(assistant) {
        return false;
    }
    if assistant.to_ascii_uppercase().contains("GOAL_COMPLETE") {
        return false;
    }
    let low = assistant.to_ascii_lowercase();
    if regexish_done(&low) {
        return false;
    }
    if !user_asked_for_work(user) {
        return false;
    }
    if handed_work_to_user(assistant) {
        return true;
    }
    if polite_closer(assistant) {
        return false;
    }
    if looks_incomplete(assistant) {
        return true;
    }
    if asked_user_a_question(assistant) {
        return false;
    }
    promised_action(assistant)
}

pub fn next_goal_prompt(pin: &str, prior: &str, step: u32, max_steps: u32) -> Option<String> {
    if pin.trim().is_empty() {
        return None;
    }
    if step >= max_steps.max(1) {
        return None;
    }
    Some(format!(
        "[Goal step {}/{}]\nTask: {}\nLast progress:\n{}\n\nContinue autonomously. Use Grok Build tools and computer-use as needed.\nWhen fully finished, say clearly: GOAL_COMPLETE\nIf blocked on the user, say: GOAL_BLOCKED: <reason>",
        step + 1,
        max_steps.max(1),
        pin.trim(),
        prior.chars().take(1500).collect::<String>()
    ))
}

/// Write the visible goal onto the thread being left (`/new` and tab switch).
pub fn flush_visible_goal(goal: &mut ThreadGoal, step: u32, pin: &str) {
    goal.step = step;
    if !pin.trim().is_empty() {
        goal.label = pin.to_string();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thread_goal_prompt_skips_host_results() {
        let p = thread_goal_prompt(&[
            ("user".into(), "paint the cabin".into()),
            (
                "user".into(),
                "HOST_RESULT (facts only):\n$ ls\nexit 0\n".into(),
            ),
            ("assistant".into(), "done".into()),
        ]);
        assert!(p.contains("paint the cabin"), "{p}");
        assert!(
            !p.contains("HOST_RESULT"),
            "auto-title must not name the thread from a host receipt: {p}"
        );
    }

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
        let mut hosty = Vec::new();
        for i in 0..10 {
            hosty.push(("user".into(), format!("ask {i}")));
            hosty.push(("assistant".into(), format!("ans {i}")));
            hosty.push((
                "user".into(),
                "HOST_RESULT (facts only):\n$ x\nexit 0\n".into(),
            ));
        }
        let kept = compact_keep_pin(&hosty, 8, None);
        assert!(
            kept.iter().any(|(_, c)| c == "ask 2"),
            "last 8 visible asks must survive: {kept:?}"
        );
        assert!(
            !kept.iter().any(|(_, c)| c == "ask 0" || c == "ask 1"),
            "/compact help says turns, not raw host rows: {kept:?}"
        );
        assert!(
            !looks_incomplete(""),
            "empty prose is not a reason to start another turn"
        );
        assert!(looks_incomplete("I'll continue with the flash"));
        assert!(
            !looks_incomplete("THINKING:\nnot found, I'll install ydotool\n\nTools are ready."),
            "thinking must not hide a finished answer"
        );
        assert_eq!(
            parse_goal_outcome("THINKING:\nlet me check PATH\n\n"),
            "complete",
            "thinking-only is not an incomplete job"
        );
        let p = next_goal_prompt("flash the pi", "wrote image", 0, 6).unwrap();
        assert!(p.contains("Goal step 1/6"));
        assert!(next_goal_prompt("flash the pi", "x", 6, 6).is_none());
        assert_eq!(
            goal_pin_for_job(Some("thr-a"), "thr-b", "", &[("thr-a".into(), "flash pi".into())]),
            "flash pi"
        );
        assert_eq!(goal_step_after_outcome(3, "complete", false), 3);
        assert_eq!(goal_step_after_outcome(3, "complete", true), 0);
        assert_eq!(
            visible_goal_step_on_continue(0, 1, false),
            0,
            "a background continue must not bump the visible tab step"
        );
        assert_eq!(visible_goal_step_on_continue(0, 1, true), 2);
        assert!(hub_dispatch_ok("All set. GOAL_COMPLETE"));
        assert!(
            !hub_dispatch_ok("GOAL_BLOCKED: need the serial cable"),
            "a blocked phone task must not complete as done"
        );
        assert!(hub_dispatch_ok("Flashed the card."));
        let mut g = ThreadGoal {
            label: "old".into(),
            step: 0,
            ..ThreadGoal::default()
        };
        flush_visible_goal(&mut g, 3, "flash the pi");
        assert_eq!(g.step, 3, "/new must keep the left tab's goal step");
        assert_eq!(g.label, "flash the pi");
        flush_visible_goal(&mut g, 4, "  ");
        assert_eq!(g.step, 4);
        assert_eq!(g.label, "flash the pi", "empty pin must not wipe the label");
    }

    #[test]
    fn stream_end_followup_is_strict() {
        assert!(reply_needs_followup("what is rust", "Rust is", true));
        assert!(reply_needs_followup(
            "fix the service",
            "I'll inspect journalctl next.",
            false
        ));
        assert!(!reply_needs_followup(
            "fix the service",
            "All done. GOAL_COMPLETE",
            false
        ));
        assert!(!reply_needs_followup(
            "what is rust",
            "Here is the definition of Rust.",
            false
        ));
        assert!(!reply_needs_followup(
            "fix the service",
            "Checking now.\nHOST_CMD: systemctl status foo\n",
            false
        ));
        assert!(
            reply_needs_followup(
                "fix the service",
                "Want me to restart it after the patch?",
                false
            ),
            "asking permission is not finishing the job"
        );
        assert!(!reply_needs_followup(
            "fix the service",
            "Patched. Let me know if you need anything else.",
            false
        ));
        let tools = "\
ydotool: NOT FOUND (command not in PATH)\n\
xdotool: NOT FOUND\n\
ydotool service: NOT RUNNING\n\
Status: Mouse/keyboard control via COMPUTER_CMD is disabled.\n\
Proposed Fix: sudo apt update && sudo apt install -y ydotool xdotool\n\
If this fails, tell me your distro.";
        assert!(
            reply_needs_followup(
                "check to make sure all tools are installed to use this feature",
                tools,
                false
            ),
            "handing apt back to the user is not a finished job"
        );
        assert!(looks_incomplete(tools));
        assert_eq!(parse_goal_outcome(tools), "continue");
        assert_eq!(
            goal_continue_pin("", "check to make sure all tools are installed"),
            "check to make sure all tools are installed"
        );
        assert_eq!(goal_continue_pin("hands", "check tools"), "hands");
        assert!(should_auto_continue_goal("continue", "check tools", false, 0, 6));
        assert!(
            !should_auto_continue_goal("continue", "check tools", true, 0, 6),
            "do not send_chat a goal step while host is already running"
        );
        assert!(!should_auto_continue_goal("complete", "check tools", false, 0, 6));
        assert!(!reply_needs_followup(
            "check to make sure all tools are installed",
            "THINKING:\nydotool is not found, I'll apt install it\n\n",
            false
        ));
        assert!(!reply_needs_followup(
            "check the tools",
            "THINKING:\nnot found\n\nHands are ready. GOAL_COMPLETE",
            false
        ));
        assert!(is_auto_continue_prompt(FOLLOWUP_PROMPT));
        assert_eq!(FOLLOWUP_MAX_STEPS, 4);
        assert!(FOLLOWUP_PROMPT.starts_with("FOLLOWUP:"));
        assert!(
            FOLLOWUP_PROMPT.contains("computer-use"),
            "mouse/tab stalls must still mention computer-use: {FOLLOWUP_PROMPT}"
        );
        assert!(
            reply_needs_followup(
                "use my mouse and select a new tab in firefox",
                "THINKING:\nI'll find Firefox and click New Tab.\n<tool_call>\n",
                false
            ),
            "thinking-only XML tool_call on a mouse ask must continue"
        );
        let goal = next_goal_prompt("open a new tab", "found Firefox", 0, 6).unwrap();
        assert!(
            goal.contains("computer-use"),
            "goal continue must mention computer-use: {goal}"
        );
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

    #[test]
    fn parse_goal_outcome_does_not_uppercase_an_8mb_complete() {
        let src = include_str!("goal.rs");
        let outcome = src
            .split("pub fn parse_goal_outcome(")
            .nth(1)
            .and_then(|s| s.split("pub fn compact_keep_pin(").next())
            .expect("parse_goal_outcome");
        let upper = outcome.find("to_ascii_uppercase").expect("uppercase scan");
        assert!(
            outcome[..upper].contains("TEXT_FILE_CAP")
                || outcome[..upper].contains("bound_scan")
                || outcome[..upper].contains("chip_scan"),
            "goal outcome must not uppercase an 8MB complete: {outcome}"
        );
        let follow = src
            .split("pub fn reply_needs_followup(")
            .nth(1)
            .and_then(|s| s.split("pub fn next_goal_prompt(").next())
            .expect("reply_needs_followup");
        let follow_up = follow.find("to_ascii_uppercase").expect("followup uppercase");
        assert!(
            follow[..follow_up].contains("TEXT_FILE_CAP")
                || follow[..follow_up].contains("bound_scan")
                || follow[..follow_up].contains("chip_scan"),
            "follow-up must not uppercase an 8MB complete: {follow}"
        );
    }

    #[test]
    fn parse_goal_outcome_reads_a_marker_after_a_huge_prefix() {
        let mut huge = "a".repeat(crate::attach::TEXT_FILE_CAP + 8);
        huge.push_str("\nGOAL_COMPLETE\n");
        assert_eq!(parse_goal_outcome(&huge), "complete");
        let mut blocked = "b".repeat(crate::attach::TEXT_FILE_CAP + 8);
        blocked.push_str("\nGOAL_BLOCKED: need serial\n");
        assert_eq!(parse_goal_outcome(&blocked), "blocked");
    }
}
