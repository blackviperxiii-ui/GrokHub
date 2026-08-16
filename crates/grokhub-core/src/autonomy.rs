//! Passenger policy. The 0–4 slider drives host, skill, learn, and anticipate.

use crate::host_plan::{HostPlanStep, HostRisk};
use crate::learning::LearningInsight;
use crate::project::host_cmd_leaves_project;
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

fn clamp_level(level: u8) -> u8 {
    match level {
        0 | 1 | 2 | 3 | 4 => level,
        _ => 1,
    }
}

/// YOLO uses the level-3 host rule. `/approve risky` lifts Never to SafeModerate.
pub fn autonomy_policy(level: u8, yolo: bool, approve_risky_only: bool) -> Policy {
    let level = clamp_level(level);
    let mut policy = match level {
        0 => Policy {
            host: HostAuto::Never,
            skill_write: SkillWrite::Never,
            skill_follow: SkillFollow::NamesOnly,
            learn: LearnMode::Off,
            anticipate: false,
        },
        1 => Policy {
            host: HostAuto::Never,
            skill_write: SkillWrite::Stage,
            skill_follow: SkillFollow::Inject,
            learn: LearnMode::Extract,
            anticipate: false,
        },
        2 => Policy {
            host: HostAuto::SafeModerate,
            skill_write: SkillWrite::Auto,
            skill_follow: SkillFollow::InjectAndFollow,
            learn: LearnMode::ExtractAndUserMd,
            anticipate: false,
        },
        3 => Policy {
            host: HostAuto::All,
            skill_write: SkillWrite::Auto,
            skill_follow: SkillFollow::InjectAndFollow,
            learn: LearnMode::Full,
            anticipate: false,
        },
        4 => Policy {
            host: HostAuto::All,
            skill_write: SkillWrite::Auto,
            skill_follow: SkillFollow::InjectAndFollow,
            learn: LearnMode::Full,
            anticipate: true,
        },
        _ => unreachable!("clamp_level"),
    };
    if yolo {
        policy.host = HostAuto::All;
    } else if approve_risky_only && matches!(policy.host, HostAuto::Never) {
        policy.host = HostAuto::SafeModerate;
    }
    policy
}

pub fn host_step_autorun(policy: Policy, risk: HostRisk, outside_project: bool) -> bool {
    match policy.host {
        HostAuto::Never => false,
        HostAuto::SafeModerate => !outside_project && risk != HostRisk::Destructive,
        HostAuto::All => true,
    }
}

pub fn host_plan_autorun(policy: Policy, steps: &[HostPlanStep], project_dir: &str) -> bool {
    !steps.is_empty()
        && steps.iter().all(|s| {
            host_step_autorun(
                policy,
                s.risk,
                host_cmd_leaves_project(&s.cmd, project_dir),
            )
        })
}

pub fn should_anticipate(policy: Policy, running: bool, quiet: bool, daily_blocked: bool) -> bool {
    policy.anticipate && !running && !quiet && !daily_blocked
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
    let text = text.to_ascii_lowercase();
    key.starts_with("need:") || text.contains("need") || text.contains("remind")
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
    fn policy_by_level() {
        let p0 = autonomy_policy(0, false, false);
        assert_eq!(p0.host, HostAuto::Never);
        assert_eq!(p0.skill_write, SkillWrite::Never);
        assert!(!p0.learns());
        assert!(!p0.anticipate);
        assert!(!p0.injects_skill());

        let p1 = autonomy_policy(1, false, false);
        assert_eq!(p1.host, HostAuto::Never);
        assert!(p1.stages_skill());
        assert!(p1.injects_skill());
        assert!(p1.learns());
        assert!(!p1.writes_user_md());

        let p2 = autonomy_policy(2, false, false);
        assert_eq!(p2.host, HostAuto::SafeModerate);
        assert!(p2.auto_writes_skill());
        assert_eq!(p2.skill_follow, SkillFollow::InjectAndFollow);
        assert!(p2.writes_user_md());

        let p3 = autonomy_policy(3, false, false);
        assert_eq!(p3.host, HostAuto::All);
        assert_eq!(p3.learn, LearnMode::Full);
        assert!(!p3.anticipate);

        let p4 = autonomy_policy(4, false, false);
        assert_eq!(p4.host, HostAuto::All);
        assert!(p4.anticipate);

        let unknown = autonomy_policy(9, false, false);
        assert_eq!(unknown.host, HostAuto::Never);
        assert!(unknown.stages_skill());
    }

    #[test]
    fn yolo_uses_level_three_host() {
        let p = autonomy_policy(1, true, false);
        assert_eq!(p.host, HostAuto::All);
        assert!(p.stages_skill());
        assert!(!p.anticipate);
    }

    #[test]
    fn risky_only_lifts_never() {
        let p = autonomy_policy(1, false, true);
        assert_eq!(p.host, HostAuto::SafeModerate);
    }

    #[test]
    fn host_autorun_respects_risk() {
        let p2 = autonomy_policy(2, false, false);
        assert!(host_step_autorun(p2, HostRisk::Safe, false));
        assert!(host_step_autorun(p2, HostRisk::Moderate, false));
        assert!(!host_step_autorun(p2, HostRisk::Destructive, false));
        assert!(!host_step_autorun(p2, HostRisk::Safe, true));
        let p3 = autonomy_policy(3, false, false);
        assert!(host_step_autorun(p3, HostRisk::Destructive, true));
        let p1 = autonomy_policy(1, false, false);
        assert!(!host_step_autorun(p1, HostRisk::Safe, false));
    }

    #[test]
    fn plan_autorun_is_all_or_nothing() {
        let p2 = autonomy_policy(2, false, false);
        let safe = vec![step_from_cmd("ls")];
        assert!(host_plan_autorun(p2, &safe, ""));
        let mixed = vec![step_from_cmd("ls"), step_from_cmd("rm -rf /tmp/x")];
        assert!(!host_plan_autorun(p2, &mixed, ""));
        assert!(!host_plan_autorun(p2, &[], ""));
    }

    #[test]
    fn anticipate_gates() {
        let p4 = autonomy_policy(4, false, false);
        let p3 = autonomy_policy(3, false, false);
        assert!(should_anticipate(p4, false, false, false));
        assert!(!should_anticipate(p3, false, false, false));
        assert!(!should_anticipate(p4, true, false, false));
        assert!(!should_anticipate(p4, false, true, false));
        assert!(!should_anticipate(p4, false, false, true));
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
