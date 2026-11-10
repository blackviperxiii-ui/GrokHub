//! A typed crash ask that names its program ("why does bluetoothd keep
//! crashing", "Discord.exe crashed", "what's wrong with steam.exe") reads that
//! program's own crash history, not only the background services.
//!
//! The name is the user's own word, cut to a [`ProgramName`]: anything with a
//! shell character fails the parse and the ask names no program. It reaches a
//! probe only as one argv word.

use super::{names_code, Os, ProbeId, ProgramName};

/// Words that end a crash ask: the program is the word before them.
const CRASH: &[&str] = &[
    "crash",
    "crashed",
    "crashes",
    "crashing",
    "segfault",
    "segfaults",
    "segfaulted",
    "segfaulting",
];
/// "keeps dying" names a crash only with a keep in front of it.
const DYING: &[&str] = &["dying", "die", "dies", "died"];
/// Words between the program and the crash word: "X keeps crashing", "the X
/// service crashed".
const FILLER: &[&str] = &[
    "service",
    "daemon",
    "process",
    "app",
    "program",
    "client",
    "keep",
    "keeps",
    "kept",
    "is",
    "was",
    "does",
    "did",
    "has",
    "had",
    "been",
    "just",
    "always",
    "still",
    "randomly",
    "constantly",
    "suddenly",
    "again",
    "also",
    "now",
    "often",
    "sometimes",
    "really",
    "never",
    "not",
    "to",
    "start",
    "starts",
    "started",
    "begins",
    "began",
    "tends",
    "seems",
    "will",
    "would",
    "might",
    "can",
    "could",
    "stop",
    "won't",
    "wont",
    "doesn't",
    "didn't",
];
/// Words that sit where a name would but are not a program.
const NOT_A_NAME: &[&str] = &[
    "it",
    "this",
    "that",
    "they",
    "he",
    "she",
    "you",
    "i",
    "me",
    "my",
    "the",
    "a",
    "an",
    "why",
    "what",
    "something",
    "everything",
    "thing",
    "stuff",
    "computer",
    "pc",
    "laptop",
    "machine",
    "system",
    "app",
    "apps",
    "game",
    "games",
    "program",
    "programs",
    "browser",
    "phone",
    "screen",
    "driver",
    "bluetooth",
    "wifi",
    "wi-fi",
    "internet",
    "network",
    "sound",
    "audio",
    "printer",
    "mouse",
    "keyboard",
    "build",
    "test",
    "tests",
    "server",
    "car",
    "plane",
    "market",
    "stock",
    "and",
    "or",
    "so",
    "of",
];

/// The program a crash ask names, if it names one that can be a process name.
pub fn program_in_ask(text: &str) -> Option<ProgramName> {
    let trimmed = text.trim().replace('\u{2019}', "'");
    let lower = trimmed.to_lowercase();
    if lower.is_empty() || lower.len() > 160 || lower.starts_with('/') || names_code(&lower) {
        return None;
    }
    // Split on spaces only, so "foo;rm" stays one word and fails the parse.
    let words: Vec<&str> = trimmed.split_whitespace().collect();
    let bare = |w: &str| -> String {
        w.trim_matches(|c: char| ",.!?;:\"'()[]".contains(c))
            .to_lowercase()
    };
    // A bare "crash" is a noun too ("a car crash"): it needs a question.
    let asked = ["why", "keep", "does", "did", "?"]
        .iter()
        .any(|q| lower.contains(q));
    for (i, w) in words.iter().enumerate() {
        let word = bare(w);
        let crash = CRASH.contains(&word.as_str()) && (word != "crash" || asked);
        let dying = DYING.contains(&word.as_str())
            && i > 0
            && matches!(bare(words[i - 1]).as_str(), "keep" | "keeps");
        if !(crash || dying) {
            continue;
        }
        let mut j = i;
        while j > 0 && FILLER.contains(&bare(words[j - 1]).as_str()) {
            j -= 1;
        }
        if j == 0 {
            continue;
        }
        if let Some(name) = name_word(words[j - 1]) {
            return Some(name);
        }
    }
    // "what's wrong with X.exe": an .exe is a program whatever else is said.
    let at = lower.find("wrong with ").map(|i| i + "wrong with ".len())?;
    let next = trimmed.get(at..)?.split_whitespace().next()?;
    let name = name_word(next)?;
    name.as_str()
        .to_ascii_lowercase()
        .ends_with(".exe")
        .then_some(name)
}

/// One typed word as a program name: outer punctuation off, then the strict
/// parse. A word with a shell character inside it is no name at all.
fn name_word(word: &str) -> Option<ProgramName> {
    let word = word.trim_matches(|c: char| ",!?;:\"'()[]".contains(c));
    let word = word.strip_suffix('.').unwrap_or(word);
    let word = word.strip_suffix("'s").unwrap_or(word);
    if NOT_A_NAME.contains(&word.to_lowercase().as_str()) {
        return None;
    }
    ProgramName::parse(word)
}

/// A crash ask about a named program: its crash history, plus the
/// background services when it looks like one (a name ending in `d` on
/// Linux, or the ask says service or daemon). Beats the generic services
/// reading of [`super::probes_for_intent`].
pub fn probes_for_named(text: &str, os: Os) -> Option<(ProgramName, Vec<ProbeId>)> {
    let name = program_in_ask(text)?;
    let lower = text.to_lowercase();
    let service = lower.contains("service")
        || lower.contains("daemon")
        || (os == Os::Linux && name.as_str().len() > 2 && name.as_str().ends_with('d'));
    let mut probes = ProbeId::for_subject(os).to_vec();
    if service {
        probes.extend_from_slice(if os == Os::Windows {
            &[ProbeId::ServicesStoppedAuto, ProbeId::EventErrors24h]
        } else {
            &[ProbeId::FailedServices, ProbeId::BootErrors]
        });
    }
    Some((name, probes))
}
