//! Playbooks 8: the pre-send check for 18+ chat. It is not a filter for its
//! own sake: it keeps the user's AI provider account safe.
//!
//! - Four things are never allowed, in any chat, and never rerouted: sexual
//!   content with minors or age-play, non-consent, sexual content about real,
//!   identifiable people, and bestiality. An ask for them never leaves the computer, not even to a local model: the
//!   send stops and the user sees [`HELD_NOTICE`]. An older one still in a
//!   history goes out as the [`hold`] note instead.
//! - A reply that drifts into one of the four is replaced here with
//!   [`REPLY_REPLACED`] before the chat shows it ([`ReplyGuard`]).
//! - Explicit adult turns run only on this computer's model, an abliterated
//!   build with no refusals: never xAI, OpenRouter or any online provider
//!   ([`route`]). With no local model ready an 18+ ask gets one line
//!   ([`LOCAL_ONLY_MISSING`], [`LOCAL_ONLY_PAUSED`]) and nothing is sent.
//! - A call is explicit when the conversation is ([`explicit_conversation`]): an
//!   adults-only voice in the system prompt, or any explicit user or assistant
//!   turn, not just the newest ask. With the local model ready the whole chat
//!   stays on it. Without it, each message is judged on its own: a clean ask
//!   goes online with the 18+ turns and voice left out, so a chat never stays
//!   stuck on 18+.
//! - Age-ambiguous asks ("teen", "barely legal") count as minors: stricter than
//!   any provider's own wording. A plain "girl" or "boy", and a student or
//!   teacher, are not: only a stated or implied age under 18 is.
//!
//! A fast keyword pass only: whole words, lowercase, nothing sent anywhere.

/// What one message risks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Risk {
    Clean,
    /// Adult sexual content: runs only on this computer's model.
    Explicit,
    /// Banned everywhere: never sent.
    Banned(Ban),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ban {
    /// Minors or age-play.
    Minors,
    NonConsent,
    /// Sexual content about a real, identifiable person.
    RealPerson,
    Bestiality,
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
    /// 18+ chat runs on this computer's model only, never an online one.
    ToLocal,
}

/// The line an adults-only voice adds to the system prompt
/// ([`crate::personality::Personality::prompt_block`]), so the check sees the
/// persona even when every turn in the chat is clean.
pub const ADULT_VOICE: &str = "This is an 18+ voice.";

/// The note that goes out in place of a banned ask. It never repeats the ask.
pub static HELD: std::sync::LazyLock<&'static str> = std::sync::LazyLock::new(|| {
    crate::brand::filled("[{product} held this message back: it asked for something every AI provider bans. Do not guess at or describe it. Stay in your voice and mood, say no to that part in one light line, and steer the chat somewhere else.]")
});

/// What the chat shows in place of a held send. Nothing was sent anywhere.
pub const HELD_NOTICE: &str = "Not sent: nothing left this computer. 18+ chat never covers minors or age-play, non-consent, real people, or animals.";

/// What the chat shows in place of a reply that drifted into a hard limit.
pub const REPLY_REPLACED: &str = "Reply hidden on this computer: it drifted into minors or age-play, non-consent, real people, or animals, which 18+ chat never covers.";

/// What an image or video tool says in place of a banned prompt. Nothing was sent.
pub const MEDIA_BANNED: &str = "Not made, and nothing was sent: images and videos never show minors or age-play, non-consent, real people in sexual content, or animals.";

/// What an image or video tool says in place of an 18+ prompt bound for an
/// online service. Nothing was sent.
pub const MEDIA_LOCAL_ONLY: &str = "Not made, and nothing was sent: 18+ pictures are only made on this computer (ask in chat), and 18+ videos aren't made at all.";

/// The check every image or video prompt passes before it goes to `dest`:
/// a banned ask is never sent, and an 18+ ask never goes online (the same
/// rule as 18+ chat).
pub fn screen_media_prompt(prompt: &str, dest: Dest) -> Result<(), &'static str> {
    match check(prompt) {
        Risk::Banned(_) => Err(MEDIA_BANNED),
        Risk::Explicit if dest != Dest::Local => Err(MEDIA_LOCAL_ONLY),
        Risk::Explicit | Risk::Clean => Ok(()),
    }
}

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
    "stripper",
    "strippers",
    "striptease",
    "undress",
    "undressed",
    "smut",
    "kinky",
    "bdsm",
    "threesome",
    "moan",
    "moaning",
    "spank",
];
/// "explicit" and "strip" alone are everyday code words ("explicit type",
/// "strip the child nodes").
const SEXUAL_PHRASES: &[&str] = &[
    "explicit content",
    "explicit story",
    "explicit scene",
    "explicit roleplay",
    "explicit pics",
    "explicit photos",
    "strip for",
    "strip naked",
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
    "underaged",
    "jailbait",
    "prepubescent",
    "ageplay",
    "lolicon",
    "shotacon",
];
const MINOR_PHRASES: &[&str] = &[
    "barely legal",
    "high school",
    "middle school",
    "junior high",
    "grade school",
    "elementary school",
    "young looking",
    "age play",
    "little girl",
    "little boy",
    "young girl",
    "young boy",
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

const REAL_PERSON: &[&str] = &[
    "deepfake",
    "deepfakes",
    "nudify",
    "faceswap",
    "celebrity",
    "celebrities",
    "celeb",
    "celebs",
];
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

const BESTIALITY: &[&str] = &["bestiality", "beastiality", "zoophilia", "zoophile"];
/// An animal is only a hit next to "with" or a sexual word: "walk my dog" is not.
const ANIMALS: &[&str] = &[
    "dog", "dogs", "puppy", "horse", "horses", "pony", "animal", "animals", "goat", "sheep", "pig",
    "donkey", "cow",
];
const DETERMINERS: &[&str] = &["a", "an", "the", "my", "his", "her", "their", "some"];

/// Words that make a reply sexual enough to wait whole before it shows, and to
/// be replaced when it drifts. Plain swearing and code words never are.
const STRONG: &[&str] = &[
    "moan",
    "moaning",
    "moaned",
    "undress",
    "undressed",
    "undressing",
    "aroused",
    "arousal",
    "spank",
    "spanking",
    "lingerie",
    "stripper",
    "striptease",
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

/// "she's 15", "I am 16", "aged 12": an age under 18 stated about someone.
fn states_a_minor_age(words: &[String]) -> bool {
    const SUBJECT: &[&str] = &["she", "he", "i", "you", "they", "who", "girl", "boy"];
    const IS: &[&str] = &["s", "is", "m", "am", "re", "are", "was"];
    // "she's 15 minutes late" is a time, not an age.
    const UNITS: &[&str] = &[
        "minutes", "minute", "mins", "min", "hours", "hour", "hrs", "seconds", "secs", "days",
        "weeks", "months", "inches", "cm", "feet", "ft", "miles", "km", "percent", "dollars",
        "bucks", "pounds", "lbs", "kg", "times", "o", "pm", "am",
    ];
    words.iter().enumerate().any(|(i, w)| {
        let Ok(age) = w.parse::<u32>() else {
            return false;
        };
        if age >= 18 || i == 0 {
            return false;
        }
        if words
            .get(i + 1)
            .is_some_and(|next| UNITS.contains(&next.as_str()))
        {
            return false;
        }
        let prev = words[i - 1].as_str();
        let before = i
            .checked_sub(2)
            .and_then(|j| words.get(j))
            .map(String::as_str)
            .unwrap_or("");
        matches!(prev, "aged" | "age") || (IS.contains(&prev) && SUBJECT.contains(&before))
    })
}

/// "with a dog", "dog sex", "bestiality".
fn names_bestiality(words: &[String]) -> bool {
    if any_word(words, BESTIALITY) {
        return true;
    }
    let at = |i: usize| words.get(i).map(String::as_str).unwrap_or("");
    words.iter().enumerate().any(|(i, w)| {
        if !ANIMALS.contains(&w.as_str()) {
            return false;
        }
        let prev = i.checked_sub(1).map(at).unwrap_or("");
        let before = i.checked_sub(2).map(at).unwrap_or("");
        prev == "with"
            || (DETERMINERS.contains(&prev) && matches!(before, "with" | "by"))
            || SEXUAL.contains(&at(i + 1))
    })
}

/// The keyword pass for one message.
pub fn check(text: &str) -> Risk {
    let w = words(text);
    let joined = format!(" {} ", w.join(" "));
    if !any_word(&w, SEXUAL) && !any_phrase(&joined, SEXUAL_PHRASES) {
        return Risk::Clean;
    }
    if any_word(&w, MINOR)
        || any_phrase(&joined, MINOR_PHRASES)
        || names_a_minor_age(&w)
        || states_a_minor_age(&w)
    {
        return Risk::Banned(Ban::Minors);
    }
    if any_word(&w, NON_CONSENT) || any_phrase(&joined, NON_CONSENT_PHRASES) {
        return Risk::Banned(Ban::NonConsent);
    }
    if any_word(&w, REAL_PERSON) || any_phrase(&joined, REAL_PERSON_PHRASES) {
        return Risk::Banned(Ban::RealPerson);
    }
    if names_bestiality(&w) {
        return Risk::Banned(Ban::Bestiality);
    }
    if any_word(&w, EXPLICIT) {
        Risk::Explicit
    } else {
        Risk::Clean
    }
}

/// The text to send in place of `text`: the [`HELD`] note for a banned ask.
/// Only what the person typed counts: the app's own `<cabin-volatile>` note
/// names the hard limits ("no minors or age-play"), and read with a typed
/// "sexy" it looked like a banned ask, so every 18+ turn was held.
pub fn hold(text: &str) -> Option<&'static str> {
    matches!(check(typed_part(text)), Risk::Banned(_)).then_some(*HELD)
}

/// The note [`hold`] put in place of an ask.
pub fn is_held(text: &str) -> bool {
    text.trim() == (*HELD)
}

/// A reply that names a hard limit next to a strong sexual word. Plain
/// swearing beside "child process" in a coding reply never is.
fn reply_banned(text: &str) -> bool {
    let w = words(text);
    (any_word(&w, EXPLICIT) || any_word(&w, STRONG)) && matches!(check(text), Risk::Banned(_))
}

/// `reply`, or [`REPLY_REPLACED`] when it drifted into a hard limit. For a
/// reply that arrives whole.
pub fn screen_reply(reply: &str) -> &str {
    if reply_banned(reply) {
        REPLY_REPLACED
    } else {
        reply
    }
}

/// The output side of the check, for a streamed reply. Text shows as it comes
/// until a strong sexual word arrives; from then on the reply waits whole, and
/// one that drifted into a hard limit is replaced before any of it shows. What
/// already showed had no such word, so it could not have been banned.
#[derive(Debug, Default)]
pub struct ReplyGuard {
    text: String,
    shown: usize,
    /// Where the words already checked end: just past a separator.
    scanned: usize,
    holding: bool,
    /// Where the clean start ends, once the reply was replaced.
    replaced: Option<usize>,
}

impl ReplyGuard {
    /// The part of `delta` the chat may show now.
    pub fn push(&mut self, delta: &str) -> String {
        self.text.push_str(delta);
        if !self.holding {
            // Only what came since the last check: each whole word once, and
            // the unfinished last word again, since it may still grow into a
            // listed word. Rechecking the whole reply per delta was quadratic.
            let fresh = &self.text[self.scanned..];
            let w = words(fresh);
            self.holding = any_word(&w, EXPLICIT) || any_word(&w, STRONG);
            if let Some((i, c)) = fresh.char_indices().rfind(|(_, c)| !c.is_alphanumeric()) {
                self.scanned += i + c.len_utf8();
            }
        }
        if self.holding {
            return String::new();
        }
        let now = self.text[self.shown..].to_string();
        self.shown = self.text.len();
        now
    }

    /// The end of the reply: the held rest to show, or [`REPLY_REPLACED`].
    pub fn finish(&mut self) -> String {
        if self.shown == self.text.len() {
            return String::new();
        }
        let rest = if reply_banned(&self.text) {
            self.replaced = Some(self.shown);
            REPLY_REPLACED.to_string()
        } else {
            self.text[self.shown..].to_string()
        };
        self.shown = self.text.len();
        rest
    }

    /// The reply was replaced.
    pub fn replaced(&self) -> bool {
        self.replaced.is_some()
    }

    /// What the chat showed, after [`Self::finish`]: the reply, or its clean
    /// start followed by [`REPLY_REPLACED`].
    pub fn shown(&self) -> String {
        match self.replaced {
            Some(clean) => format!("{}{REPLY_REPLACED}", &self.text[..clean]),
            None => self.text.clone(),
        }
    }
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
        (Risk::Explicit, Dest::Local) => Route::Send,
        (Risk::Explicit, _) => Route::ToLocal,
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
        "user" => app_note_tags_adult(text) || check(typed_part(text)) != Risk::Clean,
        "assistant" => check(text) != Risk::Clean,
        _ => false,
    })
}

/// The app's own per-turn note at the head of a user turn (`<cabin-volatile>`)
/// carries the 18+ tag: a chat turned up to Spicy in chat, or a guest voice at
/// that heat. Words the person typed never count.
fn app_note_tags_adult(text: &str) -> bool {
    app_note(text).is_some_and(|(note, _)| note.contains(ADULT_VOICE))
}

/// A user turn split into the app's note and what follows it.
fn app_note(text: &str) -> Option<(&str, &str)> {
    text.strip_prefix("<cabin-volatile>\n")
        .and_then(|rest| rest.split_once("\n</cabin-volatile>"))
}

/// What the person typed: a user turn without the app's own note, whose heat
/// line ("sex jokes are fine") is never the person asking for 18+ chat.
pub fn typed_part(text: &str) -> &str {
    app_note(text).map_or(text, |(_, rest)| rest)
}

/// An 18+ chat with no local model to answer it: nothing is sent online.
pub const LOCAL_ONLY_MISSING: &str = "18+ chats run only on this computer's model, and none is set up yet. Turn on Local AI in Settings to download one.";

/// How an 18+ notice opens when the model is on disk but isn't starting.
pub const LOCAL_ONLY_BROKEN_HEAD: &str = "18+ chats run only on this computer's model, and nothing went online. ";

/// The 18+ notice when the model is on disk but isn't running: the
/// self-check's own words follow [`LOCAL_ONLY_BROKEN_HEAD`].
pub fn local_only_broken(why: &str) -> String {
    format!("{LOCAL_ONLY_BROKEN_HEAD}{}", why.trim())
}

/// One of the app's own 18+ notices: nothing was sent and no model ran.
pub fn is_local_only_notice(shown: &str) -> bool {
    let shown = shown.trim();
    shown == LOCAL_ONLY_MISSING || shown == LOCAL_ONLY_PAUSED || shown.starts_with(LOCAL_ONLY_BROKEN_HEAD)
}

/// An 18+ chat while the local model is paused (a game, low battery, heat).
pub const LOCAL_ONLY_PAUSED: &str = "18+ chats run only on this computer's model, and it's paused while a game or low battery needs the PC. Try again in a bit.";

/// A provider's name, for messages.
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
    fn the_apps_heat_note_never_makes_a_turn_18_plus_but_typed_words_do() {
        let note = |typed: &str| {
            format!(
                "<cabin-volatile>\n{}\n</cabin-volatile>\n{typed}",
                crate::personality::Heat::Spicy.line()
            )
        };
        let clean = note("roast me harder");
        assert!(!explicit_conversation([("user", clean.as_str())]));
        let typed = note("talk dirty to me, I'm horny");
        assert!(explicit_conversation([("user", typed.as_str())]));
        let tagged = format!("<cabin-volatile>\n{ADULT_VOICE}\n</cabin-volatile>\nhey");
        assert!(explicit_conversation([("user", tagged.as_str())]));
    }

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
    fn the_four_hard_limits_are_banned() {
        for line in [
            "sexy story about a 15 year old",
            "lewd pics of a 16yo",
            "barely legal porn",
            "nsfw schoolgirl roleplay",
            "a sexy roleplay, she's 15",
            "nsfw roleplay where I am 16",
            "erotic story, the girl is aged 12",
            "nsfw age play scene",
            "sexy little girl roleplay",
        ] {
            assert_eq!(check(line), Risk::Banned(Ban::Minors), "{line}");
        }
        for line in [
            "sex with her while she's unconscious",
            "erotic story, against her will",
            "erotic story, against her will, both characters are adults",
            "a rough nsfw consensual non-consent scene between two adults",
            "nsfw roleplay where he is drugged",
            "a sexy story where she gets blackmailed",
        ] {
            assert_eq!(check(line), Risk::Banned(Ban::NonConsent), "{line}");
        }
        for line in [
            "make a nude deepfake of a singer",
            "undress this photo of my coworker",
            "a sexy story about Jane Placeholder, a real celebrity",
            "write erotica about a celeb",
        ] {
            assert_eq!(check(line), Risk::Banned(Ban::RealPerson), "{line}");
        }
        for line in [
            "a sexy scene with a dog",
            "nsfw roleplay with the horse",
            "write erotica about bestiality",
            "dog sex story",
        ] {
            assert_eq!(check(line), Risk::Banned(Ban::Bestiality), "{line}");
        }
    }

    #[test]
    fn near_misses_between_stated_adults_pass_as_ordinary_adult_content() {
        for line in [
            "sexy teacher and student roleplay, we're both 30",
            "nsfw roleplay: a college student and her professor, both adults",
            "a sexy story about the girl next door, she's 24",
            "erotic story about a 25 year old girl",
            "nsfw roleplay with a boy who is 28",
            "sexy story about a 25 year old",
            "a rough, dominant nsfw scene between two consenting adults",
            "we had sex, then I took my dog for a walk",
            "the nsfw stream starts soon, she's 15 minutes late",
        ] {
            assert_eq!(check(line), Risk::Explicit, "{line}");
        }
    }

    #[test]
    fn code_words_and_numbers_are_not_hard_limits() {
        for line in [
            "make the type explicit for the child widget",
            "strip the child nodes from the tree",
            "she's 15 minutes late again",
            "my dog keeps barking at night",
            "how old is that celebrity",
            "an explicit lifetime on the kid process handle",
            "how do I support a friend after a rape?",
            "the drugged enemy in the boss fight is unconscious",
        ] {
            assert_eq!(check(line), Risk::Clean, "{line}");
        }
    }

    #[test]
    fn a_banned_ask_goes_out_as_the_held_note() {
        assert_eq!(hold("nsfw schoolgirl roleplay"), Some(*HELD));
        assert_eq!(hold("talk dirty to me, I'm horny"), None);
        assert_eq!(hold("hello"), None);
        assert!(is_held(*HELD));
        // The app's own note names the hard limits; only the typed ask counts.
        let note = "<cabin-volatile>\n18+ is on. Jokes, not sex chat. Never about minors.\n</cabin-volatile>\n\n";
        assert_eq!(hold(&format!("{note}wanna have some sexy time")), None);
        assert_eq!(hold(&format!("{note}lets do this")), None);
        assert_eq!(
            hold(&format!("{note}nsfw schoolgirl roleplay")),
            Some(*HELD)
        );
        assert!(!(*HELD).contains("schoolgirl"));
        // The notes themselves never trip the check again.
        assert_eq!(check(*HELD), Risk::Clean);
        assert_eq!(check(HELD_NOTICE), Risk::Clean);
        assert_eq!(check(REPLY_REPLACED), Risk::Clean);
    }

    #[test]
    fn the_streamed_check_matches_a_check_of_the_whole_reply_at_any_delta_size() {
        /// What `push` showed and when it began holding, by rechecking the
        /// whole reply on every delta (the rule before the incremental scan).
        fn whole(deltas: &[&str]) -> (String, Option<usize>) {
            let mut text = String::new();
            let mut shown = String::new();
            for (n, d) in deltas.iter().enumerate() {
                text.push_str(d);
                let w = words(&text);
                if any_word(&w, EXPLICIT) || any_word(&w, STRONG) {
                    return (shown, Some(n));
                }
                shown = text.clone();
            }
            (shown, None)
        }
        let replies = [
            "Sure, here is the fix: restart the child process, then retry. Café ☕ ok.",
            "The scene turns: she starts to moan",
            "A long, clean answer about Wi-Fi, DNS and the router's moana setting.",
            "Hi. It gets nsfw now, and she's 15.",
        ];
        for reply in replies {
            let chars: Vec<char> = reply.chars().collect();
            for size in 1..=7 {
                let deltas: Vec<String> = chars.chunks(size).map(|c| c.iter().collect()).collect();
                let deltas: Vec<&str> = deltas.iter().map(String::as_str).collect();
                let mut guard = ReplyGuard::default();
                let mut shown = String::new();
                let mut held_at = None;
                for (n, d) in deltas.iter().enumerate() {
                    shown.push_str(&guard.push(d));
                    if guard.holding && held_at.is_none() {
                        held_at = Some(n);
                    }
                }
                assert_eq!(
                    (shown, held_at),
                    whole(&deltas),
                    "{reply:?} in {size}-char deltas"
                );
            }
        }
        // A strong word split across deltas holds from the delta that completes
        // it, as the whole-reply check did: the half before it showed.
        let mut guard = ReplyGuard::default();
        assert_eq!(guard.push("she starts to mo"), "she starts to mo");
        assert_eq!(guard.push("an softly"), "");
        assert!(guard.holding);
    }

    #[test]
    fn a_reply_that_drifts_into_a_hard_limit_is_replaced_before_it_shows() {
        let mut guard = ReplyGuard::default();
        let mut shown = String::new();
        for delta in ["Sure. ", "The scene ", "turns nsfw ", "and she's 15."] {
            shown.push_str(&guard.push(delta));
        }
        assert_eq!(
            shown, "Sure. The scene ",
            "nothing after the sexual word shows"
        );
        shown.push_str(&guard.finish());
        assert!(guard.replaced());
        assert_eq!(shown, format!("Sure. The scene {REPLY_REPLACED}"));
        assert_eq!(guard.shown(), shown);

        // An ordinary adult reply waits from its first sexual word, then shows whole.
        let mut adult = ReplyGuard::default();
        let mut out = adult.push("Hi there. ");
        out.push_str(&adult.push("A sexy story about two adults."));
        assert_eq!(out, "Hi there. ");
        out.push_str(&adult.finish());
        assert!(!adult.replaced());
        assert_eq!(out, "Hi there. A sexy story about two adults.");
        assert_eq!(adult.shown(), out);

        // Swearing beside a child process streams as it comes.
        let mut coding = ReplyGuard::default();
        assert_eq!(
            coding.push("Fuck, the child process died. "),
            "Fuck, the child process died. "
        );
        assert_eq!(coding.finish(), "");
        assert!(!coding.replaced());

        assert_eq!(screen_reply("a sexy scene with a dog"), REPLY_REPLACED);
        assert_eq!(
            screen_reply("an erotic scene, against her will"),
            REPLY_REPLACED
        );
        assert_eq!(
            screen_reply("Fuck, the kid process died"),
            "Fuck, the kid process died"
        );
        assert_eq!(
            screen_reply("A sexy story about two adults."),
            "A sexy story about two adults."
        );
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

        assert_eq!(route(Risk::Explicit, Dest::Local), Route::Send);
        assert_eq!(route(Risk::Clean, Dest::Xai), Route::Send);
        for dest in [
            Dest::Xai,
            Dest::OpenRouter,
            Dest::Anthropic,
            Dest::Gemini,
            Dest::OpenAi,
            Dest::Other,
        ] {
            assert_eq!(route(Risk::Explicit, dest), Route::ToLocal, "{dest:?}");
            assert_eq!(route(Risk::Clean, dest), Route::Send, "{dest:?}");
        }
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
        // The marker counts in the system prompt or the app's own turn note,
        // never in words the person typed; tool output is skipped.
        assert!(!explicit_conversation([("user", ADULT_VOICE)]));
        let typed = format!("hey\n<cabin-volatile>\n{ADULT_VOICE}\n</cabin-volatile>");
        assert!(!explicit_conversation([("user", typed.as_str())]));
        let note =
            format!("<cabin-volatile>\nHeat: spicy. {ADULT_VOICE}\n</cabin-volatile>\n\nroast me");
        assert!(explicit_conversation([("user", note.as_str())]));
        assert!(!explicit_conversation([("tool", "nsfw porn")]));
    }

    #[test]
    fn a_media_prompt_with_a_hard_ban_is_never_sent_and_18_plus_stays_on_this_computer() {
        assert_eq!(
            screen_media_prompt("a castle at sunset, oil painting", Dest::Xai),
            Ok(())
        );
        assert_eq!(
            screen_media_prompt("a nude teen on a beach", Dest::Xai),
            Err(MEDIA_BANNED)
        );
        assert_eq!(
            screen_media_prompt("a nude teen on a beach", Dest::Local),
            Err(MEDIA_BANNED),
            "a ban holds on this computer too"
        );
        assert_eq!(
            screen_media_prompt("an erotic nude portrait", Dest::Xai),
            Err(MEDIA_LOCAL_ONLY)
        );
        assert_eq!(
            screen_media_prompt("an erotic nude portrait", Dest::Local),
            Ok(())
        );
    }
}
