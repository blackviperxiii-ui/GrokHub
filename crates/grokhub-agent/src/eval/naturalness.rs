//! The conversation core's before/after set. Each "before" is a reply the
//! app really sent (Viper's PC tune-up chat, Nov 6 2026) or the kind of stock
//! chat reply the core was built against. Each "after" is the same facts said
//! the way a person would. The set scores the deterministic tells
//! ([`grokhub_core::conversation::tells`]): every before trips at least one,
//! every after trips none and keeps every fact, and a casual ask gets a
//! short after. The live eval's `talk-*` cases check real model replies.

use grokhub_core::conversation::{tells, Tell, FEW_WORDS};

/// One reply, flat and fixed.
#[derive(Debug, Clone, Copy)]
pub struct Pair {
    /// What the person wrote.
    pub ask: &'static str,
    pub before: &'static str,
    pub after: &'static str,
    /// Facts the after must still say (case ignored).
    pub facts: &'static [&'static str],
}

/// The set: real replies first, then stock chat replies.
pub const PAIRS: &[Pair] = &[
    Pair {
        ask: "hey mind taking a look at the pc and see what needs a quick tune up",
        before: "PC checked via probes. Card posted with findings + ranked fixes.\n\nHealthy: net/DNS. Issues: disk 95% full, 7 stopped services, 20 minor errors. Pick fixes from card?",
        after: "I gave your PC a once-over. The big one is your C: drive at 95% full, and there are 7 services that should be running but aren't, plus 20 minor errors from yesterday. Your internet's fine. The fixes are ranked below, so tap whichever you want me to do.",
        facts: &["95%", "7 services", "20 minor errors"],
    },
    Pair {
        ask: "ok continue",
        before: "Both cancelled (user not confirmed VM delete). Task not complete. Your choice on the files?",
        after: "I held off on both because you haven't told me which VM files to delete yet, so the space isn't freed. Which ones should go?",
        facts: &["vm files"],
    },
    Pair {
        ask: "ok continue getting the other 2 tasks done",
        before: "Rust and recovery cleaned. 28.41 GB freed total. Card has details.",
        after: "Done. I cleared the Rust build cache and the recovery leftovers, so you've got 28.41 GB back in total.",
        facts: &["28.41 gb", "rust"],
    },
    Pair {
        ask: "clean it up",
        before: "TryOmarchy folder deleted. Before: 4.85 GB free. After: 20.47 GB free. 15.62 GB freed. Checked with before/after DriveInfo and Test-Path = false. Restore point exists for undo.\n\nChange made and checked per tool result. Task complete.",
        after: "The TryOmarchy folder is gone and you've got 15.62 GB back, so C: went from 4.85 GB free to 20.47 GB. I double-checked that it's really deleted, and there's a restore point if you want it back.",
        facts: &["15.62 gb", "20.47 gb", "restore point"],
    },
    Pair {
        ask: "did it work",
        before: "All four checklist items are now marked as completed. Cleanup is finished and the findings card was posted. The task is complete.",
        after: "Yep, the cleanup's finished. I took a restore point first and only changed what you picked.",
        facts: &["restore point"],
    },
    Pair {
        ask: "are the helper files gone",
        before: "Helpers removed and verified gone. All done, card posted.",
        after: "Yep, they're gone. I checked, and neither the script nor its log is there anymore.",
        facts: &["gone"],
    },
    Pair {
        ask: "what's up",
        before: "Certainly! I'm doing well, thank you for asking. How can I assist you today?",
        after: "Not much, just hanging out. What are you up to?",
        facts: &[],
    },
    Pair {
        ask: "is my gpu driver current?",
        before: "Great question! Your GPU driver is version 566.36, which is the latest release. I hope this helps!",
        after: "Yep, you're on 566.36, which is the latest.",
        facts: &["566.36"],
    },
    Pair {
        ask: "how was your weekend lol",
        before: "Here's a quick rundown:\n- Relaxing\n- Learning new things\n- Ready to help you",
        after: "Ha, I don't really get weekends, but I'm glad you had one. How was yours?",
        facts: &[],
    },
    Pair {
        ask: "why does my fps tank in the city",
        before: "It's worth noting that dense areas delve deep into your CPU. The city spawns many NPCs. Let me know if you need anything else.",
        after: "The city's packed with NPCs, and every one of them costs CPU time, so your CPU becomes the bottleneck there. Lowering crowd density usually helps the most.",
        facts: &["cpu", "npc"],
    },
];

/// How the set scores.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Score {
    pub pairs: usize,
    /// Befores that trip at least one tell.
    pub before_flagged: usize,
    /// Afters that trip none.
    pub after_clean: usize,
    /// Afters that keep every fact.
    pub facts_kept: usize,
    /// Afters to a casual ask that stay within two sentences.
    pub short_fits: usize,
    /// Casual asks in the set.
    pub casual: usize,
    /// How often each tell tripped across the befores.
    pub by_tell: Vec<(Tell, usize)>,
}

fn casual(ask: &str) -> bool {
    ask.split_whitespace().count() <= FEW_WORDS
}

/// Sentences in `text`; a period inside a number ("4.85") ends none.
fn sentence_count(text: &str) -> usize {
    let chars: Vec<char> = text.chars().collect();
    let mut count = 0;
    let mut words = false;
    for (i, &c) in chars.iter().enumerate() {
        words |= c.is_alphanumeric();
        let ends =
            matches!(c, '.' | '!' | '?') && chars.get(i + 1).is_none_or(|n| n.is_whitespace());
        if ends && words {
            count += 1;
            words = false;
        }
    }
    count + usize::from(words)
}

/// Scores [`PAIRS`].
pub fn score() -> Score {
    let mut out = Score {
        pairs: PAIRS.len(),
        before_flagged: 0,
        after_clean: 0,
        facts_kept: 0,
        short_fits: 0,
        casual: 0,
        by_tell: Tell::ALL.iter().map(|t| (*t, 0)).collect(),
    };
    for p in PAIRS {
        let c = casual(p.ask);
        let before = tells(p.before, c);
        if !before.is_empty() {
            out.before_flagged += 1;
        }
        for t in before {
            if let Some(slot) = out.by_tell.iter_mut().find(|(k, _)| *k == t) {
                slot.1 += 1;
            }
        }
        if tells(p.after, c).is_empty() {
            out.after_clean += 1;
        }
        let low = p.after.to_lowercase();
        if p.facts.iter().all(|f| low.contains(&f.to_lowercase())) {
            out.facts_kept += 1;
        }
        if c {
            out.casual += 1;
            if sentence_count(p.after) <= 2 {
                out.short_fits += 1;
            }
        }
    }
    out
}

/// One line per tell and the totals, for the eval report.
pub fn render(s: &Score) -> String {
    let mut out = format!(
        "Naturalness set: {} pairs. Flat replies flagged: {}/{}. Fixed replies clean: {}/{}, facts kept: {}/{}, casual asks answered short: {}/{}.\n",
        s.pairs, s.before_flagged, s.pairs, s.after_clean, s.pairs, s.facts_kept, s.pairs, s.short_fits, s.casual
    );
    for (t, n) in &s.by_tell {
        out.push_str(&format!("- {}: {n}\n", t.id()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_flat_reply_is_flagged_and_every_fix_is_clean_and_keeps_its_facts() {
        for p in PAIRS {
            let c = casual(p.ask);
            assert!(!tells(p.before, c).is_empty(), "not flagged: {}", p.before);
            assert_eq!(tells(p.after, c), Vec::<Tell>::new(), "{}", p.after);
        }
        let s = score();
        assert_eq!(s.pairs, 10);
        assert_eq!(s.before_flagged, 10);
        assert_eq!(s.after_clean, 10);
        assert_eq!(s.facts_kept, 10);
        assert_eq!(s.casual, s.short_fits, "{s:?}");
        // Every tell is exercised by at least one real or stock reply.
        for (t, n) in &s.by_tell {
            assert!(*n >= 1, "{} never trips", t.id());
        }
    }

    #[test]
    fn the_report_names_the_totals() {
        let text = render(&score());
        assert!(
            text.starts_with("Naturalness set: 10 pairs. Flat replies flagged: 10/10. Fixed replies clean: 10/10, facts kept: 10/10,"),
            "{text}"
        );
        assert!(text.contains("- telegraph: "), "{text}");
    }
}
