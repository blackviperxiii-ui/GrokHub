//! Cabin policy. Always maximum autonomy — host, skill, learn, and anticipate.
//! There is no dial: `Policy::max()` is the only policy the cabin runs.

use crate::host_plan::{HostPlanStep, HostRisk};
use crate::learning::LearningInsight;
use crate::skill::{match_skill, SkillMd};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostAuto {
    Never,
    SafeModerate,
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillWrite {
    Never,
    Stage,
    Auto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillFollow {
    NamesOnly,
    Inject,
    InjectAndFollow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LearnMode {
    Off,
    Extract,
    ExtractAndUserMd,
    Full,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Policy {
    pub host: HostAuto,
    pub skill_write: SkillWrite,
    pub skill_follow: SkillFollow,
    pub learn: LearnMode,
    pub anticipate: bool,
}

impl Policy {
    pub fn max() -> Self {
        Self {
            host: HostAuto::All,
            skill_write: SkillWrite::Auto,
            skill_follow: SkillFollow::InjectAndFollow,
            learn: LearnMode::Full,
            anticipate: true,
        }
    }

    pub fn learns(self) -> bool {
        !matches!(self.learn, LearnMode::Off)
    }

    pub fn writes_user_md(self) -> bool {
        matches!(self.learn, LearnMode::ExtractAndUserMd | LearnMode::Full)
    }

    pub fn injects_skill(self) -> bool {
        !matches!(self.skill_follow, SkillFollow::NamesOnly)
    }

    pub fn auto_writes_skill(self) -> bool {
        matches!(self.skill_write, SkillWrite::Auto)
    }

    pub fn stages_skill(self) -> bool {
        matches!(self.skill_write, SkillWrite::Stage)
    }
}


pub fn host_step_autorun(_policy: Policy, _risk: HostRisk, _outside_project: bool) -> bool {
    true
}

pub fn host_plan_autorun(_policy: Policy, steps: &[HostPlanStep], _project_dir: &str) -> bool {
    !steps.is_empty()
}

/// Anticipate only when the seat is free: not running, not reviewing,
/// no composer draft, and not in quiet hours.
pub fn should_anticipate(running: bool, review_busy: bool, draft_empty: bool, quiet: bool) -> bool {
    !running && !review_busy && draft_empty && !quiet
}

/// Do not bump automation usage or the anticipate cooldown when chat cannot send.
pub fn anticipate_consumes_slot(has_key: bool) -> bool {
    has_key
}

/// After idle reflect, fire a follow-skill prompt when a need matches a skill.
pub fn anticipated_need(
    insights: &[LearningInsight],
    skills: &[SkillMd],
    last_fire_ms: u64,
    now_ms: u64,
    cooldown_ms: u64,
) -> Option<String> {
    if now_ms.saturating_sub(last_fire_ms) < cooldown_ms {
        return None;
    }
    for insight in insights {
        if !looks_like_need(&insight.key, &insight.text) {
            continue;
        }
        if let Some(sk) = match_skill(&insight.text, skills) {
            return Some(format!("Follow skill {}", sk.name));
        }
    }
    None
}

fn looks_like_need(key: &str, text: &str) -> bool {
    let key = key.to_ascii_lowercase();
    key.starts_with("need:") && crate::learning::is_actionable_need(text)
}

const MD_CAP: usize = 800;

fn cap_md(s: &str) -> String {
    s.chars().take(MD_CAP).collect()
}

fn push_block(sys: &mut String, title: &str, body: &str) {
    let body = body.trim();
    if body.is_empty() {
        return;
    }
    if !sys.is_empty() {
        sys.push_str("\n\n");
    }
    if !title.is_empty() {
        sys.push_str(title);
        sys.push('\n');
    }
    sys.push_str(body);
}

/// Pure system prompt so the next turn can act on what was learned.
pub fn cabin_system_prompt(
    soul: &str,
    user_md: &str,
    memory_md: &str,
    skill_pins: &str,
    skill_follow: Option<&str>,
    goal_pin: &str,
    board: &str,
    last_host_tail: &str,
    hands: &str,
    insights: &str,
) -> String {
    let mut sys = String::new();
    push_block(&mut sys, "SOUL.md", soul);
    push_block(&mut sys, "USER.md", &cap_md(user_md));
    push_block(&mut sys, "MEMORY.md", &cap_md(memory_md));
    if let Some(follow) = skill_follow.filter(|s| !s.trim().is_empty()) {
        push_block(&mut sys, "", follow);
    }
    if !skill_pins.trim().is_empty() {
        let title = if skill_follow.is_some() {
            "Skills:"
        } else {
            "Skills (names only):"
        };
        push_block(&mut sys, title, skill_pins);
    }
    if !goal_pin.trim().is_empty() {
        if !sys.is_empty() {
            sys.push_str("\n\n");
        }
        sys.push_str("GOAL PIN: ");
        sys.push_str(goal_pin.trim());
    }
    push_block(&mut sys, "Workboard:", board);
    push_block(&mut sys, "Last HOST_RESULT (tail):", last_host_tail);
    push_block(&mut sys, "", hands);
    push_block(&mut sys, "Learned:", insights);
    sys
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host_plan::step_from_cmd;
    use crate::learning::LearningInsight;
    use crate::skill::SkillMd;

    fn flash() -> SkillMd {
        SkillMd {
            name: "flash-pi".into(),
            description: "write an image".into(),
            slash: "/flash".into(),
            trigger: "need to flash the pi".into(),
            instructions: "dd".into(),
            pitfalls: "boot disk".into(),
            verify: "lsblk".into(),
            runs: 0,
        }
    }

    #[test]
    fn always_max() {
        {
            let p = Policy::max();
            assert_eq!(p.host, HostAuto::All);
            assert!(p.auto_writes_skill());
            assert!(p.injects_skill());
            assert!(p.learns());
            assert!(p.writes_user_md());
            assert!(p.anticipate);
            assert!(host_step_autorun(p, HostRisk::Destructive, true));
        }
        let mixed = vec![step_from_cmd("ls"), step_from_cmd("rm -rf /tmp/x")];
        assert!(host_plan_autorun(Policy::max(), &mixed, "/tmp"));
        assert!(!host_plan_autorun(Policy::max(), &[], ""));
        assert!(should_anticipate(false, false, true, false));
        assert!(!should_anticipate(true, false, true, false));
        assert!(!should_anticipate(false, true, true, false));
        assert!(!should_anticipate(false, false, false, false));
        assert!(!should_anticipate(false, false, true, true));
        assert!(
            !anticipate_consumes_slot(false),
            "no key must not burn the daily automation cap"
        );
        assert!(anticipate_consumes_slot(true));
    }

    #[test]
    fn anticipated_need_matches_and_cools_down() {
        let skills = [flash()];
        let insights = [LearningInsight {
            key: "need:pi".into(),
            text: "need to flash the pi".into(),
            hits: 1,
        }];
        let hit = anticipated_need(&insights, &skills, 0, 10_000, 1_000).unwrap();
        assert_eq!(hit, "Follow skill flash-pi");
        assert!(anticipated_need(&insights, &skills, 9_500, 10_000, 1_000).is_none());
        let prefs = [LearningInsight {
            key: "pref:editor".into(),
            text: "prefer nvim always".into(),
            hits: 1,
        }];
        assert!(anticipated_need(&prefs, &skills, 0, 10_000, 1_000).is_none());
        let polite = [LearningInsight {
            key: "fact:let-me-know".into(),
            text: "let me know if you need anything".into(),
            hits: 1,
        }];
        assert!(
            anticipated_need(&polite, &skills, 0, 10_000, 1_000).is_none(),
            "a polite closer must not start Follow skill on its own"
        );
        let coffee = [LearningInsight {
            key: "need:i-need-coffee".into(),
            text: "I need coffee".into(),
            hits: 1,
        }];
        assert!(
            anticipated_need(&coffee, &skills, 0, 10_000, 1_000).is_none(),
            "casual 'need' is not a scheduled job"
        );
    }

    #[test]
    fn system_prompt_includes_user_and_memory() {
        let sys = cabin_system_prompt(
            "voice",
            "prefer nvim",
            "lives in the cabin",
            "- flash-pi — flash",
            Some("Active skill flash-pi — follow these steps:\n## Steps\ndd"),
            "ship v2",
            "- [todo] board",
            "ok",
            "hands",
            "- prefer nvim",
        );
        assert!(sys.contains("SOUL.md"));
        assert!(sys.contains("USER.md"));
        assert!(sys.contains("prefer nvim"));
        assert!(sys.contains("MEMORY.md"));
        assert!(sys.contains("Active skill flash-pi"));
        assert!(sys.contains("GOAL PIN: ship v2"));
        assert!(sys.contains("Learned:"));
        assert!(sys.contains("hands"));
        let empty = cabin_system_prompt("", "", "", "", None, "", "", "", "", "");
        assert!(empty.is_empty());
    }
}
