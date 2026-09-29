//! What a turn was for. Welcome, chips, ideas, skills, and automations
//! read this instead of copying the person's words.
//!
//! The strings here are fixed. A turn only chooses which one fits.

use crate::is_plain_text;
use crate::learning::is_greeting_chitchat;

/// Which kind of help the turn called for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveKind {
    Automate,
    Wrap,
    Friday,
    Morning,
    Night,
    Fix,
    Decide,
    Explain,
    Build,
    Computer,
    Imagine,
    Preference,
    Next,
}

impl MoveKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Automate => "automate",
            Self::Wrap => "wrap",
            Self::Friday => "friday",
            Self::Morning => "morning",
            Self::Night => "night",
            Self::Fix => "fix",
            Self::Decide => "decide",
            Self::Explain => "explain",
            Self::Build => "build",
            Self::Computer => "computer",
            Self::Imagine => "imagine",
            Self::Preference => "preference",
            Self::Next => "next",
        }
    }
}

/// Help built from a turn: why it happened, the chip that would have
/// helped, a skill for the next chat like it, and an automation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LearnedMove {
    pub kind: MoveKind,
    pub why: String,
    pub chip_label: String,
    pub chip_value: String,
    pub chip_hint: String,
    pub skill_name: String,
    pub skill_description: String,
    pub skill_trigger: String,
    pub skill_steps: String,
    pub idea_title: String,
    pub idea_body: String,
    pub auto_title: String,
    pub auto_body: String,
}

/// True when `proposal` repeats `source` closely enough to be a quote.
pub fn echoes_source(proposal: &str, source: &str) -> bool {
    if repeats_words(proposal, source) {
        return true;
    }
    let proposal = norm(proposal);
    let source = norm(source);
    let words = source.split_whitespace().count();
    words >= 3 && source.chars().count() >= 12 && (proposal == source || proposal.contains(&source))
}

fn norm(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

fn repeats_words(proposal: &str, source: &str) -> bool {
    let src = words(source);
    let prop = words(proposal);
    if src.len() < 4 || prop.len() < 4 {
        return false;
    }
    src.windows(4).any(|window| prop.windows(4).any(|part| part == window))
}

fn words(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| w.len() >= 2)
        .map(|w| w.to_ascii_lowercase())
        .collect()
}

/// Read one exchange and say what would have helped. `None` for greetings,
/// secrets, and lines too thin to learn from.
pub fn learn_from_turns(user: &str, assistant: &str) -> Option<LearnedMove> {
    let user = user.trim();
    if is_greeting_chitchat(user) || user.starts_with('/') || user.starts_with("HOST_") {
        return None;
    }
    if !user.is_empty() && !is_plain_text(user) {
        return None;
    }
    classify(user, assistant).map(move_for)
}

fn classify(user: &str, assistant: &str) -> Option<MoveKind> {
    let user_l = user.to_ascii_lowercase();
    let asst_l = assistant.to_ascii_lowercase();
    if !user_l.is_empty() {
        if wants_automation(&user_l) {
            return Some(MoveKind::Automate);
        }
        if has_token(&user_l, &["wrap"]) {
            return Some(MoveKind::Wrap);
        }
        if has_token(&user_l, &["friday", "fridays"]) {
            return Some(MoveKind::Friday);
        }
        if has_token(&user_l, &["morning", "mornings"]) {
            return Some(MoveKind::Morning);
        }
        if has_token(&user_l, &["night", "nights", "nightly"]) {
            return Some(MoveKind::Night);
        }
    }
    let blob = format!("{user_l}\n{asst_l}");
    if has_token(
        &blob,
        &["error", "failed", "failure", "crash", "broken", "exception", "bug"],
    ) {
        return Some(MoveKind::Fix);
    }
    if has_token(&user_l, &["recommend", "tradeoff"])
        || user_l.contains("should i")
        || user_l.contains("which ")
    {
        return Some(MoveKind::Decide);
    }
    if user_l.contains("explain")
        || user_l.contains("how does")
        || user_l.contains("what is")
        || user_l.contains("why does")
        || user_l.contains("walk me")
    {
        return Some(MoveKind::Explain);
    }
    if blob.contains("imagine")
        || blob.contains("draw ")
        || has_token(&blob, &["logo"])
        || blob.contains("generate an image")
    {
        return Some(MoveKind::Imagine);
    }
    if blob.contains("host_cmd") || blob.contains("desktop") || blob.contains("on this computer") {
        return Some(MoveKind::Computer);
    }
    if user_l.contains("implement")
        || user_l.contains("build ")
        || user_l.contains("create ")
        || user_l.contains("wire ")
        || user_l.contains("patch ")
    {
        return Some(MoveKind::Build);
    }
    if user_l.contains("prefer ")
        || user_l.contains("i use ")
        || user_l.contains("i always")
        || user_l.contains("i never")
    {
        return Some(MoveKind::Preference);
    }
    if word_count(user) >= 3 || (!assistant.trim().is_empty() && word_count(user) >= 1) {
        return Some(MoveKind::Next);
    }
    if assistant.trim().chars().count() >= 24 {
        return Some(MoveKind::Next);
    }
    None
}

fn wants_automation(user: &str) -> bool {
    const PHRASES: &[&str] = &[
        "remind me",
        "remember to",
        "every time",
        "whenever",
        "automate",
        "each time",
        "every day",
        "every weekday",
        "every morning",
        "every night",
        "every hour",
        "every week",
        "do not ask",
        "don't ask",
        "without asking",
    ];
    PHRASES.iter().any(|phrase| user.contains(phrase))
}

fn has_token(text: &str, tokens: &[&str]) -> bool {
    text.split(|c: char| !c.is_ascii_alphanumeric())
        .any(|word| tokens.iter().any(|token| word == *token))
}

fn word_count(text: &str) -> usize {
    text.split_whitespace().filter(|w| !w.is_empty()).count()
}

fn move_for(kind: MoveKind) -> LearnedMove {
    let (why, chip_label, chip_value, chip_hint, skill_name, skill_description, skill_trigger, skill_steps, idea_title, idea_body, auto_title, auto_body) =
        match kind {
            MoveKind::Automate => (
                "They had to ask for something that should happen on its own.",
                "Save this routine",
                "Turn this kind of ask into a routine that runs without being typed again.",
                "So this ask does not have to be repeated",
                "save-the-routine",
                "Runs a repeated ask without waiting for it to be typed again.",
                "this should happen without asking",
                "1. Recognize this kind of ask.\n2. Do the step they would have had to request.\n3. Leave a short result.",
                "A reminder that runs on its own",
                "A chip saves the routine. A skill runs it next time. An automation fires it so you do not ask again.",
                "Run this without asking",
                "Fire the saved routine on a clock you choose.",
            ),
            MoveKind::Wrap => (
                "The day needed a close, without another request for it.",
                "Wrap the day",
                "Summarize what got done and what is still open. Keep it short.",
                "The day needed a close",
                "end-of-day-wrap",
                "Closes the day with what got done and what is still open.",
                "the day is done",
                "1. List what got done.\n2. List what is still open.\n3. Stop at one screen.",
                "An end-of-day wrap",
                "A chip closes the day. A skill repeats that close. An automation can run it each night.",
                "Nightly wrap",
                "Post a short close of the day on the home feed.",
            ),
            MoveKind::Friday => (
                "A Friday pattern showed up and should not need to be said again.",
                "Friday routine",
                "Do the Friday step and say when it is done.",
                "A Friday pattern is worth keeping",
                "friday-routine",
                "Does the Friday step without waiting for it to be described again.",
                "it is Friday",
                "1. Do the Friday step.\n2. Say when it is done.",
                "A Friday routine",
                "A chip starts the Friday step. A skill remembers it. An automation can run it on Fridays.",
                "Friday routine",
                "Run the Friday step on a clock you choose.",
            ),
            MoveKind::Morning => (
                "A morning step showed up and should be ready when the day starts.",
                "Morning routine",
                "Do the morning step and keep the note short.",
                "The morning step should be ready",
                "morning-routine",
                "Does the morning step when the day starts.",
                "the morning starts",
                "1. Do the morning step.\n2. Leave a short note.",
                "A morning routine",
                "A chip starts the morning step. A skill repeats it. An automation can run it on weekday mornings.",
                "Morning routine",
                "Run the morning step when the day starts.",
            ),
            MoveKind::Night => (
                "A night step showed up and should run after the day, without another ask.",
                "Night routine",
                "Do the night step after the day and keep the note short.",
                "The night step should run on its own",
                "night-routine",
                "Does the night step after the day.",
                "the day is over",
                "1. Do the night step.\n2. Leave a short note.",
                "A night routine",
                "A chip starts the night step. A skill repeats it. An automation can run it after the day.",
                "Night routine",
                "Run the night step after the day.",
            ),
            MoveKind::Fix => (
                "Something failed, and the useful next step is the cause.",
                "Fix the cause",
                "Find why the last step failed, change that cause, and say how to tell it worked.",
                "The last step failed",
                "fix-the-cause",
                "Finds the cause of a failed step and checks that the fix worked.",
                "a step fails",
                "1. Name the cause in one line.\n2. Change that cause.\n3. Say how to tell it worked.",
                "A chip for when something fails",
                "A chip goes after the cause. A skill repeats that fix. An automation can retry this kind of failure.",
                "Retry this kind of failure",
                "When this kind of failure shows up, run the fix without waiting.",
            ),
            MoveKind::Decide => (
                "They were choosing and needed a recommendation plus the first step.",
                "Recommend and start",
                "Recommend one option for this situation and take the first step.",
                "A choice needed a recommendation",
                "choose-and-start",
                "Picks one option and starts it.",
                "help me choose",
                "1. Recommend one option.\n2. Say why in one line.\n3. Take the first step.",
                "A chip for choosing",
                "A chip recommends and starts. A skill does that for the next choice. An automation is only worth it if the choice repeats.",
                "Repeat this choice",
                "When this choice shows up again, recommend and start.",
            ),
            MoveKind::Explain => (
                "They asked because the reason was not already in front of them.",
                "Explain the why",
                "Say why this works, in a few sentences, then the one next step.",
                "The reason was not already there",
                "explain-the-why",
                "Gives the reason first, then one next step.",
                "explain why",
                "1. Say why in a few sentences.\n2. Name the one next step.",
                "A chip for the reason",
                "A chip gives the reason. A skill answers this kind of why. An automation can keep the answer ready.",
                "Keep this explanation ready",
                "Answer this kind of why without waiting to be asked.",
            ),
            MoveKind::Build => (
                "They wanted the work done in this chat, not a restatement of the ask.",
                "Do the next slice",
                "Do the next small slice of this work. Inspect what you need, then apply it.",
                "The work needed a next slice",
                "do-the-next-slice",
                "Does the next small slice of this kind of work.",
                "do the next slice of this work",
                "1. Inspect only what the slice needs.\n2. Apply that slice.\n3. Say how to tell it worked.",
                "A chip for the next slice",
                "A chip does the next slice. A skill repeats that kind of work. An automation can run it when the ask repeats.",
                "Repeat this slice",
                "When this kind of work shows up again, do the slice.",
            ),
            MoveKind::Computer => (
                "They wanted this computer to do the step.",
                "Do it here",
                "Do the step on this computer. Drive the desktop if the step needs it, then summarize.",
                "The step belongs on this computer",
                "do-it-here",
                "Does the step on this computer and summarizes the result.",
                "do this on the computer",
                "1. Do the step on this computer.\n2. Summarize the result.",
                "A chip for doing it here",
                "A chip does the step here. A skill repeats it. An automation can run it on a clock.",
                "Run this here",
                "Do this kind of computer step without waiting to be asked.",
            ),
            MoveKind::Imagine => (
                "They wanted a picture, not a description of one.",
                "Make the image",
                "Make the still they were asking for and show it.",
                "A picture was the point",
                "make-the-image",
                "Makes the still instead of only describing it.",
                "make the image",
                "1. Make the still.\n2. Show it.",
                "A chip for the picture",
                "A chip makes the still. A skill repeats that kind of image. An automation can make it on a clock.",
                "Make this image again",
                "Make this kind of still without waiting to be asked.",
            ),
            MoveKind::Preference => (
                "They told the cabin how to work with them next time.",
                "Keep this way",
                "Keep working this way on the next turn without asking again.",
                "A way of working to keep",
                "keep-this-way",
                "Keeps a way of working without asking for it again.",
                "keep working this way",
                "1. Keep this way of working.\n2. Do not ask for it again.",
                "A way of working to keep",
                "A chip keeps the way of working. A skill applies it next time. It should not need to be said again.",
                "Keep this way of working",
                "Apply this way of working without asking again.",
            ),
            MoveKind::Next => (
                "They handed over a task, and the next chip should move it forward.",
                "Take the next step",
                "Continue from the goal of this chat. Do the next concrete step and stop when it is done.",
                "The task needs a next step",
                "take-the-next-step",
                "Takes the next concrete step for this kind of task.",
                "this kind of task comes up again",
                "1. Do the next concrete step.\n2. Stop when it is done.",
                "A chip for the next step",
                "A chip takes the next step. A skill does this kind of task again. An automation can run it if it repeats.",
                "Do this kind of task again",
                "When this kind of task shows up, do the next step.",
            ),
        };
    LearnedMove {
        kind,
        why: why.into(),
        chip_label: chip_label.into(),
        chip_value: chip_value.into(),
        chip_hint: chip_hint.into(),
        skill_name: skill_name.into(),
        skill_description: skill_description.into(),
        skill_trigger: skill_trigger.into(),
        skill_steps: skill_steps.into(),
        idea_title: idea_title.into(),
        idea_body: idea_body.into(),
        auto_title: auto_title.into(),
        auto_body: auto_body.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spoken(user: &str) -> LearnedMove {
        learn_from_turns(user, "ok").expect("a turn worth learning")
    }

    #[test]
    fn a_reminder_is_not_a_quote_of_the_ask() {
        let said = "remind me to start the stream checklist on Fridays";
        let learned = spoken(said);
        assert_eq!(learned.kind, MoveKind::Automate);
        assert!(!learned.idea_title.to_ascii_lowercase().contains("stream"));
        assert!(!learned.idea_title.to_ascii_lowercase().contains("checklist"));
        for field in [
            &learned.why,
            &learned.chip_label,
            &learned.chip_value,
            &learned.skill_description,
            &learned.skill_trigger,
            &learned.skill_steps,
            &learned.idea_title,
            &learned.idea_body,
            &learned.auto_title,
            &learned.auto_body,
        ] {
            assert!(!echoes_source(field, said), "{field}");
            assert!(field.chars().count() <= 220, "{field}");
        }
        assert!(learned.idea_body.chars().count() <= 160, "{}", learned.idea_body);
    }

    #[test]
    fn friday_and_a_wrap_are_different_ideas() {
        let friday = spoken("Ships on Friday nights.");
        let wrap = spoken("A nightly wrap of what got done.");
        assert_eq!(friday.kind, MoveKind::Friday);
        assert_eq!(wrap.kind, MoveKind::Wrap);
        assert_ne!(friday.idea_title, wrap.idea_title);
        assert!(!friday.idea_title.contains("Ships"));
        assert!(!wrap.idea_title.to_ascii_lowercase().contains("what got done"));
    }

    #[test]
    fn a_failure_becomes_a_cause_chip_not_the_sentence() {
        let said = "the build failed after the last change";
        let learned = learn_from_turns(said, "TypeError: foo is not a function.").unwrap();
        assert_eq!(learned.kind, MoveKind::Fix);
        assert_eq!(learned.chip_label, "Fix the cause");
        assert!(!echoes_source(&learned.chip_value, said));
        assert!(!learned.chip_value.contains("TypeError"));
    }

    #[test]
    fn greetings_and_secrets_are_not_lessons() {
        assert!(learn_from_turns("hi", "hello").is_none());
        assert!(learn_from_turns("how are you", "fine").is_none());
        assert!(learn_from_turns("token sk-abcdefghijklmnopqrstuv", "ok").is_none());
    }

    #[test]
    fn tonight_is_not_a_night_routine() {
        let learned = spoken("paint the wall tonight please");
        assert_eq!(learned.kind, MoveKind::Next);
        assert!(!echoes_source(&learned.idea_title, "paint the wall tonight please"));
    }
}
