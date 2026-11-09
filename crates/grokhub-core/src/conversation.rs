//! The conversation core: how every voice talks like a person. It sits under
//! every personality, the default included. The personality sets tone, and the
//! core keeps how a good human talks: their length, full sentences, "you",
//! no narration of the app's own machinery, and the same warmth through tool
//! work and errors.
//!
//! Three parts:
//! - [`CORE`], a short block in the cached chat prefix (stable, so it costs a
//!   cache read, not a fresh prompt, every turn);
//! - [`cue`], one per-turn line picked from the turn's shape, so the core
//!   holds over long chats, model switches and compaction (personas drift
//!   back to the model default without a per-turn reminder);
//! - [`tells`], a deterministic check of a finished reply. It never edits a
//!   reply and never costs a model call: the next turn's cue names what slipped.
//!
//! The tell lists stay small on purpose. Banning thousands of slop phrases
//! breaks output (Antislop, arXiv 2510.15061); steering does the work, and the
//! lists only catch what the real Loadout logs showed.

/// The core block for the cached chat prefix. Safety, approvals, honesty and
/// the hard limits sit above it; a personality sets tone on top of it.
pub const CORE: &str = "<conversation>
How you talk in every voice. The personality sets tone on top of this; the safety, approval and honesty rules above win over both.
- Talk with them, not about them: say \"you\", never \"the user\".
- Match their length and energy. A few words get a sentence or two back; a big question gets a full answer. Answer first: no restating the question, no stock openers or sign-offs like \"Certainly!\", \"Great question\", \"I hope this helps\" or \"Let me know if...\".
- Use full, natural sentences with contractions, never clipped notes like \"Card posted. Pick fix?\". Short means fewer things, not missing words.
- React to what they actually said and to earlier parts of the chat. A quick human reaction or a light joke is fine when it fits. Ask at most one question, and only a real one.
- Prose by default; lists only for truly parallel items, never for small talk.
- Never narrate your own machinery: checklists, todo items, cards being posted, word limits, styles or playbooks. Say what happened and what's next in plain words.
- Keep the same voice and warmth through long tool work, errors and repairs. Status lines are what a person would say, and mistakes get owned plainly.
- When unsure, say so like a person (\"not sure, let me check\") and check.
</conversation>";

/// Most bytes [`CORE`] may take in the cached prefix.
pub const CORE_MAX_BYTES: usize = 1_400;

/// One way a finished reply sounded like a bot instead of a person.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Tell {
    /// "Certainly!", "Great question", "I'd be happy to".
    StockOpener,
    /// "I hope this helps", "Let me know if you need anything else".
    StockCloser,
    /// "The user wants...": talking about them, not to them.
    ThirdPerson,
    /// Checklists, todo items, cards posted, word caps, "task complete".
    ProcessTalk,
    /// "Rust and recovery cleaned. 28.41 GB freed total. Card has details."
    Telegraph,
    /// "delve", "a testament to", "it's worth noting".
    Slop,
    /// A bulleted list answering small talk.
    CasualList,
}

impl Tell {
    pub const ALL: [Tell; 7] = [
        Tell::StockOpener,
        Tell::StockCloser,
        Tell::ThirdPerson,
        Tell::ProcessTalk,
        Tell::Telegraph,
        Tell::Slop,
        Tell::CasualList,
    ];

    /// What the next turn's cue says about it.
    pub fn fix(self) -> &'static str {
        match self {
            Tell::StockOpener => "it opened with a stock phrase, so start with the answer",
            Tell::StockCloser => "it ended on a stock sign-off, so just stop when you're done",
            Tell::ThirdPerson => "it called them \"the user\", so talk to them as \"you\"",
            Tell::ProcessTalk => {
                "it narrated checklists, cards or limits, so say what happened in plain words"
            }
            Tell::Telegraph => "it read like clipped notes, so use full, natural sentences",
            Tell::Slop => "it used stock filler words, so say it plainly",
            Tell::CasualList => "it answered small talk with a list, so talk in prose",
        }
    }

    /// Short id for eval reports.
    pub fn id(self) -> &'static str {
        match self {
            Tell::StockOpener => "stock-opener",
            Tell::StockCloser => "stock-closer",
            Tell::ThirdPerson => "third-person",
            Tell::ProcessTalk => "process-talk",
            Tell::Telegraph => "telegraph",
            Tell::Slop => "slop",
            Tell::CasualList => "casual-list",
        }
    }
}

/// First words of a stock opener, anywhere in the first sentence's start.
const OPENERS: [&str; 10] = [
    "certainly",
    "great question",
    "good question",
    "what a great question",
    "i'd be happy to",
    "i would be happy to",
    "happy to help",
    "as an ai",
    "as a language model",
    "i understand your concern",
];

/// Openers that are stock only as a whole exclamation ("Absolutely!").
const OPENER_EXCLAIMS: [&str; 3] = ["absolutely", "of course", "sure thing"];

const CLOSERS: [&str; 11] = [
    "i hope this helps",
    "hope this helps",
    "hope that helps",
    "let me know if you need",
    "let me know if you have any",
    "let me know if there's anything",
    "feel free to ask",
    "feel free to reach out",
    "don't hesitate to",
    "is there anything else",
    "anything else i can help",
];

/// "the user" followed by one of these is the model talking about them.
const THIRD_PERSON: [&str; 18] = [
    "the user wants",
    "the user asked",
    "the user has",
    "the user hasn't",
    "the user is",
    "the user said",
    "the user provided",
    "the user flagged",
    "the user noted",
    "the user confirmed",
    "the user picked",
    "the user needs",
    "the user may",
    "the user did",
    "the user's request",
    "the user's ask",
    "user not confirmed",
    "user has not",
];

const PROCESS: [&str; 22] = [
    "checklist",
    "todo item",
    "todo list",
    "todo_write",
    "card posted",
    "card is posted",
    "card has been posted",
    "card was posted",
    "posted the card",
    "words or fewer",
    "word limit",
    "word cap",
    "under 80 words",
    "under 120 words",
    "under 60 words",
    "talk style",
    "playbook",
    "task complete",
    "task is complete",
    "marked as completed",
    "marked completed",
    "checkpoints marked",
];

const SLOP: [&str; 10] = [
    "delve",
    "tapestry",
    "a testament to",
    "it's important to note",
    "it is important to note",
    "it's worth noting",
    "it is worth noting",
    "navigate the complexities",
    "rest assured",
    "in today's fast-paced",
];

/// Little words a person's sentence nearly always has and a clipped note drops
/// ("and" stays out: notes keep it, as in "Rust and recovery cleaned").
const FUNCTION_WORDS: [&str; 36] = [
    "i", "i'm", "i've", "i'll", "i'd", "you", "you're", "you've", "your", "we", "we're", "it",
    "it's", "its", "that", "that's", "this", "the", "a", "an", "is", "are", "was", "were", "be",
    "they", "there", "there's", "here", "here's", "my", "me", "so", "just", "what", "how",
];

fn plain(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_fence = false;
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        // Inline code is names and commands, not talk.
        let mut code = false;
        for c in line.chars() {
            if c == '`' {
                code = !code;
                continue;
            }
            if !code {
                out.push(c);
            }
        }
        out.push('\n');
    }
    out.replace(['\u{2019}', '\u{2018}'], "'")
        .replace(['\u{201c}', '\u{201d}'], "\"")
        .to_lowercase()
}

fn is_list_line(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("- ")
        || t.starts_with("* ")
        || t.starts_with("• ")
        || t.starts_with('|')
        || t.starts_with('#')
        || t.split_once(". ")
            .is_some_and(|(n, _)| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
}

/// Prose sentences: list, table and heading lines left out. A period inside a
/// number ("28.41") does not end a sentence.
fn sentences(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in text.lines().filter(|l| !is_list_line(l)) {
        let mut cur = String::new();
        let chars: Vec<char> = line.chars().collect();
        for (i, &c) in chars.iter().enumerate() {
            cur.push(c);
            let ends =
                matches!(c, '.' | '!' | '?') && chars.get(i + 1).is_none_or(|n| n.is_whitespace());
            if ends {
                push_sentence(&mut out, &cur);
                cur.clear();
            }
        }
        push_sentence(&mut out, &cur);
    }
    out
}

fn push_sentence(out: &mut Vec<String>, s: &str) {
    let s = s.trim();
    if s.chars().any(char::is_alphanumeric) {
        out.push(s.to_string());
    }
}

fn words(s: &str) -> Vec<String> {
    s.split(|c: char| c.is_whitespace() || c == ',' || c == '(' || c == ')')
        .map(|w| {
            w.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'')
                .to_string()
        })
        .filter(|w| !w.is_empty())
        .collect()
}

/// Leading emoji, markdown and punctuation off a sentence.
fn bare_start(s: &str) -> &str {
    s.trim_start_matches(|c: char| !c.is_alphanumeric())
}

fn opener(text: &str) -> bool {
    let Some(first) = sentences(text).into_iter().next() else {
        return false;
    };
    let start = bare_start(&first);
    if OPENERS.iter().any(|o| start.starts_with(o)) {
        return true;
    }
    let exclaim = start.trim_end_matches(|c: char| !c.is_alphanumeric());
    first.trim_end().ends_with('!') && OPENER_EXCLAIMS.contains(&exclaim)
}

fn closer(text: &str) -> bool {
    let all = sentences(text);
    let tail = &all[all.len().saturating_sub(2)..];
    tail.iter().any(|s| CLOSERS.iter().any(|c| s.contains(c)))
}

/// Longest sentence, in words, that still reads as a clipped note when it has
/// none of the [`FUNCTION_WORDS`].
const CLIPPED_WORDS: usize = 7;

fn telegraph(text: &str) -> bool {
    if sentences(text).iter().any(|s| s.contains(" + ")) {
        return true;
    }
    // Two-word interjections ("Nice!", "Done.") are fine on their own.
    let counted: Vec<Vec<String>> = sentences(text)
        .iter()
        .map(|s| words(s))
        .filter(|w| w.len() >= 3)
        .collect();
    let clipped = counted
        .iter()
        .filter(|w| {
            w.len() <= CLIPPED_WORDS && !w.iter().any(|x| FUNCTION_WORDS.contains(&x.as_str()))
        })
        .count();
    clipped >= 2 && clipped * 2 >= counted.len()
}

fn casual_list(text: &str) -> bool {
    text.lines()
        .filter(|l| {
            let t = l.trim_start();
            t.starts_with("- ") || t.starts_with("* ") || t.starts_with("• ")
        })
        .count()
        >= 3
}

/// What in `reply` sounded like a bot, in [`Tell::ALL`] order. `casual_ask`
/// is true when the person's message was small talk or a short question.
pub fn tells(reply: &str, casual_ask: bool) -> Vec<Tell> {
    let text = plain(reply);
    if text.trim().is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    if opener(&text) {
        out.push(Tell::StockOpener);
    }
    if closer(&text) {
        out.push(Tell::StockCloser);
    }
    if THIRD_PERSON.iter().any(|p| text.contains(p)) {
        out.push(Tell::ThirdPerson);
    }
    if PROCESS.iter().any(|p| text.contains(p)) {
        out.push(Tell::ProcessTalk);
    }
    if telegraph(&text) {
        out.push(Tell::Telegraph);
    }
    if SLOP.iter().any(|p| text.contains(p)) {
        out.push(Tell::Slop);
    }
    if casual_ask && casual_list(&text) {
        out.push(Tell::CasualList);
    }
    out
}

/// The turn as the cue sees it: the person's words and the Brief's read.
#[derive(Debug, Clone, Copy)]
pub struct Turn<'a> {
    pub text: &'a str,
    /// The Brief's intent (`chat`, `question`, `task`, `repair`, ...).
    pub intent: &'a str,
    /// The Brief's care (`casual`, `careful`, `critical`).
    pub care: &'a str,
    /// The Brief's mood (`calm`, `urgent`, `frustrated`, `low`).
    pub mood: &'a str,
}

/// Most words a message may have and still get the short-reply cue.
pub const FEW_WORDS: usize = 8;

/// The cue when they sound stressed or the stakes are high.
pub const STEADY: &str = "Talk like a person this turn: they sound stressed, so be steady and warm, sort the problem first, no jokes, plain words.";
/// The cue for tool work.
pub const WORK: &str = "Talk like a person this turn: while you work, keep any status line short and in your voice, talk to them as you go, and finish in full, natural sentences.";
/// The cue for a few words of chat.
pub const SHORT: &str = "Talk like a person this turn: they wrote a few words, so answer in a sentence or two, like a text back.";
/// The cue for every other turn.
pub const BASE: &str =
    "Talk like a person this turn: their length and energy, full sentences, your voice.";

impl Turn<'_> {
    /// Small talk or a short question: a list or a long answer would be off.
    pub fn casual(&self) -> bool {
        matches!(self.intent, "chat" | "question") && self.word_count() <= FEW_WORDS
    }

    fn word_count(&self) -> usize {
        self.text.split_whitespace().count()
    }
}

/// The per-turn line for `turn`, plus a fix line when the last reply slipped.
pub fn cue(turn: &Turn, last: &[Tell]) -> String {
    let shape = if matches!(turn.mood, "low" | "frustrated" | "urgent") || turn.care == "critical" {
        STEADY
    } else if matches!(turn.intent, "task" | "repair" | "research") {
        WORK
    } else if turn.casual() {
        SHORT
    } else {
        BASE
    };
    let mut out = shape.to_string();
    if let Some(fix) = fix_line(last) {
        out.push(' ');
        out.push_str(&fix);
    }
    out
}

/// "Your last reply slipped: ..." for up to two tells, or `None`.
pub fn fix_line(last: &[Tell]) -> Option<String> {
    let mut seen: Vec<Tell> = last.to_vec();
    seen.sort();
    seen.dedup();
    if seen.is_empty() {
        return None;
    }
    let fixes: Vec<&str> = seen.iter().take(2).map(|t| t.fix()).collect();
    Some(format!("Your last reply slipped: {}.", fixes.join("; ")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(text: &str) -> Vec<Tell> {
        tells(text, false)
    }

    #[test]
    fn the_core_fits_its_cap_and_keeps_safety_above_it() {
        assert!(CORE.len() <= CORE_MAX_BYTES, "core is {} bytes", CORE.len());
        assert!(CORE.starts_with("<conversation>\n"));
        assert!(CORE.ends_with("</conversation>"));
        assert!(CORE.contains("the safety, approval and honesty rules above win over both"));
        assert!(CORE.contains("say \"you\", never \"the user\""));
        // The core adds no refusal or caution text: local turns keep no app-side refusals.
        for word in ["refuse", "decline", "disclaimer", "can't help"] {
            assert!(!CORE.to_lowercase().contains(word), "{word}");
        }
    }

    #[test]
    fn the_real_flat_replies_from_the_tune_up_log_trip_a_tell() {
        assert_eq!(
            t("PC checked via probes. Card posted with findings + ranked fixes."),
            vec![Tell::ProcessTalk, Tell::Telegraph]
        );
        assert_eq!(
            t("Both cancelled (user not confirmed VM delete). Task not complete. Your choice on the files?"),
            vec![Tell::ThirdPerson, Tell::Telegraph]
        );
        assert_eq!(
            t("Rust and recovery cleaned. 28.41 GB freed total. Card has details."),
            vec![Tell::Telegraph]
        );
        assert_eq!(
            t("All four checklist items are now marked as completed. Cleanup is finished and the findings card was posted. The task is complete."),
            vec![Tell::ProcessTalk]
        );
        assert_eq!(
            t("The user provided another checkpoint for this ask."),
            vec![Tell::ThirdPerson]
        );
        assert_eq!(
            t("Helpers removed and verified gone. All done, card posted."),
            vec![Tell::ProcessTalk, Tell::Telegraph]
        );
    }

    #[test]
    fn natural_replies_trip_nothing() {
        for reply in [
            "Done! I cleared the Rust build cache and freed 12.8 GB. Your C: drive has 33 GB free now.",
            "Ha, fair. What are you playing tonight?",
            "Nice!",
            "Not sure, let me check the log real quick.",
            "Your drive's at 95%, which is what's slowing things down. The biggest win is that 24 GB VM folder. Want me to clear it?",
            "Yep, that's fixed. The restore point is there if you want to undo it.",
            "Run `cargo test -p grokhub-core` and check the output.",
        ] {
            assert_eq!(t(reply), Vec::<Tell>::new(), "{reply}");
        }
    }

    #[test]
    fn stock_openers_and_closers_are_caught_but_plain_yes_is_not() {
        assert_eq!(
            t("Certainly! Here's the fix: restart the service."),
            vec![Tell::StockOpener]
        );
        assert_eq!(
            t("Great question, it depends on your GPU."),
            vec![Tell::StockOpener]
        );
        assert_eq!(
            t("Absolutely! It works on Windows too."),
            vec![Tell::StockOpener]
        );
        assert_eq!(
            t("Absolutely, it works on Windows too."),
            Vec::<Tell>::new()
        );
        assert_eq!(
            t("Your GPU driver is current. I hope this helps!"),
            vec![Tell::StockCloser]
        );
        assert_eq!(
            t("I restarted the service. Let me know if you need anything else."),
            vec![Tell::StockCloser]
        );
        // Curly quotes and emoji in front are read the same way.
        assert_eq!(
            t("🎉 I\u{2019}d be happy to help with that build."),
            vec![Tell::StockOpener]
        );
    }

    #[test]
    fn slop_and_lists_for_small_talk_are_caught() {
        assert_eq!(
            t("Let's delve into why the frame rate drops in that zone."),
            vec![Tell::Slop]
        );
        let list = "Here's how I'm doing:\n- good\n- busy\n- caffeinated";
        assert_eq!(tells(list, true), vec![Tell::CasualList]);
        assert_eq!(tells(list, false), Vec::<Tell>::new());
    }

    #[test]
    fn code_blocks_are_not_read_as_talk() {
        let reply = "Here's the command that does it.\n```\n# todo_write checklist\nrm -rf target + cache\n```";
        assert_eq!(t(reply), Vec::<Tell>::new());
    }

    #[test]
    fn the_cue_follows_the_turn_shape() {
        let turn = |text, intent, care, mood| Turn {
            text,
            intent,
            care,
            mood,
        };
        assert_eq!(cue(&turn("yo", "chat", "casual", "calm"), &[]), SHORT);
        assert_eq!(
            cue(&turn("is my gpu ok?", "question", "casual", "calm"), &[]),
            SHORT
        );
        assert_eq!(
            cue(
                &turn(
                    "this stupid game keeps crashing",
                    "repair",
                    "careful",
                    "frustrated"
                ),
                &[]
            ),
            STEADY
        );
        assert_eq!(
            cue(&turn("clean up my drive", "task", "careful", "calm"), &[]),
            WORK
        );
        assert_eq!(cue(&turn("hi", "chat", "critical", "calm"), &[]), STEADY);
        assert_eq!(
            cue(
                &turn(
                    "walk me through how frame generation actually works on my card",
                    "question",
                    "casual",
                    "calm"
                ),
                &[]
            ),
            BASE
        );
    }

    #[test]
    fn the_cue_names_up_to_two_slips_from_the_last_reply() {
        let turn = Turn {
            text: "ok",
            intent: "chat",
            care: "casual",
            mood: "calm",
        };
        assert_eq!(
            cue(&turn, &[Tell::Telegraph, Tell::ProcessTalk, Tell::Slop, Tell::ProcessTalk]),
            format!(
                "{SHORT} Your last reply slipped: it narrated checklists, cards or limits, so say what happened in plain words; it read like clipped notes, so use full, natural sentences."
            )
        );
        assert_eq!(fix_line(&[]), None);
    }

    #[test]
    fn every_tell_has_its_own_id_and_fix() {
        let ids: std::collections::BTreeSet<_> = Tell::ALL.iter().map(|t| t.id()).collect();
        assert_eq!(ids.len(), Tell::ALL.len());
        for t in Tell::ALL {
            assert!(t.fix().starts_with("it "), "{}", t.fix());
        }
    }
}
