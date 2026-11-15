//! Ideas the model writes for this person: automations, reminders, skills, and
//! things to try, grounded in what they actually work on.
//!
//! The cabin used to fill the Ideas board from templates ("A chip for the next
//! step", "Set up: exit 1 · 9ms"). Those never said anything about the person's
//! work. This module holds the checks an idea must pass: topic matching, the
//! reason check against their asks and lasting context, and template titles.

use serde::{Deserialize, Serialize};

/// Ask again at most this often.
pub const IDEAS_REFRESH_MS: u64 = 6 * 60 * 60 * 1000;
/// With this many live generated ideas on the board, do not ask for more.
pub const IDEAS_LIVE_ENOUGH: usize = 4;
/// Source id on cards the model wrote.
pub const IDEA_SOURCE_GENERATED: &str = "gen";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IdeaKind {
    /// Runs on a clock or an interval.
    Automation,
    /// A one-time or repeating nudge.
    Reminder,
    /// A saved procedure for work that repeats.
    Skill,
    /// Something to ask the cabin right now.
    Try,
}

impl IdeaKind {
    /// The type shown first on a card: Automation, Skill, or Suggestion.
    /// A reminder is a scheduled nudge, so it reads as an automation.
    pub fn type_label(self) -> &'static str {
        match self {
            Self::Automation | Self::Reminder => "Automation",
            Self::Skill => "Skill",
            Self::Try => "Suggestion",
        }
    }

    /// Short line shown as the card's "why this" label.
    pub fn why_label(self) -> &'static str {
        match self {
            Self::Automation => "An automation you can turn on",
            Self::Reminder => "A reminder for your work",
            Self::Skill => "A skill for work that repeats",
            Self::Try => "Something to try now",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdeaSeed {
    pub kind: IdeaKind,
    pub title: String,
    /// One short line under the title.
    pub body: String,
    /// The full explanation shown when the card opens.
    pub details: String,
    /// The exact message to send the cabin to do it.
    pub prompt: String,
    /// Why this is worth making: the pattern in their work it answers.
    /// Shown on the card. Empty when an older reply left it out.
    pub reason: String,
}

/// Words that say nothing about the topic of an ask.
const STOP_WORDS: &[&str] = &[
    "about", "after", "again", "also", "before", "could", "every", "from", "have", "help", "into",
    "just", "like", "make", "need", "please", "should", "some", "that", "than", "them", "then",
    "there", "these", "they", "this", "what", "when", "where", "which", "while", "will", "with",
    "would", "your", "yours", "mine", "it's", "want", "wanna", "gonna", "thing", "things", "still",
    "work", "working", "doesn't", "does", "done", "can't", "cant", "dont", "don't", "today",
    "tomorrow", "check", "show", "tell", "give", "using", "used", "being", "been", "were", "here",
    "each", "time", "times", "daily", "weekly", "weekday", "weekdays", "morning", "night", "hour",
    "hours", "minute", "minutes", "cabin",
];

/// Verbs of a job you do once. An ask led by one of these is not a routine.
const ONE_OFF_STEMS: &[&str] = &[
    "install",
    "reinstall",
    "uninstall",
    "setup",
    "configur",
    "fix",
    "repair",
    "troubleshoot",
    "debug",
    "download",
    "flash",
    "mount",
    "upgrad",
    "migrat",
    "recover",
    "restor",
    "reset",
    "unbrick",
    "partition",
    "format",
    "replac",
];

fn stem(word: &str) -> String {
    let w = word.trim_matches(|c: char| !c.is_ascii_alphanumeric());
    for suffix in ["ing", "ed", "es", "s"] {
        if w.len() > suffix.len() + 3 {
            if let Some(base) = w.strip_suffix(suffix) {
                return base.to_string();
            }
        }
    }
    w.to_string()
}

/// The words that name what a line is about: lowercased, stemmed, no filler.
pub fn topic_words(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for raw in text
        .to_ascii_lowercase()
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '\'' || c == '-'))
    {
        let raw = raw.trim_matches(['\'', '-']);
        if raw.len() < 4 || STOP_WORDS.contains(&raw) || raw.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let w = stem(raw);
        if w.len() >= 3 && !out.contains(&w) {
            out.push(w);
        }
    }
    out
}

/// Two lines are about the same thing when they share two topic words, or one
/// when either line has only one or two to begin with.
pub fn same_topic(a: &str, b: &str) -> bool {
    same_topic_words(&topic_words(a), &topic_words(b))
}

/// [`same_topic`] on [`topic_words`] already worked out, for callers that
/// compare one line against many.
pub fn same_topic_words(wa: &[String], wb: &[String]) -> bool {
    if wa.is_empty() || wb.is_empty() {
        return false;
    }
    let shared = wa.iter().filter(|w| wb.contains(w)).count();
    shared >= 2 || (shared == 1 && wa.len().min(wb.len()) <= 2)
}

/// An ask for a job you do once: install a driver, fix a bug, set something up.
pub fn is_one_off_ask(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    if lower.contains("set up") || lower.contains("set-up") {
        return true;
    }
    lower
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .take(6)
        .any(|w| ONE_OFF_STEMS.iter().any(|s| w.starts_with(s)))
}

/// What an idea may stand on: their recent asks, and lasting context (memory
/// and USER.md lines, open workboard cards).
#[derive(Debug, Clone, Copy, Default)]
pub struct IdeaGround<'a> {
    pub asks: &'a [String],
    pub lasting: &'a [String],
}

/// Does this idea answer something real? An automation, reminder, or skill needs
/// work they repeat (two or more asks on the topic that are not one-time jobs) or
/// lasting context. A Try needs at least one ask or lasting line on the topic that
/// is not a one-time job. An idea built on one install or one fix has no reason.
pub fn idea_has_reason(kind: IdeaKind, text: &str, ground: &IdeaGround) -> bool {
    let lasting = ground
        .lasting
        .iter()
        .any(|l| !l.trim().is_empty() && same_topic(text, l) && !is_one_off_ask(l));
    if lasting {
        return true;
    }
    let routine = ground
        .asks
        .iter()
        .filter(|a| same_topic(text, a) && !is_one_off_ask(a))
        .count();
    match kind {
        IdeaKind::Automation | IdeaKind::Reminder | IdeaKind::Skill => routine >= 2,
        IdeaKind::Try => routine >= 1,
    }
}

/// An idea that only answers a one-time job: every recent ask on its topic is
/// an install, a fix, or a setup, and nothing lasting backs it. Ideas with no
/// matching ask at all are left alone (their evidence may have aged out).
pub fn idea_from_one_off(text: &str, ground: &IdeaGround) -> bool {
    let lasting = ground
        .lasting
        .iter()
        .any(|l| same_topic(text, l) && !is_one_off_ask(l));
    if lasting {
        return false;
    }
    let mut matches = ground
        .asks
        .iter()
        .filter(|a| same_topic(text, a))
        .peekable();
    matches.peek().is_some() && matches.all(|a| is_one_off_ask(a))
}

/// The words an idea is about, for the reason check and for one-per-topic.
pub fn idea_topic_text(seed: &IdeaSeed) -> String {
    format!("{} {} {}", seed.title, seed.body, seed.prompt)
}

/// Titles the old template generators wrote. An untouched card with one of these goes.
pub fn is_template_idea_title(title: &str) -> bool {
    let t = title.trim().to_ascii_lowercase();
    const EXACT: &[&str] = &[
        "morning reminder",
        "nightly wrap-up",
        "remind me later",
        "ping me when i sit down",
        "a next step from the brief",
        "a reminder that runs on its own",
        "an end-of-day wrap",
        "a friday routine",
        "a morning routine",
        "a night routine",
        "a way of working to keep",
        "idea",
    ];
    const STARTS: &[&str] = &["set up:", "setup:", "a chip for", "a chip to"];
    EXACT.contains(&t.as_str()) || STARTS.iter().any(|p| t.starts_with(p))
}

/// Ask for ideas when the board is thin and the last ask is old enough.
pub fn ideas_refresh_due(last_ms: u64, now_ms: u64, live_generated: usize) -> bool {
    live_generated < IDEAS_LIVE_ENOUGH
        && (last_ms == 0 || now_ms.saturating_sub(last_ms) >= IDEAS_REFRESH_MS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_time_jobs_are_recognized() {
        let asks = vec![
            "the nvidia driver install failed, try installing it again".to_string(),
            "install the nvidia 550 driver for my 4070".to_string(),
        ];
        for a in &asks {
            assert!(is_one_off_ask(a), "{a}");
        }
        assert!(!is_one_off_ask("summarize my open GitHub issues"));
        assert!(is_one_off_ask("can you set up the printer"));
    }

    #[test]
    fn untouched_one_off_ideas_are_recognized_for_cleanup() {
        let asks = vec![
            "install the nvidia driver".to_string(),
            "summarize my open github issues".to_string(),
        ];
        let ground = IdeaGround {
            asks: &asks,
            lasting: &[],
        };
        assert!(idea_from_one_off(
            "Driver update check: every monday check the nvidia driver",
            &ground
        ));
        assert!(!idea_from_one_off(
            "Issue digest: summarize open github issues",
            &ground
        ));
        assert!(
            !idea_from_one_off("Stream prep before Friday streams", &ground),
            "no matching ask: left alone"
        );
    }

    #[test]
    fn template_titles_are_recognized() {
        for t in [
            "Set up: run debug",
            "Set up: exit 1 · 9ms",
            "A chip for the next slice",
            "A chip for when something fails",
            "Ping me when I sit down",
            "Remind me later",
            "A next step from the brief",
        ] {
            assert!(is_template_idea_title(t), "{t}");
        }
        assert!(!is_template_idea_title("Morning test run"));
    }

    #[test]
    fn refresh_waits_for_a_thin_board_and_the_interval() {
        assert!(ideas_refresh_due(0, 10, 0));
        assert!(!ideas_refresh_due(0, 10, IDEAS_LIVE_ENOUGH));
        assert!(!ideas_refresh_due(1_000, 1_000 + IDEAS_REFRESH_MS - 1, 0));
        assert!(ideas_refresh_due(1_000, 1_000 + IDEAS_REFRESH_MS, 0));
    }

    #[test]
    fn kinds_have_card_labels_and_lowercase_json() {
        assert_eq!(IdeaKind::Try.type_label(), "Suggestion");
        assert_eq!(IdeaKind::Reminder.type_label(), "Automation");
        assert_eq!(IdeaKind::Skill.type_label(), "Skill");
        let json = serde_json::to_string(&IdeaKind::Try).unwrap();
        assert_eq!(json, "\"try\"");
    }
}
