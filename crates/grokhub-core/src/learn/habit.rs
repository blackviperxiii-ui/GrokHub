//! The habit miner: what the user does at the same time of day.
//!
//! Each finished run of a skill votes for the part of the day it ran in and
//! against the other three, so the belief `habit:<skill>/<part>` reads "this
//! share of the skill's runs happen then". A habit sure enough to suggest
//! becomes a quick chip in that part of the day, after the decision engine
//! has weighed it ([`super::hub::habit_chips`]). A tap is a yes to the rule
//! and a hide is a no, which rests it for a month.
//!
//! A habit also belongs to a setting. A run while a game is up votes under
//! `habit:<skill>/<part>.gaming`, and only those habits are due while a game
//! runs, so what the user does between matches does not show up at work.
//!
//! Ids only: the skill's name, the part of the day and the setting. Never
//! what was asked.

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

/// The end of a habit's id: the part of the day, and `.gaming` for a habit
/// that holds while a game runs.
pub fn tail(part: Part, gaming: bool) -> String {
    let setting = if gaming { ".gaming" } else { "" };
    format!("{}{setting}", part.as_str())
}

/// Reads [`tail`] back.
pub fn parse_tail(text: &str) -> Option<(Part, bool)> {
    match text.split_once('.') {
        Some((part, "gaming")) => Some((Part::parse(part)?, true)),
        Some(_) => None,
        None => Some((Part::parse(text)?, false)),
    }
}

/// "in the evening", "at night while gaming".
pub fn when(part: Part, gaming: bool) -> String {
    let setting = if gaming { " while gaming" } else { "" };
    format!("{}{setting}", part.when())
}

/// The belief for a skill in a part of the day and a setting. `None` for a
/// name that is not an id or is too long.
pub fn subject(skill: &str, part: Part, gaming: bool) -> Option<Subject> {
    Subject::new("habit", &format!("{skill}/{}", tail(part, gaming)))
}

/// A skill run finished well in local hour `hour`: one vote for that part of
/// the day and one against each other part, all in the run's own setting.
pub fn skill_run(skill: &str, hour: u32, gaming: bool, at_ms: u64) -> Vec<Signal> {
    let then = Part::of_hour(hour);
    Part::ALL
        .into_iter()
        .filter_map(|part| {
            let kind = if part == then {
                Kind::Used
            } else {
                Kind::Ignored
            };
            subject(skill, part, gaming).map(|s| Signal::new(at_ms, kind, s))
        })
        .collect()
}

/// One thing the user usually does around now.
#[derive(Debug, Clone, PartialEq)]
pub struct Habit {
    pub skill: String,
    pub part: Part,
    /// It holds while a game runs, and only then.
    pub gaming: bool,
    pub confidence: f32,
    pub evidence: f32,
}

impl Habit {
    /// The rule the decision engine counts yes and no answers under.
    pub fn rule(&self) -> Option<Subject> {
        Subject::new(
            "rule",
            &format!("habit/{}/{}", self.skill, tail(self.part, self.gaming)),
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
            helps: None,
            hurts: None,
            tested: false,
        })
    }

    pub fn chip_id(&self) -> String {
        format!(
            "{CHIP_PREFIX}{}-{}",
            tail(self.part, self.gaming),
            self.skill
        )
    }
}

/// The habits that hold in `part` and this setting at `now_ms`, the surest
/// first: most of the skill's runs happen then, and the belief is sure
/// enough to suggest.
pub fn due(beliefs: &Beliefs, part: Part, gaming: bool, now_ms: u64) -> Vec<Habit> {
    let mut out: Vec<Habit> = beliefs
        .iter()
        .filter(|(s, _)| s.area() == "habit")
        .filter_map(|(s, b)| {
            let (_, id) = s.as_str().split_once(':')?;
            let (skill, at) = id.rsplit_once('/')?;
            if parse_tail(at)? != (part, gaming) || !b.leans_yes()? {
                return None;
            }
            let (confidence, evidence) = (b.confidence(now_ms), b.evidence(now_ms));
            (step(confidence, evidence, Undo::OneClick) != Step::Watch).then(|| Habit {
                skill: skill.to_string(),
                part,
                gaming,
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

/// Habits the app has seen in `part` and this setting but is not sure of:
/// they lean yes with more than one run behind them, and are still under the
/// line for a suggestion. The likeliest first. These are what an experiment
/// may try ([`super::experiment`]).
pub fn unsure(beliefs: &Beliefs, part: Part, gaming: bool, now_ms: u64) -> Vec<Habit> {
    let mut out: Vec<Habit> = beliefs
        .iter()
        .filter(|(s, _)| s.area() == "habit")
        .filter_map(|(s, b)| {
            let (_, id) = s.as_str().split_once(':')?;
            let (skill, at) = id.rsplit_once('/')?;
            if parse_tail(at)? != (part, gaming) || !b.leans_yes()? {
                return None;
            }
            let (confidence, evidence) = (b.confidence(now_ms), b.evidence(now_ms));
            (evidence >= super::experiment::MIN_EVIDENCE
                && step(confidence, evidence, Undo::OneClick) == Step::Watch)
                .then(|| Habit {
                    skill: skill.to_string(),
                    part,
                    gaming,
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

/// The chip for a habit the app is trying on purpose: the habit's own chip,
/// saying that it is a try.
pub fn trial_chip(habit: &Habit, skills: &[SkillMd], score: f32) -> Option<QuickChip> {
    let mut chip = chip(habit, skills, score)?;
    chip.hint = format!(
        "Trying this: you sometimes run it {}",
        when(habit.part, habit.gaming)
    );
    Some(chip)
}

/// The rule behind a habit chip's id, for its tap or hide. `None` for any
/// other chip.
pub fn chip_rule(chip_id: &str) -> Option<Subject> {
    let (at, skill) = chip_id.strip_prefix(CHIP_PREFIX)?.split_once('-')?;
    let (part, gaming) = parse_tail(at)?;
    Habit {
        skill: skill.to_string(),
        part,
        gaming,
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
        hint: format!("You often run this {}", when(habit.part, habit.gaming)),
        primary: false,
    })
}
