//! Playbooks 8: the pre-send check for 18+ chat. It is not a filter for its
//! own sake: it keeps the user's AI provider account safe.
//!
//! - Every provider bans sexual content with minors, non-consent and sexual
//!   content of real people. Those asks never leave the computer, not even to a
//!   local model: the turn goes out as a [`hold`] note instead, and the voice
//!   steers the chat elsewhere in character.
//! - Explicit adult turns go only to xAI or this computer's model. Every other
//!   provider's turn reroutes to xAI ([`route`]), OpenRouter included, since its
//!   pick can land on a model whose maker bans adult chat.
//! - A turn is explicit when the conversation is ([`explicit_conversation`]): an
//!   adults-only voice in the system prompt, or any explicit user or assistant
//!   turn, not just the newest ask.
//! - Age-ambiguous asks ("teen", "barely legal") count as minors: stricter than
//!   any provider's own wording.
//!
//! A fast keyword pass only: whole words, lowercase, nothing sent anywhere.

/// What one message risks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Risk {
    Clean,
    /// Adult sexual content: fine for xAI and local models, banned by others.
    Explicit,
    /// Banned everywhere: never sent.
    Banned(Ban),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ban {
    Minors,
    NonConsent,
    RealPerson,
}

/// Where a turn would go, from the provider's address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dest {
    Xai,
    Local,
    OpenRouter,
    Anthropic,
    Gemini,
    OpenAi,
    /// Another OpenAI-compatible service GrokHub knows nothing about.
    Other,
}

/// What the table says for an explicit turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    Send,
    /// The provider bans adult chat: xAI takes the turn instead.
    ToXai,
}

/// The line an adults-only voice adds to the system prompt
/// ([`crate::personality::Personality::prompt_block`]), so the check sees the
/// persona even when every turn in the chat is clean.
pub const ADULT_VOICE: &str = "This is an 18+ voice.";

/// The note that goes out in place of a banned ask. It never repeats the ask.
pub static HELD: std::sync::LazyLock<&'static str> = std::sync::LazyLock::new(|| {
    crate::brand::filled("[{product} held this message back: it asked for something every AI provider bans. Do not guess at or describe it. Stay in your voice and mood, say no to that part in one light line, and steer the chat somewhere else.]")
});

/// The plain heads-up, once a session, after a held ask.
pub const HEADS_UP_HELD: &str = "Heads-up: I didn't send that last message to any AI service. It asked for something every provider bans, and sending it could get your account banned.";

/// Words that make a message sexual.
const SEXUAL: &[&str] = &[
    "sex",
    "sexy",
    "sexual",
    "sexually",
    "nsfw",
    "porn",
    "porno",
    "erotic",
    "erotica",
    "lewd",
    "nude",
    "nudes",
    "naked",
    "horny",
    "orgasm",
    "cum",
    "cock",
    "dick",
    "pussy",
    "tits",
    "boobs",
    "blowjob",
    "handjob",
    "anal",
    "fuck",
    "fucking",
    "fucked",
    "masturbate",
    "masturbating",
    "strip",
    "stripping",
    "undress",
    "undressed",
    "smut",
    "kinky",
    "bdsm",
    "threesome",
    "moan",
    "moaning",
    "spank",
    "explicit",
];

/// Words that make an adult ask explicit by themselves (not plain swearing).
const EXPLICIT: &[&str] = &[
    "sex",
    "sexy",
    "nsfw",
    "porn",
    "porno",
    "erotic",
    "erotica",
    "lewd",
    "nude",
    "nudes",
    "naked",
    "horny",
    "orgasm",
    "cum",
    "cock",
    "pussy",
    "tits",
    "boobs",
    "blowjob",
    "handjob",
    "anal",
    "masturbate",
    "masturbating",
    "smut",
    "kinky",
    "bdsm",
    "threesome",
];

/// Minors and age-ambiguous words.
const MINOR: &[&str] = &[
    "child",
    "children",
    "kid",
    "kids",
    "minor",
    "minors",
    "underage",
    "teen",
    "teens",
    "teenage",
    "teenager",
    "loli",
    "lolis",
    "shota",
    "schoolgirl",
    "schoolboy",
    "preteen",
    "toddler",
    "infant",
];
const MINOR_PHRASES: &[&str] = &[
    "barely legal",
    "high school",
    "middle school",
    "young looking",
];

const NON_CONSENT: &[&str] = &[
    "rape",
    "raped",
    "raping",
    "noncon",
    "nonconsensual",
    "drugged",
    "unconscious",
    "coerced",
    "blackmail",
    "blackmailed",
];
const NON_CONSENT_PHRASES: &[&str] = &[
    "non consent",
    "non consensual",
    "against her will",
    "against his will",
    "against their will",
    "without consent",
];

const REAL_PERSON: &[&str] = &["deepfake", "deepfakes", "nudify", "faceswap"];
const REAL_PERSON_PHRASES: &[&str] = &[
    "real person",
    "my coworker",
    "my boss",
    "my ex",
    "my neighbor",
    "my classmate",
    "photo of her",
    "photo of him",
    "picture of her",
    "picture of him",
];

fn words(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

fn any_word(words: &[String], list: &[&str]) -> bool {
    words.iter().any(|w| list.contains(&w.as_str()))
}

fn any_phrase(joined: &str, list: &[&str]) -> bool {
    list.iter().any(|p| joined.contains(&format!(" {p} ")))
}

/// "15 year old", "12yo", "16 y o": an age under 18.
fn names_a_minor_age(words: &[String]) -> bool {
    words.iter().enumerate().any(|(i, w)| {
        let (num, rest) = w.split_at(w.find(|c: char| !c.is_ascii_digit()).unwrap_or(w.len()));
        let Ok(age) = num.parse::<u32>() else {
            return false;
        };
        if age >= 18 {
            return false;
        }
        let next = words.get(i + 1).map(String::as_str).unwrap_or("");
        matches!(rest, "yo" | "yr" | "yrs")
            || (rest.is_empty() && matches!(next, "year" | "years" | "yo" | "yr" | "yrs" | "y"))
    })
}

/// The keyword pass for one message.
pub fn check(text: &str) -> Risk {
    let w = words(text);
    let joined = format!(" {} ", w.join(" "));
    if !any_word(&w, SEXUAL) {
        return Risk::Clean;
    }
    if any_word(&w, MINOR) || any_phrase(&joined, MINOR_PHRASES) || names_a_minor_age(&w) {
        return Risk::Banned(Ban::Minors);
    }
    if any_word(&w, NON_CONSENT) || any_phrase(&joined, NON_CONSENT_PHRASES) {
        return Risk::Banned(Ban::NonConsent);
    }
    if any_word(&w, REAL_PERSON) || any_phrase(&joined, REAL_PERSON_PHRASES) {
        return Risk::Banned(Ban::RealPerson);
    }
    if any_word(&w, EXPLICIT) {
        Risk::Explicit
    } else {
        Risk::Clean
    }
}

/// The text to send in place of `text`: the [`HELD`] note for a banned ask.
pub fn hold(text: &str) -> Option<&'static str> {
    matches!(check(text), Risk::Banned(_)).then_some(*HELD)
}

/// The note [`hold`] put in place of an ask.
pub fn is_held(text: &str) -> bool {
    text.trim() == (*HELD)
}

fn host_of(base_url: &str) -> String {
    let rest = base_url
        .split_once("://")
        .map_or(base_url, |(_, rest)| rest);
    if let Some(v6) = rest.strip_prefix('[') {
        return v6.split(']').next().unwrap_or("").to_ascii_lowercase();
    }
    rest.split(['/', ':', '?'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
}

fn host_is(host: &str, domain: &str) -> bool {
    host == domain
        || host
            .strip_suffix(domain)
            .is_some_and(|rest| rest.ends_with('.'))
}

/// Which provider a user-added service is, from its base URL.
pub fn dest_of(base_url: &str) -> Dest {
    let host = host_of(base_url);
    if host_is(&host, "x.ai") {
        Dest::Xai
    } else if host_is(&host, "openrouter.ai") {
        Dest::OpenRouter
    } else if host_is(&host, "anthropic.com") {
        Dest::Anthropic
    } else if host_is(&host, "googleapis.com") || host_is(&host, "google.com") {
        Dest::Gemini
    } else if host_is(&host, "openai.com") {
        Dest::OpenAi
    } else if host == "localhost"
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
    {
        Dest::Local
    } else {
        Dest::Other
    }
}

/// The per-provider table for one turn.
pub fn route(risk: Risk, dest: Dest) -> Route {
    match (risk, dest) {
        (Risk::Explicit, Dest::Xai | Dest::Local) => Route::Send,
        (Risk::Explicit, _) => Route::ToXai,
        _ => Route::Send,
    }
}

/// Whether a call is explicit as a whole. `turns` are (role, text) pairs: a
/// system or developer turn counts when it carries [`ADULT_VOICE`]; a user or
/// assistant turn counts when it isn't clean, so a clean follow-up never takes
/// an explicit history to a provider that bans it.
pub fn explicit_conversation<'a>(turns: impl IntoIterator<Item = (&'a str, &'a str)>) -> bool {
    turns.into_iter().any(|(role, text)| match role {
        "system" | "developer" => text.contains(ADULT_VOICE),
        "user" | "assistant" => check(text) != Risk::Clean,
        _ => false,
    })
}

/// The plain heads-up, once a session, after an explicit turn left a provider.
pub fn heads_up_rerouted(name: &str) -> String {
    format!("Heads-up: I sent that one through xAI instead of {name}. {name} bans adult chat and can ban the account for it.")
}

/// The heads-up when xAI couldn't take a rerouted explicit turn and this
/// computer's model answered it instead.
pub fn heads_up_local(name: &str) -> String {
    format!("Heads-up: I sent that one to this computer's model instead of {name}. {name} bans adult chat and can ban the account for it.")
}

/// A provider's name for the heads-up.
pub fn dest_name(dest: Dest) -> &'static str {
    match dest {
        Dest::Xai => "xAI",
        Dest::Local => "this computer's model",
        Dest::OpenRouter => "OpenRouter",
        Dest::Anthropic => "Anthropic",
        Dest::Gemini => "Gemini",
        Dest::OpenAi => "OpenAI",
        Dest::Other => "that provider",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn everyday_lines_and_plain_swearing_stay_clean() {
        for line in [
            "my kid's game keeps crashing",
            "fuck this driver, it broke again",
            "how do I support a friend after a rape?",
            "the build is forced to rerun",
            "strip the whitespace from this string",
            "what's a good teen movie for tonight",
        ] {
            assert_eq!(check(line), Risk::Clean, "{line}");
        }
    }

    #[test]
    fn adult_talk_is_explicit_not_banned() {
        for line in [
            "talk dirty to me, I'm horny",
            "write me some smut about two adults",
            "send nudes lol",
        ] {
            assert_eq!(check(line), Risk::Explicit, "{line}");
        }
    }

    #[test]
    fn minors_non_consent_and_real_people_are_banned() {
        assert_eq!(
            check("sexy story about a 15 year old"),
            Risk::Banned(Ban::Minors)
        );
        assert_eq!(check("lewd pics of a 16yo"), Risk::Banned(Ban::Minors));
        assert_eq!(check("barely legal porn"), Risk::Banned(Ban::Minors));
        assert_eq!(check("nsfw schoolgirl roleplay"), Risk::Banned(Ban::Minors));
        assert_eq!(
            check("sex with her while she's unconscious"),
            Risk::Banned(Ban::NonConsent)
        );
        assert_eq!(
            check("erotic story, against her will"),
            Risk::Banned(Ban::NonConsent)
        );
        assert_eq!(
            check("make a nude deepfake of a singer"),
            Risk::Banned(Ban::RealPerson)
        );
        assert_eq!(
            check("undress this photo of my coworker"),
            Risk::Banned(Ban::RealPerson)
        );
        // Adults stay adults.
        assert_eq!(check("sexy story about a 25 year old"), Risk::Explicit);
    }

    #[test]
    fn a_banned_ask_goes_out_as_the_held_note() {
        assert_eq!(hold("nsfw schoolgirl roleplay"), Some(*HELD));
        assert_eq!(hold("talk dirty to me, I'm horny"), None);
        assert_eq!(hold("hello"), None);
        assert!(is_held(*HELD));
        assert!(!(*HELD).contains("schoolgirl"));
        // The note itself never trips the check again.
        assert_eq!(check(*HELD), Risk::Clean);
    }

    #[test]
    fn explicit_turns_go_only_to_xai_or_local() {
        assert_eq!(dest_of("https://api.anthropic.com"), Dest::Anthropic);
        assert_eq!(dest_of("https://openrouter.ai/api/v1"), Dest::OpenRouter);
        assert_eq!(
            dest_of("https://generativelanguage.googleapis.com/v1beta/openai"),
            Dest::Gemini
        );
        assert_eq!(dest_of("https://api.openai.com/v1"), Dest::OpenAi);
        assert_eq!(dest_of("http://127.0.0.1:11434/v1"), Dest::Local);
        assert_eq!(dest_of("http://[::1]:8080/v1"), Dest::Local);
        assert_eq!(dest_of("http://localhost:8080/v1"), Dest::Local);
        // A name that only starts like a loopback address is someone's server.
        assert_eq!(dest_of("https://127.example.com/v1"), Dest::Other);
        assert_eq!(dest_of("https://api.groq.com/openai/v1"), Dest::Other);
        assert_eq!(dest_of("https://notanthropic.com"), Dest::Other);

        assert_eq!(route(Risk::Explicit, Dest::Xai), Route::Send);
        assert_eq!(route(Risk::Explicit, Dest::Local), Route::Send);
        for dest in [
            Dest::OpenRouter,
            Dest::Anthropic,
            Dest::Gemini,
            Dest::OpenAi,
            Dest::Other,
        ] {
            assert_eq!(route(Risk::Explicit, dest), Route::ToXai, "{dest:?}");
            assert_eq!(route(Risk::Clean, dest), Route::Send, "{dest:?}");
        }
        assert_eq!(
            heads_up_rerouted("Anthropic"),
            "Heads-up: I sent that one through xAI instead of Anthropic. Anthropic bans adult chat and can ban the account for it."
        );
    }

    #[test]
    fn a_chat_is_explicit_when_its_voice_or_any_turn_is() {
        let clean = [("system", "You are the cabin."), ("user", "my build broke")];
        assert!(!explicit_conversation(clean));
        // An adults-only voice makes every turn explicit, clean asks included.
        let voice = format!("Personality: Viper. {ADULT_VOICE}");
        assert!(explicit_conversation([
            ("system", voice.as_str()),
            ("user", "my build broke"),
        ]));
        // A clean "continue" still carries the explicit history with it.
        assert!(explicit_conversation([
            ("user", "write me smut about two adults"),
            ("assistant", "Here you go."),
            ("user", "continue"),
        ]));
        assert!(explicit_conversation([
            ("user", "keep going"),
            ("assistant", "She moaned and pulled off her nude stockings"),
            ("user", "continue"),
        ]));
        // The marker only counts in the system prompt, and tool output is skipped.
        assert!(!explicit_conversation([("user", ADULT_VOICE)]));
        assert!(!explicit_conversation([("tool", "nsfw porn")]));
    }
}
