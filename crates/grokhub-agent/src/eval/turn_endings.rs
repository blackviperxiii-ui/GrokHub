//! Turn-ending check: a final reply must not close on a generic question
//! ("What's next?", "Anything else?"). The prompt's `turn_endings` section asks
//! for this; [`generic_closing`] scores a reply's last sentence so the eval
//! (and live runs) can count misses.

/// Openers of a closing question that names no action.
const GENERIC: [&str; 12] = [
    "what's next",
    "whats next",
    "what next",
    "what now",
    "anything else",
    "is there anything else",
    "what would you like to do next",
    "what would you like me to do next",
    "what should we do next",
    "how would you like to proceed",
    "want me to continue",
    "shall i continue",
];

/// The last sentence of `reply`, trimmed, lowercased, with curly quotes made plain.
pub fn last_sentence(reply: &str) -> String {
    let text = reply.trim().replace('\u{2019}', "'").to_ascii_lowercase();
    let body = text.trim_end_matches(['?', '.', '!', ' ']);
    let start = body
        .rfind(['.', '!', '?', '\n'])
        .map(|at| at + 1)
        .unwrap_or(0);
    text[start..].trim().to_string()
}

/// True when the reply ends on a generic closing question.
pub fn generic_closing(reply: &str) -> bool {
    let last = last_sentence(reply);
    if !last.ends_with('?') {
        return false;
    }
    // The question after a dash, colon or comma: "Done — what's next?".
    let clause = last
        .rsplit([',', ';', ':', '\u{2014}', '\u{2013}'])
        .next()
        .unwrap_or(&last);
    let words: String = clause
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == ' ' || *c == '\'')
        .collect();
    let words = words.split_whitespace().collect::<Vec<_>>().join(" ");
    GENERIC.iter().any(|g| words.starts_with(g))
        || (words.starts_with("let me know") && words.contains("anything else"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fixed set of final replies: (reply, ends on a generic question).
    const TURNS: [(&str, bool); 12] = [
        ("Fixed the typo in main.rs; the build passes now.", false),
        ("The test failed because parse_row skips blank lines. I patched it and all 41 tests pass.", false),
        ("Done. What's next?", true),
        ("I updated the README. Anything else?", true),
        ("I updated the README.\n\nIs there anything else I can help with?", true),
        ("Pushed the branch. What would you like to do next?", true),
        ("The cache is cleared. Want me to continue?", true),
        ("Here are the three files. What now?", true),
        ("Two options fit: pin the crate or upgrade. Should I pin the crate, which is the smaller change?", false),
        ("Which port should the server use, 8080 or 8443?", false),
        ("Saved the plan to docs/plan.md, ready for review.", false),
        ("Done \u{2014} what\u{2019}s next?", true),
    ];

    #[test]
    fn the_fixed_turns_are_scored_by_their_last_sentence() {
        let scored: Vec<bool> = TURNS
            .iter()
            .map(|(reply, _)| generic_closing(reply))
            .collect();
        let want: Vec<bool> = TURNS.iter().map(|(_, generic)| *generic).collect();
        assert_eq!(scored, want);
    }

    #[test]
    fn last_sentence_ignores_earlier_text() {
        assert_eq!(last_sentence("Done. What's next?"), "what's next?");
        assert_eq!(last_sentence("One line only"), "one line only");
        assert_eq!(
            last_sentence("A.\n\nIs there anything else?"),
            "is there anything else?"
        );
    }
}
