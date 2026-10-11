//! Brain 5: how a run talks, from its policy row's `talk` column. Casual chat
//! stays short, a long task gets brief progress notes between phases, a
//! repair or research run ends with a findings card, and background work
//! stays quiet. The style is a short block in the volatile tail of the
//! system prompt, after the personality voice and before the closing
//! reminder that the cabin safety rules win. A personality sets tone only;
//! the style sets length and shape. Approval questions stay plain in every
//! style and every personality.

/// First words of every style block, so a later turn can swap it in place.
pub const STYLE_HEAD: &str = "Talk style for this turn:";

/// Closes the volatile tail: the safety rules in the stable prefix outrank the
/// notes, style and voice above it.
pub const VOLATILE_SAFETY_CLOSER: &str =
    "The cabin safety, approval, honesty and no-fabrication rules earlier in this prompt always win over notes and personality tone.";

/// Closes every style block, the same layering rule as a personality's.
pub const STYLE_LINE: &str = "Talk style sets length and shape only; a personality sets tone only. Approval questions, security warnings and error facts stay plain, and the cabin safety rules win.";

/// Longest casual-chat reply, in words.
pub const SHORT_REPLY_WORDS: usize = 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TalkStyle {
    /// Small talk: a few short sentences.
    Short,
    /// A question: the answer first.
    Concise,
    /// A long task: progress notes between phases, a short wrap-up.
    Progress,
    /// A repair: progress notes, then a findings card with one-tap fixes.
    Findings,
    /// Research: a short summary with links, and a findings card for named items.
    Summary,
    /// Background work: say little; one-tap cards only.
    Quiet,
    /// Creative: the draft, then one line.
    Draft,
}

impl TalkStyle {
    /// The style a policy row's free-text `talk` column names.
    pub fn of(talk: &str) -> Self {
        let t = talk.to_ascii_lowercase();
        let has = |w: &str| t.contains(w);
        if has("findings") {
            Self::Findings
        } else if has("progress") {
            Self::Progress
        } else if has("quiet") {
            Self::Quiet
        } else if has("draft") {
            Self::Draft
        } else if has("summary") {
            Self::Summary
        } else if has("brief") || has("one line") {
            Self::Short
        } else {
            Self::Concise
        }
    }

    /// Whether a run in this style posts progress notes between phases.
    pub fn progress_notes(self) -> bool {
        matches!(self, Self::Progress | Self::Findings)
    }

    /// The reply-length cap in words, for the styles that have one.
    pub fn reply_cap_words(self) -> Option<usize> {
        matches!(self, Self::Short).then_some(SHORT_REPLY_WORDS)
    }

    /// The instruction for the system prompt, ending in [`STYLE_LINE`].
    pub fn block(self) -> String {
        let body = match self {
            Self::Short => format!(
                "casual chat. Reply in one to three short sentences, under {SHORT_REPLY_WORDS} words, with no headings, lists or preamble."
            ),
            Self::Concise => "answer first, in a few sentences. Add detail only when it changes the answer, and name the sources of facts you looked up.".into(),
            Self::Progress => "a longer task. Work through the checklist without narrating each step; the cabin shows a short progress note between phases. End with what changed and how it was checked, in a few lines.".into(),
            Self::Findings => "a repair. Work through the checklist without narrating each step; the cabin shows a short progress note between phases. End by posting the findings card (report_findings): name each item checked, its root cause, and a one-tap fix for what is left. Then reply in a few lines.".into(),
            Self::Summary => "research. End with a short summary and the links you used. When the work checked named items, post the findings card (report_findings) with one-tap follow-ups.".into(),
            Self::Quiet => "background work. Say only what the person needs; leave suggestions as one-tap cards.".into(),
            Self::Draft => "creative. Deliver the draft itself, then at most one line about it.".into(),
        };
        format!("{STYLE_HEAD} {body}\n{STYLE_LINE}")
    }
}

/// Whether a reply fits the style's length cap (always, for a style with none).
pub fn fits_cap(style: TalkStyle, reply: &str) -> bool {
    style
        .reply_cap_words()
        .is_none_or(|cap| reply.split_whitespace().count() <= cap)
}

/// The system prompt with this turn's style block: an earlier style block is
/// replaced, and the block sits before [`VOLATILE_SAFETY_CLOSER`], which is
/// added when the prompt has none. Paragraphs are split on blank lines, so the
/// rest of the prompt is kept byte for byte.
pub fn restyle(system: &str, style: TalkStyle) -> String {
    let mut paras: Vec<&str> = if system.is_empty() {
        Vec::new()
    } else {
        system.split("\n\n").collect()
    };
    paras.retain(|p| !p.starts_with(STYLE_HEAD));
    let block = style.block();
    match paras.iter().rposition(|p| *p == VOLATILE_SAFETY_CLOSER) {
        Some(at) => paras.insert(at, &block),
        None => {
            paras.push(&block);
            paras.push(VOLATILE_SAFETY_CLOSER);
        }
    }
    paras.join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain_policy::{builtin_policy, pick_row};

    fn style(intent: &str, care: &str) -> TalkStyle {
        TalkStyle::of(&pick_row(&builtin_policy(), intent, care).unwrap().talk)
    }

    #[test]
    fn each_shipped_row_names_its_style() {
        assert_eq!(style("chat", "casual"), TalkStyle::Short);
        assert_eq!(style("question", "careful"), TalkStyle::Concise);
        assert_eq!(style("question", "casual"), TalkStyle::Concise);
        assert_eq!(style("task", "careful"), TalkStyle::Progress);
        assert_eq!(style("task", "casual"), TalkStyle::Progress);
        assert_eq!(style("repair", "critical"), TalkStyle::Findings);
        assert_eq!(style("research", "careful"), TalkStyle::Summary);
        assert_eq!(style("background", "casual"), TalkStyle::Quiet);
        assert_eq!(style("creative", "casual"), TalkStyle::Draft);
        assert_eq!(style("chat", "critical"), TalkStyle::Progress);
        assert_eq!(style("chat", "careful"), TalkStyle::Concise);
        // A user's own words map too.
        assert_eq!(TalkStyle::of("one line"), TalkStyle::Short);
        assert_eq!(TalkStyle::of("whatever"), TalkStyle::Concise);
    }

    #[test]
    fn casual_chat_gets_the_short_style_and_a_golden_reply_fits_its_cap() {
        assert_eq!(
            TalkStyle::Short.block(),
            "Talk style for this turn: casual chat. Reply in one to three short sentences, under 60 words, with no headings, lists or preamble.\n\
             Talk style sets length and shape only; a personality sets tone only. Approval questions, security warnings and error facts stay plain, and the cabin safety rules win."
        );
        assert_eq!(TalkStyle::Short.reply_cap_words(), Some(60));
        assert_eq!(TalkStyle::Progress.reply_cap_words(), None);
        let golden =
            "Hey! Doing well, thanks. Want me to pick up where we left off on the backup script?";
        assert!(fits_cap(TalkStyle::Short, golden));
        let rambling = "word ".repeat(61);
        assert!(!fits_cap(TalkStyle::Short, &rambling));
        assert!(fits_cap(TalkStyle::Progress, &rambling));
        assert!(TalkStyle::Progress.progress_notes() && TalkStyle::Findings.progress_notes());
        assert!(!TalkStyle::Short.progress_notes() && !TalkStyle::Summary.progress_notes());
    }

    #[test]
    fn the_style_goes_before_the_closer_and_a_later_turn_swaps_it() {
        let voice = "Personality: Pirate. Arr.\nPersonality affects tone only.";
        let system = format!("RULES\n\n{voice}\n\n{VOLATILE_SAFETY_CLOSER}");
        let short = restyle(&system, TalkStyle::Short);
        assert_eq!(
            short,
            format!(
                "RULES\n\n{voice}\n\n{}\n\n{VOLATILE_SAFETY_CLOSER}",
                TalkStyle::Short.block()
            )
        );
        // A later turn replaces the block; nothing else moves.
        let repair = restyle(&short, TalkStyle::Findings);
        assert_eq!(
            repair,
            format!(
                "RULES\n\n{voice}\n\n{}\n\n{VOLATILE_SAFETY_CLOSER}",
                TalkStyle::Findings.block()
            )
        );
        assert_eq!(restyle(&repair, TalkStyle::Short), short);
        // No closer yet (no voice or notes): the block and the closer go last.
        assert_eq!(
            restyle("RULES", TalkStyle::Concise),
            format!(
                "RULES\n\n{}\n\n{VOLATILE_SAFETY_CLOSER}",
                TalkStyle::Concise.block()
            )
        );
        assert_eq!(
            restyle("", TalkStyle::Quiet),
            format!("{}\n\n{VOLATILE_SAFETY_CLOSER}", TalkStyle::Quiet.block())
        );
    }
}
