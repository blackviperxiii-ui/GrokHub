//! The first-run questions: one tap each, asked only when the answer would
//! change something, and never when the PC already said.
//!
//! "Make things bigger" is not here: it is the big button on the first
//! screen, before any question.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::device::{DeviceProfile, Tier};

/// A question.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Qid {
    Talk,
    Approvals,
    Games,
    Coding,
    Notifications,
    Sleep,
    Background,
    LocalAi,
}

/// Answers by question: an option's `value`.
pub type Answers = BTreeMap<Qid, String>;

/// One tap-to-answer option.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Choice {
    pub label: &'static str,
    pub value: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub struct Question {
    pub id: Qid,
    pub prompt: &'static str,
    pub choices: &'static [Choice],
}

const fn c(label: &'static str, value: &'static str) -> Choice {
    Choice { label, value }
}

/// In the order they are asked: what changes the most first.
const BANK: &[Question] = &[
    Question {
        id: Qid::Talk,
        prompt: "How should I talk to you?",
        choices: &[
            c("Friendly", "friendly"),
            c("Straight to the point", "direct"),
            c("Playful", "playful"),
        ],
    },
    Question {
        id: Qid::Approvals,
        prompt: "Before I do something on your PC, should I ask?",
        choices: &[
            c("Ask me first", "ask"),
            c("Only for risky things", "risky"),
        ],
    },
    Question {
        id: Qid::Games,
        prompt: "Do you play games on this PC?",
        choices: &[c("Yes", "yes"), c("No", "no")],
    },
    Question {
        id: Qid::Notifications,
        prompt: "How many notifications do you want?",
        choices: &[
            c("All of them", "all"),
            c("Only important ones", "important"),
            c("As few as possible", "quiet"),
        ],
    },
    Question {
        id: Qid::Sleep,
        prompt: "When do you usually go to bed?",
        choices: &[
            c("Around 9 PM", "21"),
            c("Around 11 PM", "23"),
            c("After midnight", "1"),
        ],
    },
    Question {
        id: Qid::Coding,
        prompt: "Do you write code on this PC?",
        choices: &[c("Yes", "yes"), c("No", "no")],
    },
    Question {
        id: Qid::Background,
        prompt: "When you close the window, should I keep running?",
        choices: &[c("Yes, stay ready", "yes"), c("No, quit", "no")],
    },
    Question {
        id: Qid::LocalAi,
        prompt: "This PC can run an AI model by itself, even offline. Set that up?",
        choices: &[c("Yes", "yes"), c("Not now", "no")],
    },
];

pub fn bank() -> &'static [Question] {
    BANK
}

pub fn question(id: Qid) -> &'static Question {
    BANK.iter().find(|q| q.id == id).unwrap_or(&BANK[0])
}

/// How many questions someone gets.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Pace {
    /// Picked "Just set it up", skipped, or answers in a flash: 3 at most.
    Quick,
    /// Reads and answers: 8 at most.
    #[default]
    Detailed,
}

impl Pace {
    pub fn limit(self) -> usize {
        match self {
            Pace::Quick => 3,
            Pace::Detailed => 8,
        }
    }

    /// From the first answer: under 2.5 s, or a skip, reads as quick.
    pub fn from_first(answer_ms: u64, skipped: bool) -> Pace {
        if skipped || answer_ms < 2_500 {
            Pace::Quick
        } else {
            Pace::Detailed
        }
    }
}

/// What the PC already answers, so the question is never asked.
pub fn answered_by_device(id: Qid, d: &DeviceProfile) -> Option<&'static str> {
    match id {
        Qid::Games if d.steam => Some("yes"),
        Qid::Coding if d.dev_folders => Some("yes"),
        _ => None,
    }
}

/// A question this PC can't use (no local AI offer on a PC that can't run it).
pub fn fits_device(id: Qid, d: &DeviceProfile) -> bool {
    match id {
        Qid::LocalAi => d.tier() >= Tier::T2 && d.free_disk_mb >= 20_000,
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::super::device::tests::{beast, surface_pro_7};
    use super::*;

    #[test]
    fn every_question_has_two_or_three_choices_and_one_place() {
        for q in bank() {
            assert!((2..=3).contains(&q.choices.len()), "{:?}", q.id);
            assert_eq!(bank().iter().filter(|o| o.id == q.id).count(), 1);
            assert!(q.prompt.ends_with('?'));
        }
        assert_eq!(bank().len(), 8);
    }

    #[test]
    fn pace_comes_from_the_first_answer() {
        assert_eq!(Pace::from_first(1_200, false), Pace::Quick);
        assert_eq!(Pace::from_first(6_000, true), Pace::Quick);
        assert_eq!(Pace::from_first(6_000, false), Pace::Detailed);
        assert_eq!(Pace::Quick.limit(), 3);
        assert_eq!(Pace::Detailed.limit(), 8);
    }

    #[test]
    fn the_pc_answers_what_it_can() {
        assert_eq!(answered_by_device(Qid::Games, &beast()), Some("yes"));
        assert_eq!(answered_by_device(Qid::Games, &surface_pro_7()), None);
        assert!(!fits_device(Qid::LocalAi, &surface_pro_7()));
        let mut b = beast();
        b.free_disk_mb = 300_000;
        assert!(fits_device(Qid::LocalAi, &b));
    }
}
