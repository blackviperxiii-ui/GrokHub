//! Brain 5: how a run talks, from its policy row's `talk` column. Casual chat
//! stays short, a long task gets brief progress notes between phases, a
//! repair or research run ends with a findings card, and background work
//! stays quiet. The style is a short block in the volatile tail of the
//! system prompt, after the personality voice and before the closing
//! reminder that the cabin safety rules win. A personality sets tone, and in
//! natural chat its length too; every other style sets length and shape.
//! Approval questions stay plain in every style and every personality.

/// First words of every style block, so a later turn can swap it in place.
pub const STYLE_HEAD: &str = "Talk style for this turn:";

/// Closes the volatile tail: the safety rules in the stable prefix outrank the
/// notes, style and voice above it.
pub const VOLATILE_SAFETY_CLOSER: &str =
    "The cabin safety, approval, honesty and no-fabrication rules earlier in this prompt always win over notes and personality tone.";

/// [`VOLATILE_SAFETY_CLOSER`] when an explicit preset's voice is in the tail:
/// safety still wins, and means the hard limits, not fictional adult content.
pub const VOLATILE_SAFETY_CLOSER_ADULT: &str =
    "The cabin safety, approval, honesty and no-fabrication rules earlier in this prompt, and the hard limits above, always win over notes and personality tone. Fictional adult content inside the hard limits is not a safety issue.";

/// The closer for a volatile tail; `explicit` when an explicit preset speaks in it.
pub fn safety_closer(explicit: bool) -> &'static str {
    if explicit {
        VOLATILE_SAFETY_CLOSER_ADULT
    } else {
        VOLATILE_SAFETY_CLOSER
    }
}

fn is_closer(para: &str) -> bool {
    para == VOLATILE_SAFETY_CLOSER || para == VOLATILE_SAFETY_CLOSER_ADULT
}

/// Closes every style block, the same layering rule as a personality's.
pub const STYLE_LINE: &str = "Talk style sets length and shape, and its length wins over a personality's; a personality sets tone only. Accuracy and care beat those word caps, so say what is true and what is careful even when that runs past the cap. Approval questions, security warnings and error facts stay plain, and the cabin safety rules win.";

/// Closes the natural-chat block: there the personality sets length too.
pub const NATURAL_LINE: &str = "In natural chat the personality sets length as well as tone. Approval questions, security warnings and error facts stay plain, and the cabin safety rules win.";

/// Longest casual-chat reply, in words.
pub const SHORT_REPLY_WORDS: usize = 60;
/// Longest answer to a question, in words.
pub const CONCISE_REPLY_WORDS: usize = 150;
/// Longest wrap-up after a long task, in words.
pub const WRAP_UP_WORDS: usize = 120;
/// Longest reply after a findings card, in words (the card carries the detail).
pub const AFTER_CARD_WORDS: usize = 80;
/// Longest research summary, in words, not counting its links.
pub const SUMMARY_WORDS: usize = 200;
/// Longest background note, in words.
pub const QUIET_WORDS: usize = 40;

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
    /// Hanging out: the personality sets the length, and there is no cap.
    Natural,
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
        } else if has("natural") {
            Self::Natural
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

    /// The reply-length cap in words; a draft has none.
    pub fn reply_cap_words(self) -> Option<usize> {
        match self {
            Self::Short => Some(SHORT_REPLY_WORDS),
            Self::Concise => Some(CONCISE_REPLY_WORDS),
            Self::Progress => Some(WRAP_UP_WORDS),
            Self::Findings => Some(AFTER_CARD_WORDS),
            Self::Summary => Some(SUMMARY_WORDS),
            Self::Quiet => Some(QUIET_WORDS),
            Self::Draft | Self::Natural => None,
        }
    }

    /// "short, under 60 words": the style as the trail and `/why` show it.
    pub fn label(self) -> String {
        let (name, cap) = match self {
            Self::Short => ("short", "under"),
            Self::Concise => ("concise", "under"),
            Self::Progress => ("progress notes", "wrap-up under"),
            Self::Findings => ("findings card", "reply under"),
            Self::Summary => ("summary with links", "under"),
            Self::Quiet => ("quiet", "under"),
            Self::Draft => ("draft", ""),
            Self::Natural => return "natural, personality sets length".into(),
        };
        match self.reply_cap_words() {
            Some(words) => format!("{name}, {cap} {words} words"),
            None => format!("{name}, no length cap"),
        }
    }

    /// The instruction for the system prompt, ending in [`STYLE_LINE`].
    pub fn block(self) -> String {
        self.prompt_block(false)
    }

    /// [`block`] with soft length targets on the 60, 150 and 120 styles.
    /// Natural has no target. Other styles stay exact.
    pub fn prompt_block(self, soft: bool) -> String {
        let body = match self {
            Self::Short if soft => format!(
                "casual chat. Reply in one to three short sentences, about {SHORT_REPLY_WORDS} words, and go longer when care needs it. No headings, lists or preamble."
            ),
            Self::Short => format!(
                "casual chat. Reply in one to three short sentences, under {SHORT_REPLY_WORDS} words, with no headings, lists or preamble."
            ),
            Self::Concise if soft => format!(
                "answer first, in a few sentences, about {CONCISE_REPLY_WORDS} words, and go longer when care needs it. Add detail only when it changes the answer, and name the sources of facts you looked up."
            ),
            Self::Concise => format!(
                "answer first, in a few sentences, under {CONCISE_REPLY_WORDS} words. Add detail only when it changes the answer, and name the sources of facts you looked up."
            ),
            Self::Progress if soft => format!(
                "a longer task. Work through the checklist without narrating each step; the cabin shows a short progress note between phases. End with what changed and how it was checked, about {WRAP_UP_WORDS} words, and go longer when care needs it."
            ),
            Self::Progress => format!(
                "a longer task. Work through the checklist without narrating each step; the cabin shows a short progress note between phases. End with what changed and how it was checked, under {WRAP_UP_WORDS} words."
            ),
            Self::Findings => format!(
                "a repair. Work through the checklist without narrating each step; the cabin shows a short progress note between phases. End by posting the findings card (report_findings): name each item checked, its root cause, and a one-tap fix for what is left. Then reply under {AFTER_CARD_WORDS} words; the card carries the detail."
            ),
            Self::Summary => format!(
                "research. End with a summary under {SUMMARY_WORDS} words and the links you used. When the work checked named items, post the findings card (report_findings) with one-tap follow-ups."
            ),
            Self::Quiet => format!(
                "background work. Say only what the person needs, under {QUIET_WORDS} words; leave suggestions as one-tap cards."
            ),
            Self::Draft => "creative. Deliver the draft itself, then at most one line about it.".into(),
            Self::Natural => {
                return format!(
                    "{STYLE_HEAD} natural conversation. Your personality sets the length: match their energy, a quick line for a quick line and more when they open up. When they are just hanging out, stay in it with them: no task offers, no next steps, and no sign-offs or wrap-up lines. No headings, lists or preamble.\n{NATURAL_LINE}"
                )
            }
        };
        format!("{STYLE_HEAD} {body}\n{STYLE_LINE}")
    }
}

/// Whether a reply fits the style's length cap (always, for a style with none).
/// Links are not counted, so a research summary can cite what it used.
fn reply_words(reply: &str) -> usize {
    reply
        .split_whitespace()
        .filter(|w| !w.trim_start_matches(['(', '<', '[']).starts_with("http"))
        .count()
}

/// Words a soft cap allows past the hard target. Two fifths is forty percent,
/// the same room the care-first length budget allows a reply to grow.
pub fn soft_cap_words(style: TalkStyle) -> Option<usize> {
    let hard = style.reply_cap_words()?;
    let slack = matches!(
        style,
        TalkStyle::Short | TalkStyle::Concise | TalkStyle::Progress
    );
    Some(if slack { hard + hard / 5 * 2 } else { hard })
}

pub fn fits_cap(style: TalkStyle, reply: &str) -> bool {
    style
        .reply_cap_words()
        .is_none_or(|cap| reply_words(reply) <= cap)
}

/// The care-first check. Short, concise and progress may run past
/// the hard target up to [`soft_cap_words`].
pub fn fits_soft_cap(style: TalkStyle, reply: &str) -> bool {
    soft_cap_words(style).is_none_or(|cap| reply_words(reply) <= cap)
}

/// The system prompt with this turn's style block: an earlier style block is
/// replaced, and the block sits before the closer ([`safety_closer`]), which is
/// added when the prompt has none: the adults-only one when an explicit
/// preset's voice is in the prompt. Paragraphs are split on blank lines, so the
/// rest of the prompt is kept byte for byte.
pub fn restyle(system: &str, style: TalkStyle) -> String {
    restyle_caps(system, style, false)
}

/// [`restyle`] that softens the 60, 150 and 120 targets when `soft` is set.
pub fn restyle_caps(system: &str, style: TalkStyle, soft: bool) -> String {
    let mut paras: Vec<&str> = if system.is_empty() {
        Vec::new()
    } else {
        system.split("\n\n").collect()
    };
    paras.retain(|p| !p.starts_with(STYLE_HEAD));
    let block = style.prompt_block(soft);
    match paras.iter().rposition(|p| is_closer(p)) {
        Some(at) => paras.insert(at, &block),
        None => {
            paras.push(&block);
            paras.push(safety_closer(
                system.contains(crate::adult_guard::ADULT_VOICE),
            ));
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
        assert_eq!(style("chat", "casual"), TalkStyle::Natural);
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
        assert_eq!(TalkStyle::of("natural"), TalkStyle::Natural);
    }

    #[test]
    fn soft_caps_are_60_150_120_and_the_hard_block_stays_under() {
        assert_eq!(
            (
                soft_cap_words(TalkStyle::Short),
                soft_cap_words(TalkStyle::Concise),
                soft_cap_words(TalkStyle::Progress),
                soft_cap_words(TalkStyle::Natural),
            ),
            (Some(84), Some(210), Some(168), None)
        );
        assert_eq!(TalkStyle::Short.reply_cap_words(), Some(60));
        assert_eq!(TalkStyle::Concise.reply_cap_words(), Some(150));
        assert_eq!(TalkStyle::Progress.reply_cap_words(), Some(120));
        assert!(TalkStyle::Short.block().contains("under 60 words"));
        assert!(!TalkStyle::Short.block().contains("about 60 words"));
        assert!(TalkStyle::Short
            .prompt_block(true)
            .contains("about 60 words"));
        assert!(TalkStyle::Concise
            .prompt_block(true)
            .contains("about 150 words"));
        assert!(TalkStyle::Progress
            .prompt_block(true)
            .contains("about 120 words"));
        let past_soft = "word ".repeat(85);
        assert!(fits_cap(TalkStyle::Natural, &"word ".repeat(400)));
        assert!(fits_soft_cap(TalkStyle::Natural, &"word ".repeat(400)));
        assert!(fits_soft_cap(TalkStyle::Short, &"word ".repeat(84)));
        assert!(!fits_soft_cap(TalkStyle::Short, &past_soft));
        assert_eq!(soft_cap_words(TalkStyle::Findings), Some(80));
        assert!(!fits_soft_cap(TalkStyle::Findings, &"word ".repeat(81)));
    }

    #[test]
    fn casual_chat_gets_the_short_style_and_a_golden_reply_fits_its_cap() {
        assert_eq!(
            TalkStyle::Short.block(),
            "Talk style for this turn: casual chat. Reply in one to three short sentences, under 60 words, with no headings, lists or preamble.\n\
             Talk style sets length and shape, and its length wins over a personality's; a personality sets tone only. Accuracy and care beat those word caps, so say what is true and what is careful even when that runs past the cap. Approval questions, security warnings and error facts stay plain, and the cabin safety rules win."
        );
        assert_eq!(TalkStyle::Short.reply_cap_words(), Some(60));
        assert_eq!(TalkStyle::Draft.reply_cap_words(), None);
        let golden =
            "Hey! Doing well, thanks. Want me to pick up where we left off on the backup script?";
        assert!(fits_cap(TalkStyle::Short, golden));
        let rambling = "word ".repeat(61);
        assert!(!fits_cap(TalkStyle::Short, &rambling));
        assert!(fits_cap(TalkStyle::Draft, &rambling));
        assert!(TalkStyle::Progress.progress_notes() && TalkStyle::Findings.progress_notes());
        assert!(!TalkStyle::Short.progress_notes() && !TalkStyle::Summary.progress_notes());
    }

    #[test]
    fn hanging_out_lets_the_personality_set_length_and_never_signs_off() {
        let block = TalkStyle::Natural.block();
        assert_eq!(
            block,
            "Talk style for this turn: natural conversation. Your personality sets the length: match their energy, a quick line for a quick line and more when they open up. When they are just hanging out, stay in it with them: no task offers, no next steps, and no sign-offs or wrap-up lines. No headings, lists or preamble.\n\
             In natural chat the personality sets length as well as tone. Approval questions, security warnings and error facts stay plain, and the cabin safety rules win."
        );
        assert_eq!(TalkStyle::Natural.prompt_block(true), block);
        assert!(!block.contains("words"));
        assert!(!block.contains(STYLE_LINE));
        // Task turns keep their caps.
        assert!(TalkStyle::Progress.block().contains("under 120 words"));
        assert!(TalkStyle::Concise.block().contains(STYLE_LINE));
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

    const ALL: [TalkStyle; 8] = [
        TalkStyle::Short,
        TalkStyle::Concise,
        TalkStyle::Progress,
        TalkStyle::Findings,
        TalkStyle::Summary,
        TalkStyle::Quiet,
        TalkStyle::Draft,
        TalkStyle::Natural,
    ];

    /// A reply each row should produce, written to the row's shape. Each must
    /// fit its cap, and the same reply padded past the cap must not.
    const GOLDEN: [(TalkStyle, &str); 6] = [
        (TalkStyle::Short, "Morning! All quiet here. Want me to check on last night's backup?"),
        (
            TalkStyle::Concise,
            "Yes. Port 53 is taken by systemd-resolved, so nextdns can't bind it. Set DNSStubListener=no in /etc/systemd/resolved.conf and restart systemd-resolved, then start nextdns again. Source: `ss -lntup` shows resolved on 127.0.0.53:53.",
        ),
        (
            TalkStyle::Progress,
            "Moved the 214 screenshots from Desktop into Pictures/Screenshots/2026, grouped by month. Nothing was deleted. Checked: the folder holds 214 files and Desktop has none left.",
        ),
        (
            TalkStyle::Findings,
            "The printer queue was stuck on a job from Friday; I cleared it and restarted CUPS. A test page printed. The card lists the driver update that is still waiting.",
        ),
        (
            TalkStyle::Summary,
            "The Framework 16 and the XPS 15 trade blows: the Framework is repairable and upgradable, the XPS has the better screen and battery. For Linux, the Framework has first-party support. Sources: https://frame.work/laptop16 https://www.dell.com/xps15 https://wiki.archlinux.org/title/Framework_Laptop_16",
        ),
        (TalkStyle::Quiet, "Nightly backup finished: 3.2 GB, no errors."),
    ];

    #[test]
    fn each_row_has_a_tuned_cap_and_its_golden_reply_fits_it() {
        let caps: Vec<_> = ALL
            .iter()
            .map(|s| (s.label(), s.reply_cap_words()))
            .collect();
        assert_eq!(
            caps,
            vec![
                ("short, under 60 words".to_string(), Some(60)),
                ("concise, under 150 words".to_string(), Some(150)),
                (
                    "progress notes, wrap-up under 120 words".to_string(),
                    Some(120)
                ),
                ("findings card, reply under 80 words".to_string(), Some(80)),
                ("summary with links, under 200 words".to_string(), Some(200)),
                ("quiet, under 40 words".to_string(), Some(40)),
                ("draft, no length cap".to_string(), None),
                ("natural, personality sets length".to_string(), None),
            ]
        );
        for (style, reply) in GOLDEN {
            assert!(fits_cap(style, reply), "{style:?}: {reply}");
            let cap = style.reply_cap_words().unwrap();
            let padded = format!("{reply} {}", "more ".repeat(cap));
            assert!(
                !fits_cap(style, &padded),
                "{style:?} let a padded reply through"
            );
            // The block tells the model the same cap the fixture checks.
            assert!(
                style.block().contains(&format!("under {cap} words")),
                "{style:?}"
            );
        }
        // Links don't count toward a summary's words.
        let links = "https://example.com/a ".repeat(300);
        assert!(fits_cap(
            TalkStyle::Summary,
            &format!("Two sources agree. {links}")
        ));
        assert!(!TalkStyle::Draft.block().contains("words"));
    }

    #[test]
    fn every_shipped_personality_composes_with_every_style() {
        for p in crate::personality::builtins() {
            let voice = p.prompt_block();
            let closer = safety_closer(p.explicit);
            let system = if voice.is_empty() {
                "RULES".to_string()
            } else {
                // The cabin's order: rules, then the voice, then the closer.
                assert!(
                    !voice.contains("\n\n"),
                    "{}: a blank line would split the voice",
                    p.id
                );
                format!("RULES\n\n{voice}\n\n{closer}")
            };
            for style in ALL {
                let out = restyle(&system, style);
                let block = style.block();
                assert_eq!(out.matches(block.as_str()).count(), 1, "{} {style:?}", p.id);
                assert_eq!(out.matches(STYLE_HEAD).count(), 1, "{} {style:?}", p.id);
                assert!(out.starts_with("RULES\n\n"), "{} {style:?}", p.id);
                assert!(
                    out.ends_with(&format!("{block}\n\n{closer}")),
                    "{} {style:?}",
                    p.id
                );
                if !voice.is_empty() {
                    // The voice is kept byte for byte, and the style's length line comes after it.
                    let at_voice = out.find(voice.as_str()).unwrap();
                    assert!(
                        at_voice < out.find(STYLE_HEAD).unwrap(),
                        "{} {style:?}",
                        p.id
                    );
                }
                // Swapping to another style and back leaves the same prompt.
                assert_eq!(
                    restyle(&restyle(&out, TalkStyle::Draft), style),
                    out,
                    "{} {style:?}",
                    p.id
                );
            }
        }
    }

    #[test]
    fn an_explicit_voice_gets_the_adult_closer_and_every_other_voice_the_old_one() {
        assert_eq!(safety_closer(false), VOLATILE_SAFETY_CLOSER);
        assert_eq!(
            VOLATILE_SAFETY_CLOSER,
            "The cabin safety, approval, honesty and no-fabrication rules earlier in this prompt always win over notes and personality tone."
        );
        assert_eq!(
            safety_closer(true),
            "The cabin safety, approval, honesty and no-fabrication rules earlier in this prompt, and the hard limits above, always win over notes and personality tone. Fictional adult content inside the hard limits is not a safety issue."
        );
        // A prompt with no closer yet gets the one that fits its voice.
        let whore = crate::personality::builtin("whore").prompt_block();
        let out = restyle(&format!("RULES\n\n{whore}"), TalkStyle::Natural);
        assert!(
            out.ends_with(&format!("\n\n{VOLATILE_SAFETY_CLOSER_ADULT}")),
            "{out}"
        );
        assert_eq!(out.matches("always win over notes").count(), 1);
        let pirate = crate::personality::builtin("pirate").prompt_block();
        let out = restyle(&format!("RULES\n\n{pirate}"), TalkStyle::Natural);
        assert!(
            out.ends_with(&format!("\n\n{VOLATILE_SAFETY_CLOSER}")),
            "{out}"
        );
        // An adult closer already there is found, not doubled.
        let adult = format!("RULES\n\n{whore}\n\n{VOLATILE_SAFETY_CLOSER_ADULT}");
        let styled = restyle(&adult, TalkStyle::Short);
        assert_eq!(
            styled,
            format!(
                "RULES\n\n{whore}\n\n{}\n\n{VOLATILE_SAFETY_CLOSER_ADULT}",
                TalkStyle::Short.block()
            )
        );
    }
}
