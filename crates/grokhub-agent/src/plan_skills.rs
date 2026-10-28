//! Plan-to-skill wiring for Loadout built-in vstack skills.
//!
//! The agent picks these on its own at the moments the Command Centre card
//! named: ship/release gates, verification before done, and multi-step
//! follow-through. Pure rules — no I/O — so playbooks, close-check, and the
//! prompt can share one table.

/// Built-in skill names this module wires (must match assets under
/// `assets/skills/`).
pub const QA_WALKTHROUGH: &str = "qa-walkthrough";
pub const AV_QA_WALKTHROUGH: &str = "av-qa-walkthrough";
pub const CREATE_VERIFICATION: &str = "create-verification-skill";
pub const MAINTAIN_VERIFICATION: &str = "maintain-verification-skill";
pub const FOLLOW_THROUGH: &str = "follow-through";

/// Moments that should pull a built-in skill into the plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillMoment {
    /// Before a ship or release step. `av_changed` adds the AV walkthrough.
    BeforeShip { av_changed: bool },
    /// Before marking work done — run a verification step.
    BeforeDone,
    /// Any multi-step plan or mission — keep a ledger and check in.
    MultiStepPlan,
}

/// Skills the agent should load for `moment`, in invoke order.
pub fn skills_for(moment: SkillMoment) -> &'static [&'static str] {
    match moment {
        SkillMoment::BeforeShip { av_changed: false } => &[QA_WALKTHROUGH],
        SkillMoment::BeforeShip { av_changed: true } => &[QA_WALKTHROUGH, AV_QA_WALKTHROUGH],
        SkillMoment::BeforeDone => &[CREATE_VERIFICATION, MAINTAIN_VERIFICATION],
        SkillMoment::MultiStepPlan => &[FOLLOW_THROUGH],
    }
}

/// Words that mark a plan step / todo as a ship or release gate.
const SHIP_WORDS: &[&str] = &[
    "ship",
    "release",
    "promote",
    "beta to main",
    "beta→main",
    "tag the release",
    "publish",
    "deploy",
];

/// Words that mark audio / video / capture / streaming work.
const AV_WORDS: &[&str] = &[
    "audio",
    "video",
    "av ",
    " a/v",
    "capture",
    "streaming",
    "stream",
    "codec",
    "ffmpeg",
    "ffprobe",
    "playback",
    "microphone",
    "webcam",
    "screen record",
    "screen-record",
];

/// True when `text` looks like a ship or release step.
pub fn is_ship_or_release(text: &str) -> bool {
    let t = text.to_ascii_lowercase();
    SHIP_WORDS.iter().any(|w| t.contains(w))
}

/// True when `text` (ask, todo, or changed-path summary) touches AV work.
pub fn is_av_change(text: &str) -> bool {
    let t = text.to_ascii_lowercase();
    AV_WORDS.iter().any(|w| t.contains(w))
}

/// True when the plan or mission has more than one pending step.
pub fn is_multi_step(pending_count: usize) -> bool {
    pending_count >= 2
}

/// Context the router / close-check / prompt feed into [`skills_for_context`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlanSkillCtx<'a> {
    /// Joined plan steps, todos, and mission checklist text.
    pub plan_text: &'a str,
    /// Ask, diff summary, or changed-path list for AV detection.
    pub change_text: &'a str,
    /// Pending + in-progress todos / checklist items.
    pub pending_steps: usize,
    /// The agent is about to claim done / EndTurn with open work cleared.
    pub marking_done: bool,
}

/// Every skill moment that fires for `ctx`, then the flat skill list (deduped,
/// invoke order preserved).
pub fn moments_for(ctx: &PlanSkillCtx<'_>) -> Vec<SkillMoment> {
    let mut out = Vec::new();
    if is_ship_or_release(ctx.plan_text) || is_ship_or_release(ctx.change_text) {
        out.push(SkillMoment::BeforeShip {
            av_changed: is_av_change(ctx.change_text) || is_av_change(ctx.plan_text),
        });
    }
    if ctx.marking_done {
        out.push(SkillMoment::BeforeDone);
    }
    if is_multi_step(ctx.pending_steps) {
        out.push(SkillMoment::MultiStepPlan);
    }
    out
}

/// Deduped skill names for every moment that fires on `ctx`.
pub fn skills_for_context(ctx: &PlanSkillCtx<'_>) -> Vec<&'static str> {
    let mut out = Vec::new();
    for moment in moments_for(ctx) {
        for name in skills_for(moment) {
            if !out.contains(name) {
                out.push(*name);
            }
        }
    }
    out
}

/// Prompt block so the agent loads the right built-in without being asked.
/// Empty when nothing fires.
pub fn prompt_block(ctx: &PlanSkillCtx<'_>) -> String {
    let moments = moments_for(ctx);
    if moments.is_empty() {
        return String::new();
    }
    let mut lines = vec![
        "<plan-skills>".to_string(),
        "Load these built-in skills with the skill tool at the named moments:".to_string(),
    ];
    for moment in moments {
        let names = skills_for(moment).join(", ");
        let when = match moment {
            SkillMoment::BeforeShip { av_changed: false } => "before any ship or release step",
            SkillMoment::BeforeShip { av_changed: true } => {
                "before any ship or release step (AV also changed)"
            }
            SkillMoment::BeforeDone => "before marking work done (verification step)",
            SkillMoment::MultiStepPlan => {
                "on this multi-step plan or mission (ledger and check-ins)"
            }
        };
        lines.push(format!("- {when}: {names}"));
    }
    lines.push("</plan-skills>".to_string());
    lines.join("\n")
}

/// Standing reminder always listed with built-in skills (not moment-gated).
pub fn standing_reminder() -> &'static str {
    "<plan-skills>\n\
Built-in Loadout skills (always available via the skill tool or /name):\n\
- Before a ship or release step: qa-walkthrough; also av-qa-walkthrough when audio, video, capture, or streaming changed.\n\
- Before marking work done: run a verification step (create-verification-skill / maintain-verification-skill / the project's verify skill).\n\
- On any multi-step plan or mission: follow-through (ledger, check-ins, stalls, status).\n\
</plan-skills>"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ship_gate_loads_qa_and_av_when_needed() {
        assert_eq!(
            skills_for(SkillMoment::BeforeShip { av_changed: false }),
            &[QA_WALKTHROUGH]
        );
        assert_eq!(
            skills_for(SkillMoment::BeforeShip { av_changed: true }),
            &[QA_WALKTHROUGH, AV_QA_WALKTHROUGH]
        );
        assert!(is_ship_or_release("Ship the beta → main promote"));
        assert!(is_ship_or_release("tag the release v1.2.3"));
        assert!(!is_ship_or_release("read the README"));
        assert!(is_av_change("ffmpeg capture pipeline"));
        assert!(is_av_change("screen record helper"));
        assert!(!is_av_change("rename the settings tile"));
    }

    #[test]
    fn before_done_loads_verification_skills() {
        assert_eq!(
            skills_for(SkillMoment::BeforeDone),
            &[CREATE_VERIFICATION, MAINTAIN_VERIFICATION]
        );
        let ctx = PlanSkillCtx {
            marking_done: true,
            ..Default::default()
        };
        assert_eq!(
            skills_for_context(&ctx),
            vec![CREATE_VERIFICATION, MAINTAIN_VERIFICATION]
        );
        assert!(prompt_block(&ctx).contains("verification step"));
        assert!(prompt_block(&ctx).contains(CREATE_VERIFICATION));
    }

    #[test]
    fn multi_step_plan_loads_follow_through() {
        assert!(is_multi_step(2));
        assert!(!is_multi_step(1));
        assert_eq!(skills_for(SkillMoment::MultiStepPlan), &[FOLLOW_THROUGH]);
        let ctx = PlanSkillCtx {
            pending_steps: 3,
            ..Default::default()
        };
        assert_eq!(skills_for_context(&ctx), vec![FOLLOW_THROUGH]);
        assert!(prompt_block(&ctx).contains(FOLLOW_THROUGH));
        assert!(prompt_block(&ctx).contains("multi-step"));
    }

    #[test]
    fn ship_with_av_and_multi_step_combines_without_dupes() {
        let ctx = PlanSkillCtx {
            plan_text: "1. fix codec\n2. ship the release",
            change_text: "audio playback + streaming reconnect",
            pending_steps: 2,
            marking_done: false,
        };
        let names = skills_for_context(&ctx);
        assert_eq!(
            names,
            vec![QA_WALKTHROUGH, AV_QA_WALKTHROUGH, FOLLOW_THROUGH]
        );
        let block = prompt_block(&ctx);
        assert!(block.contains("AV also changed"), "{block}");
        assert!(block.contains(FOLLOW_THROUGH), "{block}");
        assert!(standing_reminder().contains(QA_WALKTHROUGH));
        assert!(standing_reminder().contains(FOLLOW_THROUGH));
    }

    #[test]
    fn quiet_single_step_emits_nothing() {
        let ctx = PlanSkillCtx {
            plan_text: "rename the button",
            change_text: "ui label",
            pending_steps: 1,
            marking_done: false,
        };
        assert!(moments_for(&ctx).is_empty());
        assert!(skills_for_context(&ctx).is_empty());
        assert!(prompt_block(&ctx).is_empty());
    }
}
