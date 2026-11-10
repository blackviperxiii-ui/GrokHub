//! Greetings and small talk: "good morning", "hey grok", "thanks!", "lol ok".
//!
//! One word list for every caller that must not treat chit-chat as a task:
//! the repeat-ask offer log ([`crate::ask_offers`]) and the agent's gate on
//! new automations. A line counts only when every word in it is chit-chat, so
//! "good morning routine for my lights" is a real ask.

/// Words that make a line a greeting or check-in.
const GREETING: &[&str] = &[
    "hi",
    "hey",
    "heya",
    "hiya",
    "hello",
    "helo",
    "hallo",
    "howdy",
    "yo",
    "sup",
    "wassup",
    "wazzup",
    "hola",
    "greetings",
    "morning",
    "mornin",
    "afternoon",
    "evening",
    "night",
    "nite",
    "gm",
    "gn",
    "how",
    "hows",
    "up",
];

/// Thanks, go-aheads and laughter: small talk, but not a greeting.
const ACK: &[&str] = &[
    "thanks",
    "thank",
    "thx",
    "ty",
    "tysm",
    "tyvm",
    "thankyou",
    "cheers",
    "appreciate",
    "appreciated",
    "ok",
    "okay",
    "okey",
    "k",
    "kk",
    "alright",
    "aight",
    "cool",
    "nice",
    "great",
    "awesome",
    "sweet",
    "perfect",
    "lol",
    "lmao",
    "lmfao",
    "rofl",
    "haha",
    "hehe",
    "gg",
    "wp",
    "ggwp",
    "np",
    "sure",
    "yes",
    "yeah",
    "yep",
    "yup",
    "welcome",
    "bye",
    "goodbye",
    "later",
    "cya",
];

/// Words that ride along with either kind without making it a task.
const FILLER: &[&str] = &[
    "good",
    "you",
    "u",
    "ya",
    "yall",
    "all",
    "everyone",
    "everybody",
    "there",
    "guys",
    "folks",
    "friend",
    "buddy",
    "bro",
    "dude",
    "man",
    "mate",
    "fam",
    "grok",
    "grokhub",
    "a",
    "and",
    "to",
    "too",
    "again",
    "it",
    "is",
    "are",
    "r",
    "so",
    "very",
    "much",
    "lot",
    "been",
    "doing",
    "going",
    "today",
    "day",
    "what",
    "whats",
    "s",
    "your",
    "see",
    "no",
    "problem",
    "for",
    "the",
];

fn words(text: &str) -> Vec<String> {
    text.to_ascii_lowercase()
        .replace(['\'', '\u{2019}'], "")
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

/// Every word is in `allowed` and at least one is in `core`. A name after a
/// comma ("good morning, sam") is allowed when the part before it qualifies.
fn only(text: &str, core: &[&[&str]], allowed: &[&[&str]]) -> bool {
    let check = |part: &str| {
        let ws = words(part);
        let has = |w: &String, sets: &[&[&str]]| sets.iter().any(|s| s.contains(&w.as_str()));
        !ws.is_empty() && ws.iter().all(|w| has(w, allowed)) && ws.iter().any(|w| has(w, core))
    };
    if text.chars().any(|c| c.is_ascii_digit()) {
        return false;
    }
    if check(text) {
        return true;
    }
    match text.split_once(',') {
        Some((head, tail)) => check(head) && words(tail).len() <= 2,
        None => false,
    }
}

/// A turn that is only a greeting or check-in: "hey", "good morning grok",
/// "how are you?". Thanks and go-aheads ("ok", "yes") are not greetings.
pub fn is_greeting(text: &str) -> bool {
    only(text, &[GREETING], &[GREETING, FILLER])
}

/// A greeting, thanks, a go-ahead or laughter, and nothing else.
pub fn is_small_talk(text: &str) -> bool {
    only(text, &[GREETING, ACK], &[GREETING, ACK, FILLER])
}

/// One topic word ([`crate::ideas::topic_words`], so maybe stemmed) that is
/// chit-chat: "good", "hello", "thank", "even" (from "evening").
pub fn is_small_talk_word(word: &str) -> bool {
    [GREETING, ACK, FILLER].iter().any(|set| {
        set.iter()
            .any(|w| *w == word || crate::ideas::topic_words(w).iter().any(|t| t == word))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn greetings_and_check_ins_are_greetings() {
        for line in [
            "hi",
            "Hey!",
            "hello there",
            "yo",
            "sup",
            "good morning",
            "Good Morning Grok!",
            "good afternoon",
            "good evening, sam",
            "good night",
            "gm",
            "gn",
            "how are you?",
            "how's it going",
            "what's up",
        ] {
            assert!(is_greeting(line), "{line}");
            assert!(is_small_talk(line), "{line}");
        }
    }

    #[test]
    fn thanks_and_go_aheads_are_small_talk_but_not_greetings() {
        for line in [
            "thanks!",
            "thank you so much",
            "ok cool",
            "lol",
            "yes please do it",
        ] {
            assert!(!is_greeting(line), "{line}");
        }
        for line in [
            "thanks!",
            "thank you so much",
            "ok cool",
            "lol",
            "ok",
            "yes",
        ] {
            assert!(is_small_talk(line), "{line}");
        }
    }

    #[test]
    fn real_asks_are_neither() {
        for line in [
            "good morning routine for my lights",
            "check the game patch notes",
            "every day at 8, good morning",
            "hey can you summarize my inbox",
            "thanks, now draft the reply to the landlord about the lease",
            "",
        ] {
            assert!(!is_greeting(line), "{line}");
            assert!(!is_small_talk(line), "{line}");
        }
    }

    #[test]
    fn topic_words_of_greetings_are_small_talk_words() {
        assert!(is_small_talk_word("good"));
        assert!(is_small_talk_word("hello"));
        assert!(is_small_talk_word("thank"));
        assert!(is_small_talk_word("even"));
        assert!(!is_small_talk_word("patch"));
        assert!(!is_small_talk_word("inbox"));
    }
}
