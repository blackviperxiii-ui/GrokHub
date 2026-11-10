//! The habit miner: what the user does at the same time of day.
//!
//! Each finished run of a skill votes for the part of the day it ran in and
//! against the other three, so the belief `habit:<skill>/<part>` reads "this
//! share of the skill's runs happen then". A habit sure enough to suggest
//! becomes a quick chip in that part of the day, after the decision engine
//! has weighed it ([`super::hub::habit_chips`]). A tap is a yes to the rule
//! and a hide is a no, which rests it for a month.
//!
//! Ids only: the skill's name and the part of the day. Never what was asked.

use super::belief::Beliefs;
use super::decide::{Area, Proposal};
use super::ladder::{step, Step, Undo};
use super::signal::{Kind, Signal, Subject};
use crate::chips::{ChipKind, QuickChip};
use crate::skill::SkillMd;

/// The prefix of a habit chip's id (`habit-evening-tidy`).
pub const CHIP_PREFIX: &str = "habit-";
/// Habit chips on one row at most.
pub const CHIPS_MAX: usize = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    Morning,
    Afternoon,
    Evening,
    Night,
}

impl Part {
    pub const ALL: [Part; 4] = [Part::Morning, Part::Afternoon, Part::Evening, Part::Night];

    /// Morning from 5, afternoon from 12, evening from 17, night from 22.
    pub fn of_hour(hour: u32) -> Self {
        match hour % 24 {
            5..=11 => Self::Morning,
            12..=16 => Self::Afternoon,
            17..=21 => Self::Evening,
            _ => Self::Night,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Morning => "morning",
            Self::Afternoon => "afternoon",
            Self::Evening => "evening",
            Self::Night => "night",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.as_str() == text)
    }

    /// "in the evening", "at night".
    pub fn when(self) -> &'static str {
        match self {
            Self::Morning => "in the morning",
            Self::Afternoon => "in the afternoon",
            Self::Evening => "in the evening",
            Self::Night => "at night",
        }
    }
}

/// The belief for a skill in a part of the day. `None` for a name that is
/// not an id or is too long.
pub fn subject(skill: &str, part: Part) -> Option<Subject> {
    Subject::new("habit", &format!("{skill}/{}", part.as_str()))
}

/// A skill run finished well in local hour `hour`: one vote for that part of
/// the day and one against each other part.
pub fn skill_run(skill: &str, hour: u32, at_ms: u64) -> Vec<Signal> {
    let then = Part::of_hour(hour);
    Part::ALL
        .into_iter()
        .filter_map(|part| {
            let kind = if part == then {
                Kind::Used
            } else {
                Kind::Ignored
            };
            subject(skill, part).map(|s| Signal::new(at_ms, kind, s))
        })
        .collect()
}

/// One thing the user usually does around now.
#[derive(Debug, Clone, PartialEq)]
pub struct Habit {
    pub skill: String,
    pub part: Part,
    pub confidence: f32,
    pub evidence: f32,
}

impl Habit {
    /// The rule the decision engine counts yes and no answers under.
    pub fn rule(&self) -> Option<Subject> {
        Subject::new(
            "rule",
            &format!("habit/{}/{}", self.skill, self.part.as_str()),
        )
    }

    /// The habit as a proposal to show its chip. Showing a chip is undone by
    /// hiding it, so it is a one-click change.
    pub fn proposal(&self) -> Option<Proposal> {
        Some(Proposal {
            area: Area::Chips,
            rule: self.rule()?,
            confidence: self.confidence,
            evidence: self.evidence,
            undo: Undo::OneClick,
            value: self.confidence,
            hard: false,
        })
    }

    pub fn chip_id(&self) -> String {
        format!("{CHIP_PREFIX}{}-{}", self.part.as_str(), self.skill)
    }
}

/// The habits that hold in `part` at `now_ms`, the surest first: most of the
/// skill's runs happen then, and the belief is sure enough to suggest.
pub fn due(beliefs: &Beliefs, part: Part, now_ms: u64) -> Vec<Habit> {
    let mut out: Vec<Habit> = beliefs
        .iter()
        .filter(|(s, _)| s.area() == "habit")
        .filter_map(|(s, b)| {
            let (_, id) = s.as_str().split_once(':')?;
            let (skill, at) = id.rsplit_once('/')?;
            if Part::parse(at)? != part || !b.leans_yes()? {
                return None;
            }
            let (confidence, evidence) = (b.confidence(now_ms), b.evidence(now_ms));
            (step(confidence, evidence, Undo::OneClick) != Step::Watch).then(|| Habit {
                skill: skill.to_string(),
                part,
                confidence,
                evidence,
            })
        })
        .collect();
    out.sort_by(|a, b| {
        b.confidence
            .total_cmp(&a.confidence)
            .then_with(|| a.skill.cmp(&b.skill))
    });
    out
}

/// The rule behind a habit chip's id, for its tap or hide. `None` for any
/// other chip.
pub fn chip_rule(chip_id: &str) -> Option<Subject> {
    let (part, skill) = chip_id.strip_prefix(CHIP_PREFIX)?.split_once('-')?;
    Habit {
        skill: skill.to_string(),
        part: Part::parse(part)?,
        confidence: 0.0,
        evidence: 0.0,
    }
    .rule()
}

/// The chip for a habit, when the skill is still installed. A tap asks for
/// the skill the way the skill's own chip does.
pub fn chip(habit: &Habit, skills: &[SkillMd], score: f32) -> Option<QuickChip> {
    let skill = skills
        .iter()
        .find(|s| crate::skill::skill_dir_name(&s.name) == habit.skill)?;
    Some(QuickChip {
        id: habit.chip_id(),
        label: crate::chips::chip_label(&skill.name.replace('-', " ")),
        value: crate::skill::skill_use_in_chat_prompt(&skill.slash, &skill.name),
        kind: ChipKind::Chat,
        score,
        hint: format!("You often run this {}", habit.part.when()),
        primary: false,
    })
}
