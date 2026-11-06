//! Chips the model offers at the end of its own reply.
//!
//! The chat's cached rules ask the model to end each reply with a hidden
//! `<chips>` block of up to three `kind|target|label` lines. The app strips
//! the block from what it shows and saves, and the lines become chips the
//! ranker weighs with every other source. No extra model call: the block
//! rides on the reply the person already asked for.
//!
//! If the reply's chips keep losing to the local ones, the rule comes out of
//! the prompt for a while ([`review_chip_trailer`]).

use crate::chip_store::ChipOutcome;
use crate::chips::{
    cabin_chip_copy_ok, chip_label, nav_from_chip_value, retired_mode_value, ChipKind, ChipMemory,
    QuickChip,
};
use crate::redact::is_plain_text;

/// The rule added to the chat's cached system prompt while the trailer is on.
pub const CHIP_TRAILER_RULE: &str = "\
End every reply with a hidden block the app turns into tap buttons:
<chips>
kind|target|label
</chips>
Give 0 to 3 lines, none when no next step is obvious. kind is ask (target is \
the whole next message the person would send) or open (target is one of: \
settings, imagine, history, workboard, pulse, skills, automations). label is \
2 to 5 plain words. Each line is a different next step. Never restate your \
reply, name only things already in this chat, never offer to send, delete, \
pay or buy, and write nothing after the block.";

const OPEN: &str = "<chips>";
const CLOSE: &str = "</chips>";
/// Lines read from one block, at most.
const TRAILER_MAX: usize = 3;
/// How long the rule stays out after the reply's chips kept losing.
pub const CHIP_TRAILER_RETRY_MS: u64 = 14 * 24 * 60 * 60 * 1000;
/// Reply chips shown before their pick rate is judged.
const TRAILER_MIN_SHOWN: usize = 40;
/// The reply's chips lose when they are picked less than this share as often
/// as the other chips shown beside them.
const TRAILER_LOSING_RATIO: f32 = 0.5;
/// The id prefix of a chip from the reply. The chip log names it `reply`.
pub const REPLY_CHIP_PREFIX: &str = "reply-";

/// First words of an action a reply chip may never offer: one tap would speak,
/// spend or destroy for them.
const BANNED_VERBS: [&str; 17] = [
    "send",
    "email",
    "text",
    "message",
    "post",
    "publish",
    "tweet",
    "delete",
    "remove",
    "erase",
    "wipe",
    "uninstall",
    "pay",
    "buy",
    "purchase",
    "order",
    "transfer",
];

/// Strips `<chips>` blocks from a reply as it streams in. A chunk may end in
/// the middle of the tag, so a tail that could still become `<chips>` is held
/// until the next chunk says what it is.
#[derive(Debug, Clone, Default)]
pub struct ChipTrailerFilter {
    held: String,
    inside: bool,
    block: String,
}

impl ChipTrailerFilter {
    /// The part of `chunk` that is shown; the rest is kept for [`Self::finish`].
    pub fn feed(&mut self, chunk: &str) -> String {
        let mut rest = std::mem::take(&mut self.held);
        rest.push_str(chunk);
        let mut shown = String::new();
        loop {
            if self.inside {
                self.block.push_str(&rest);
                let Some(end) = self.block.find(CLOSE) else {
                    return shown;
                };
                rest = self.block[end + CLOSE.len()..].to_string();
                self.block.truncate(end);
                self.block.push('\n');
                self.inside = false;
                continue;
            }
            if let Some(start) = rest.find(OPEN) {
                shown.push_str(&rest[..start]);
                rest = rest[start + OPEN.len()..].to_string();
                self.inside = true;
                continue;
            }
            let keep = partial_open_len(&rest);
            let cut = rest.len() - keep;
            shown.push_str(&rest[..cut]);
            self.held = rest[cut..].to_string();
            return shown;
        }
    }

    /// End of the reply: the held tail that turned out to be text, and the
    /// block's lines (a block cut off before its close still counts).
    pub fn finish(&mut self) -> (String, String) {
        let done = std::mem::take(self);
        let tail = if done.inside {
            String::new()
        } else {
            done.held
        };
        (tail, done.block)
    }
}

/// How many bytes at the end of `s` could be the start of `<chips>`.
fn partial_open_len(s: &str) -> usize {
    (1..OPEN.len())
        .rev()
        .find(|&n| s.ends_with(&OPEN[..n]))
        .unwrap_or(0)
}

/// A whole reply without its `<chips>` block, and the block's lines.
pub fn split_chip_trailer(text: &str) -> (String, String) {
    let mut filter = ChipTrailerFilter::default();
    let mut shown = filter.feed(text);
    let (tail, block) = filter.finish();
    shown.push_str(&tail);
    (shown, block)
}

/// The block's lines as chips. Each is checked like any chip copy, a page must
/// be one the app has, and an action that speaks, spends or destroys is dropped.
pub fn parse_chip_trailer(block: &str) -> Vec<QuickChip> {
    let mut out: Vec<QuickChip> = Vec::new();
    for line in block.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if out.len() >= TRAILER_MAX {
            break;
        }
        let parts: Vec<&str> = line.splitn(3, '|').map(str::trim).collect();
        let [kind, target, label] = parts[..] else {
            continue;
        };
        let label = chip_label(label);
        let target: String = target.split_whitespace().collect::<Vec<_>>().join(" ");
        let (value, kind) = match kind.to_ascii_lowercase().as_str() {
            "ask" => (target.chars().take(400).collect::<String>(), ChipKind::Chat),
            "open" => {
                let value = format!("__nav:{}", target.to_ascii_lowercase());
                if nav_from_chip_value(&value).is_none() {
                    continue;
                }
                (value, ChipKind::Nav)
            }
            _ => continue,
        };
        if label.chars().count() < 2 || value.chars().count() < 4 {
            continue;
        }
        if !is_plain_text(&label) || !is_plain_text(&value) || retired_mode_value(&value) {
            continue;
        }
        if value.starts_with('/') || value.starts_with('$') {
            continue;
        }
        if kind == ChipKind::Chat && value.starts_with("__") {
            continue;
        }
        if !cabin_chip_copy_ok(&label, &value, "") {
            continue;
        }
        if starts_with_banned_verb(&label) || starts_with_banned_verb(&value) {
            continue;
        }
        if out
            .iter()
            .any(|c| c.label.eq_ignore_ascii_case(&label) || c.value == value)
        {
            continue;
        }
        let i = out.len();
        out.push(QuickChip {
            id: format!("{REPLY_CHIP_PREFIX}{i}"),
            label,
            value,
            kind,
            score: 97.0 - i as f32,
            hint: "Suggested in the reply".into(),
            primary: false,
        });
    }
    out
}

fn starts_with_banned_verb(s: &str) -> bool {
    let first: String = s
        .split_whitespace()
        .next()
        .unwrap_or("")
        .chars()
        .filter(char::is_ascii_alphabetic)
        .collect::<String>()
        .to_ascii_lowercase();
    BANNED_VERBS.contains(&first.as_str())
}

/// Whether the chat's prompt carries the trailer rule now.
pub fn chip_trailer_on(mem: &ChipMemory, now_ms: u64) -> bool {
    mem.trailer_off_at == 0 || now_ms.saturating_sub(mem.trailer_off_at) >= CHIP_TRAILER_RETRY_MS
}

/// Takes the rule out of the prompt when the reply's chips keep losing: after
/// enough showings since it last came back, they were picked less than half as
/// often as the other chips shown beside them. True when this call turned it off.
pub fn review_chip_trailer(mem: &mut ChipMemory, now_ms: u64) -> bool {
    if !chip_trailer_on(mem, now_ms) {
        return false;
    }
    let since = mem.trailer_off_at;
    let (mut reply_shown, mut reply_picked, mut other_shown, mut other_picked) = (0, 0, 0, 0);
    for e in mem.events.iter().filter(|e| e.at > since) {
        let picked = match &e.outcome {
            Some(ChipOutcome::Picked { key, .. }) => Some(key.as_str()),
            _ => None,
        };
        for s in &e.shown {
            let hit = picked == Some(s.key.as_str());
            if s.source == "reply" {
                reply_shown += 1;
                reply_picked += usize::from(hit);
            } else {
                other_shown += 1;
                other_picked += usize::from(hit);
            }
        }
    }
    if reply_shown < TRAILER_MIN_SHOWN || other_shown == 0 {
        return false;
    }
    let reply_rate = reply_picked as f32 / reply_shown as f32;
    let other_rate = other_picked as f32 / other_shown as f32;
    if reply_rate < other_rate * TRAILER_LOSING_RATIO {
        mem.trailer_off_at = now_ms.max(1);
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chip_store::{ChipEvent, ShownChip};

    fn stream(chunks: &[&str]) -> (String, String) {
        let mut f = ChipTrailerFilter::default();
        let mut shown: String = chunks.iter().map(|c| f.feed(c)).collect();
        let (tail, block) = f.finish();
        shown.push_str(&tail);
        (shown, block)
    }

    #[test]
    fn the_block_never_shows_however_the_stream_splits_it() {
        let reply = "Done, the tests pass.\n<chips>\nask|Add a test for login|Test login\n</chips>";
        let whole = stream(&[reply]);
        assert_eq!(whole.0, "Done, the tests pass.\n");
        assert!(whole.1.contains("ask|Add a test for login|Test login"));
        // Every split point, including inside both tags.
        for cut in 1..reply.len() {
            if !reply.is_char_boundary(cut) {
                continue;
            }
            let (a, b) = reply.split_at(cut);
            assert_eq!(stream(&[a, b]), whole, "split at {cut}");
        }
        let chars: Vec<String> = reply.chars().map(String::from).collect();
        let one_by_one: Vec<&str> = chars.iter().map(String::as_str).collect();
        assert_eq!(stream(&one_by_one), whole);
    }

    #[test]
    fn text_that_only_looks_like_the_tag_still_shows() {
        assert_eq!(
            stream(&["a < b and <ch", "eese> too"]).0,
            "a < b and <cheese> too"
        );
        assert_eq!(stream(&["ends with <chi"]).0, "ends with <chi");
        assert_eq!(stream(&["x <"]).0, "x <");
    }

    #[test]
    fn text_after_the_block_shows_and_a_cut_off_block_still_parses() {
        let (shown, block) = stream(&["Hi <chips>ask|Say more about it|More</chips> bye"]);
        assert_eq!(shown, "Hi  bye");
        assert_eq!(parse_chip_trailer(&block).len(), 1);
        let (shown, block) = stream(&["Hi\n<chips>\nask|Show the logs|See logs\nop"]);
        assert_eq!(shown, "Hi\n");
        assert_eq!(parse_chip_trailer(&block)[0].label, "See logs");
        let (shown, block) = split_chip_trailer("No chips here.");
        assert_eq!((shown.as_str(), block.as_str()), ("No chips here.", ""));
    }

    #[test]
    fn lines_become_ask_and_page_chips() {
        let chips = parse_chip_trailer(
            "ask|Write a test for the login fix|Test the fix\n\
             open|Settings|Open settings\n\
             ask|Explain why it failed|Why it failed",
        );
        let got: Vec<_> = chips
            .iter()
            .map(|c| (c.id.as_str(), c.kind, c.value.as_str()))
            .collect();
        assert_eq!(
            got,
            vec![
                ("reply-0", ChipKind::Chat, "Write a test for the login fix"),
                ("reply-1", ChipKind::Nav, "__nav:settings"),
                ("reply-2", ChipKind::Chat, "Explain why it failed"),
            ]
        );
        assert!(chips[0].score > chips[1].score && chips[1].score > chips[2].score);
    }

    #[test]
    fn unsafe_or_unknown_lines_are_dropped() {
        let block = "\
            open|bank|Open bank\n\
            run|rm -rf /|Clean up\n\
            ask|Send the report to Sam|Email Sam\n\
            ask|Delete the old branch|Delete branch\n\
            ask|Buy the upgrade now|Upgrade\n\
            ask|use token sk-abcdefghijklmnopqrstuv now|Use token\n\
            ask|/model think|Think harder\n\
            ask|__mode:think|Think\n\
            ask|$ cargo test|Run tests\n\
            ask|Check my Gmail inbox|Inbox\n\
            ask|What should we do next?|What's next\n\
            just a sentence\n\
            ask|Add a retry to the upload|Add retry";
        let chips = parse_chip_trailer(block);
        let labels: Vec<_> = chips.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, vec!["Add retry"]);
    }

    #[test]
    fn at_most_three_and_no_repeats() {
        let chips = parse_chip_trailer(
            "ask|Add a test|Add test\nask|Add a test|Again\nask|Fix the lint|Fix lint\n\
             ask|Profile the loop|Profile it\nask|Ship the patch now|Ship it",
        );
        let labels: Vec<_> = chips.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, vec!["Add test", "Fix lint", "Profile it"]);
    }

    fn row(at: u64, source: &str, picked: bool) -> ChipEvent {
        let shown = vec![
            ShownChip {
                key: format!("{source}{at}"),
                source: source.into(),
                ..Default::default()
            },
            ShownChip {
                key: format!("local{at}"),
                source: "pred".into(),
                slot: 1,
                ..Default::default()
            },
        ];
        let outcome = picked.then(|| ChipOutcome::Picked {
            key: format!("local{at}"),
            slot: 1,
            at,
        });
        ChipEvent {
            at,
            surface: "composer".into(),
            shown,
            outcome,
            ..ChipEvent::default()
        }
    }

    #[test]
    fn losing_reply_chips_take_the_rule_out_for_two_weeks() {
        let mut mem = ChipMemory::default();
        assert!(chip_trailer_on(&mem, 5));
        // Too few showings to judge.
        mem.events = (1..20).map(|i| row(i, "reply", true)).collect();
        assert!(!review_chip_trailer(&mut mem, 100));
        // Forty showings, never picked, while the chip beside them often is.
        mem.events = (1..=40).map(|i| row(i, "reply", i % 2 == 0)).collect();
        assert!(review_chip_trailer(&mut mem, 100));
        assert!(!chip_trailer_on(&mem, 100 + CHIP_TRAILER_RETRY_MS - 1));
        assert!(chip_trailer_on(&mem, 100 + CHIP_TRAILER_RETRY_MS));
        // Back on, it is judged only on what came after: the old losses are gone.
        assert!(!review_chip_trailer(&mut mem, 100 + CHIP_TRAILER_RETRY_MS));
    }

    #[test]
    fn reply_chips_that_get_picked_keep_the_rule() {
        let events = (1..=60)
            .map(|i| {
                let mut e = row(i, "reply", false);
                if i % 3 == 0 {
                    e.outcome = Some(ChipOutcome::Picked {
                        key: format!("reply{i}"),
                        slot: 0,
                        at: i,
                    });
                }
                e
            })
            .collect();
        let mut mem = ChipMemory {
            events,
            ..ChipMemory::default()
        };
        assert!(!review_chip_trailer(&mut mem, 100));
        assert!(chip_trailer_on(&mem, 100));
    }

    #[test]
    fn the_rule_stays_short_and_names_real_pages() {
        // About 120 tokens: it rides in every cached prompt.
        assert!(CHIP_TRAILER_RULE.len() < 700, "{}", CHIP_TRAILER_RULE.len());
        let pages = CHIP_TRAILER_RULE
            .split("one of: ")
            .nth(1)
            .and_then(|s| s.split(')').next())
            .expect("page list");
        for page in pages.split(", ") {
            assert!(
                nav_from_chip_value(&format!("__nav:{page}")).is_some(),
                "{page}"
            );
        }
    }
}
