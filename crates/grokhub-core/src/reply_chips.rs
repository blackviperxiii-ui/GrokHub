//! Predicted replies for cards that wait on the person: a plan to approve,
//! the agent's question.
//!
//! A card offers a few likely replies as chips instead of fixed buttons. The
//! plan's replies come from the plan itself (its steps), the question's from
//! its options. Both are ranked by what this person picked before, so the
//! reply they usually give leads the row. Typing a reply always works too.
//! No model call: everything here is local.
//!
//! Safety approvals (Allow once / Always allow / Deny) are not built here and
//! keep their own explicit buttons.

use crate::chips::ChipMemory;
use crate::wire::ElicitChoice;

/// Chips one plan card shows, at most.
pub const PLAN_REPLIES_SHOWN: usize = 6;
/// A step is quoted back this long, at most.
const STEP_CLIP: usize = 120;
/// One pick stops counting up here, so an old habit can still be overtaken.
const PICK_CAP: u32 = 50;

/// The text a "Yes, go" reply sends. The turn reader takes it as approval.
pub const PLAN_GO_TEXT: &str = "Go ahead with the plan.";
/// What "Tell me more" asks.
pub const PLAN_MORE_TEXT: &str = "Tell me more about this plan before I say go.";
/// What "Change this" puts in the box for the person to finish.
pub const PLAN_CHANGE_DRAFT: &str = "Change the plan: ";
/// What "Go, but" puts in the box: approve with a skip or an addition.
pub const PLAN_GO_BUT_DRAFT: &str = "Go ahead, but ";

/// What one reply chip does when tapped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplyAct {
    /// One tap sends this text as the reply.
    Send(String),
    /// Puts this text in the reply box, focused, for the person to finish.
    Prefill(String),
    /// Sets the card aside. Nothing is sent.
    Dismiss,
}

/// One predicted reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplyChip {
    /// What the pick is remembered under (`plan:go`, `ask:yes`).
    pub key: String,
    pub label: String,
    pub act: ReplyAct,
}

/// The steps of a plan: its numbered or bulleted lines, in order, without
/// their markers. A plan written as prose has none.
pub fn plan_steps(plan: &str) -> Vec<String> {
    plan.lines()
        .filter_map(|line| step_text(line.trim()))
        .filter(|s| !s.is_empty())
        .collect()
}

fn step_text(line: &str) -> Option<String> {
    for bullet in ["- ", "* ", "• "] {
        if let Some(rest) = line.strip_prefix(bullet) {
            return Some(rest.trim().to_string());
        }
    }
    let digits = line.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits == 0 || digits > 2 {
        return None;
    }
    let rest = &line[digits..];
    let rest = rest
        .strip_prefix(". ")
        .or_else(|| rest.strip_prefix(") "))
        .or_else(|| rest.strip_prefix(": "))?;
    Some(rest.trim().to_string())
}

fn clip(text: &str, max: usize) -> String {
    let text = text.trim();
    if text.chars().count() <= max {
        return text.to_string();
    }
    let cut: String = text.chars().take(max).collect();
    format!("{}…", cut.trim_end())
}

/// The plan's replies, best first: Yes, go; Tell me more; More on the
/// biggest step; Change this; Go, but…; Not now. Ranked by past picks,
/// ties keep that order. At most [`PLAN_REPLIES_SHOWN`].
pub fn plan_reply_chips(plan: &str, memory: &ChipMemory) -> Vec<ReplyChip> {
    let steps = plan_steps(plan);
    let mut out = vec![
        reply("plan:go", "Yes, go", ReplyAct::Send(PLAN_GO_TEXT.into())),
        reply(
            "plan:more",
            "Tell me more",
            ReplyAct::Send(PLAN_MORE_TEXT.into()),
        ),
    ];
    // The step with the most to it is the one most likely to need a closer look.
    if steps.len() >= 2 {
        let (i, step) = steps
            .iter()
            .enumerate()
            .max_by_key(|(i, s)| (s.chars().count(), std::cmp::Reverse(*i)))
            .map(|(i, s)| (i + 1, s))
            .unwrap_or((1, &steps[0]));
        out.push(reply(
            "plan:step",
            &format!("More on step {i}"),
            ReplyAct::Send(format!(
                "Explain step {i} in more detail: {}",
                clip(step, STEP_CLIP)
            )),
        ));
    }
    out.push(reply(
        "plan:change",
        "Change this",
        ReplyAct::Prefill(PLAN_CHANGE_DRAFT.into()),
    ));
    out.push(reply(
        "plan:go-but",
        "Go, but…",
        ReplyAct::Prefill(PLAN_GO_BUT_DRAFT.into()),
    ));
    out.push(reply("plan:not-now", "Not now", ReplyAct::Dismiss));
    rank(&mut out, memory);
    out.truncate(PLAN_REPLIES_SHOWN);
    out
}

/// The question's options as one-tap replies: the agent's recommended one
/// first, then the ones this person picks most, then the agent's order.
/// Every option stays; the agent chose them.
pub fn question_reply_chips(options: &[ElicitChoice], memory: &ChipMemory) -> Vec<ReplyChip> {
    let mut out: Vec<(bool, ReplyChip)> = options
        .iter()
        .filter(|o| !o.label.trim().is_empty())
        .map(|o| {
            let label = o.label.trim();
            (
                o.recommended,
                reply(
                    &question_key(label),
                    label,
                    ReplyAct::Send(label.to_string()),
                ),
            )
        })
        .collect();
    // Stable: equal keys keep the agent's order.
    out.sort_by_key(|(recommended, c)| (!*recommended, std::cmp::Reverse(picks(memory, &c.key))));
    out.into_iter().map(|(_, c)| c).collect()
}

fn question_key(label: &str) -> String {
    let words: Vec<String> = label
        .split_whitespace()
        .map(|w| w.to_ascii_lowercase())
        .collect();
    format!("ask:{}", words.join(" "))
}

/// Note that `key` was picked, so it ranks higher on the next card.
pub fn remember_reply_pick(memory: &mut ChipMemory, key: &str, now_ms: u64) {
    if key.trim().is_empty() {
        return;
    }
    let n = memory.reply_picks.entry(key.to_string()).or_insert(0);
    *n = (*n + 1).min(PICK_CAP);
    memory.updated_at = now_ms;
}

fn picks(memory: &ChipMemory, key: &str) -> u32 {
    memory.reply_picks.get(key).copied().unwrap_or(0)
}

fn rank(chips: &mut [ReplyChip], memory: &ChipMemory) {
    // `sort_by_key` is stable, so unpicked chips keep their written order.
    chips.sort_by_key(|c| std::cmp::Reverse(picks(memory, &c.key)));
}

fn reply(key: &str, label: &str, act: ReplyAct) -> ReplyChip {
    ReplyChip {
        key: key.to_string(),
        label: label.to_string(),
        act,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(chips: &[ReplyChip]) -> Vec<&str> {
        chips.iter().map(|c| c.label.as_str()).collect()
    }

    fn choice(label: &str, recommended: bool) -> ElicitChoice {
        ElicitChoice {
            label: label.into(),
            description: String::new(),
            recommended,
        }
    }

    #[test]
    fn steps_come_from_numbered_and_bulleted_lines_only() {
        let plan = "Here is the plan.\n1. Back up the config\n2) Swap the driver\n- Reboot\n2026 was a year\nDone.";
        assert_eq!(
            plan_steps(plan),
            vec!["Back up the config", "Swap the driver", "Reboot"]
        );
        assert!(plan_steps("Just turn it off and on again.").is_empty());
    }

    #[test]
    fn a_new_person_sees_go_first_and_more_on_the_biggest_step() {
        let plan = "1. Back up\n2. Update the GPU driver and clear the shader cache\n3. Reboot";
        let chips = plan_reply_chips(plan, &ChipMemory::default());
        assert_eq!(
            labels(&chips),
            vec![
                "Yes, go",
                "Tell me more",
                "More on step 2",
                "Change this",
                "Go, but…",
                "Not now"
            ]
        );
        assert_eq!(chips[0].act, ReplyAct::Send(PLAN_GO_TEXT.into()));
        assert_eq!(
            chips[2].act,
            ReplyAct::Send(
                "Explain step 2 in more detail: Update the GPU driver and clear the shader cache"
                    .into()
            )
        );
        assert_eq!(chips[3].act, ReplyAct::Prefill("Change the plan: ".into()));
    }

    #[test]
    fn a_prose_plan_has_no_step_chip_and_keeps_not_now() {
        let chips = plan_reply_chips("Turn off overlays, then retest.", &ChipMemory::default());
        assert_eq!(
            labels(&chips),
            vec![
                "Yes, go",
                "Tell me more",
                "Change this",
                "Go, but…",
                "Not now"
            ]
        );
        assert_eq!(chips[4].act, ReplyAct::Dismiss);
    }

    #[test]
    fn the_reply_this_person_picks_most_leads() {
        let mut mem = ChipMemory::default();
        for _ in 0..3 {
            remember_reply_pick(&mut mem, "plan:change", 10);
        }
        remember_reply_pick(&mut mem, "plan:not-now", 11);
        let chips = plan_reply_chips("Do the thing.", &mem);
        assert_eq!(
            labels(&chips),
            vec![
                "Change this",
                "Not now",
                "Yes, go",
                "Tell me more",
                "Go, but…"
            ]
        );
        assert_eq!(mem.reply_picks["plan:change"], 3);
        assert_eq!(mem.updated_at, 11);
    }

    #[test]
    fn picks_stop_counting_at_the_cap() {
        let mut mem = ChipMemory::default();
        for _ in 0..80 {
            remember_reply_pick(&mut mem, "plan:go", 1);
        }
        assert_eq!(mem.reply_picks["plan:go"], 50);
        remember_reply_pick(&mut mem, "  ", 2);
        assert_eq!(mem.reply_picks.len(), 1);
    }

    #[test]
    fn question_options_put_recommended_then_habit_then_agent_order() {
        let mut mem = ChipMemory::default();
        remember_reply_pick(&mut mem, "ask:skip it", 1);
        let opts = [
            choice("Keep it", false),
            choice("Skip it", false),
            choice("Ask later", true),
            choice("  ", false),
        ];
        let chips = question_reply_chips(&opts, &mem);
        assert_eq!(labels(&chips), vec!["Ask later", "Skip it", "Keep it"]);
        assert_eq!(chips[1].key, "ask:skip it");
        assert_eq!(chips[1].act, ReplyAct::Send("Skip it".into()));
    }

    #[test]
    fn a_long_step_is_clipped_when_quoted() {
        let long = "word ".repeat(60);
        let plan = format!("1. short\n2. {long}");
        let chips = plan_reply_chips(&plan, &ChipMemory::default());
        let ReplyAct::Send(text) = &chips[2].act else {
            panic!("step chip sends");
        };
        assert!(text.ends_with('…'));
        assert!(text.chars().count() < 160);
    }
}
