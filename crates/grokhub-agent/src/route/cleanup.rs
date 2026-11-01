//! LO-48: an opt-in local tidy of a typed prompt before the model reads it.
//!
//! Off by default (`app.json` `promptCleanup`, Settings → Local AI "Tidy typos
//! on this computer"). On, a chat turn's text goes to the on-device model with
//! [`CLEANUP_PROMPT`], and the turn waits at most [`LIMIT`] for the answer.
//! The answer is used only when [`keeps_meaning`] passes; otherwise, and on
//! any skip, timeout or error, the model gets exactly what was typed. The saved
//! chat always keeps the typed text ([`restore_typed`]).
//!
//! Never a cloud call: the step talks to the already-loaded local runtime
//! ([`super::local::warm_runtime`]) and never goes through the router, so no
//! route can send the text anywhere else.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;

use regex::Regex;

use super::local::{ChatMessage, LocalRuntime};
use crate::brain::{classify, Brief, BriefInput, Intent};
use crate::{ContentPart, InputItem};

/// The longest the turn waits for the tidied text.
pub const LIMIT: Duration = Duration::from_millis(300);
/// Shorter asks are left alone: little to fix, and a fix moves more meaning.
pub const MIN_WORDS: usize = 8;
/// Longer asks are left alone: they would not come back inside [`LIMIT`].
pub const MAX_CHARS: usize = 600;
/// The tidied text's length over the typed one's must stay in this range.
const RATIO: (f64, f64) = (0.6, 1.3);

/// What the on-device model is told. Spelling only, never a rewrite.
pub const CLEANUP_PROMPT: &str =
    "Fix spelling, typos and obvious missing punctuation in the user's message. \
Keep every word that is spelled right, the same order, the same meaning and the same tone. \
Keep numbers, names, quotes, paths, links and @-mentions exactly as written. \
Do not answer it, explain it or add anything. Reply with the corrected message only.";

/// One typed turn, as the engine saw it.
#[derive(Debug, Clone, Copy)]
pub struct TidyIn<'a> {
    pub text: &'a str,
    /// The Brief written for the typed text.
    pub brief: &'a Brief,
    /// Earlier user texts, as the Brief saw them.
    pub recent: &'a [String],
    pub image: bool,
    /// A scheduled, proactive or `/bg` run: nobody typed it.
    pub background: bool,
    /// LO-43 recognised this ask as an instant one.
    pub instant: bool,
}

/// One call in flight at a time, across every chat.
static IN_FLIGHT: AtomicBool = AtomicBool::new(false);

/// The Settings switch (or a test's [`SwitchOn`]). The engine reads it before
/// it gathers anything for [`tidy`], so an off switch costs nothing.
pub fn switched_on() -> bool {
    super::local::prompt_cleanup() || test_on()
}

/// The tidied text for the model, or `None` to send the typed text as is.
pub fn tidy(input: &TidyIn<'_>) -> Option<String> {
    tidy_on(
        &IN_FLIGHT,
        LIMIT,
        switched_on(),
        super::local::warm_runtime,
        input,
    )
}

/// [`tidy`] with its switch, runtime, lane and limit passed in.
pub(crate) fn tidy_on(
    lane: &'static AtomicBool,
    limit: Duration,
    on: bool,
    runtime: impl FnOnce() -> Option<Arc<dyn LocalRuntime>>,
    input: &TidyIn<'_>,
) -> Option<String> {
    if !on || skipped(input) {
        return None;
    }
    let reply = ask_within(lane, limit, runtime()?, input.text.trim().to_string())?;
    let tidied = unwrap_reply(&reply, input.text);
    keeps_meaning(input, &tidied).then_some(tidied)
}

/// Asks the step leaves alone without calling anything.
fn skipped(input: &TidyIn<'_>) -> bool {
    let t = input.text.trim();
    input.instant
        || input.background
        || input.brief.intent == Intent::Background
        || t.split_whitespace().count() < MIN_WORDS
        || t.chars().count() > MAX_CHARS
        || t.starts_with('/')
        || t.contains("```")
        || has_path_or_url(t)
}

fn has_path_or_url(t: &str) -> bool {
    !paths(t).is_empty() || !urls(t).is_empty()
}

/// One call on a worker thread; the turn waits at most `limit`. A call still
/// running after its limit keeps `lane` taken until it ends, so later turns
/// skip the step instead of piling calls onto the device.
fn ask_within(
    lane: &'static AtomicBool,
    limit: Duration,
    rt: Arc<dyn LocalRuntime>,
    text: String,
) -> Option<String> {
    if lane.swap(true, Ordering::SeqCst) {
        return None;
    }
    let (tx, rx) = mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("prompt-tidy".into())
        .spawn(move || {
            let out = ask(rt.as_ref(), &text);
            drop(rt);
            // Free the lane before the answer lands, so the next turn may ask.
            lane.store(false, Ordering::SeqCst);
            let _ = tx.send(out);
        });
    if spawned.is_err() {
        lane.store(false, Ordering::SeqCst);
        return None;
    }
    rx.recv_timeout(limit).ok().flatten()
}

fn ask(rt: &dyn LocalRuntime, text: &str) -> Option<String> {
    let messages = [
        ChatMessage {
            role: "system".into(),
            content: CLEANUP_PROMPT.into(),
        },
        ChatMessage {
            role: "user".into(),
            content: text.into(),
        },
    ];
    let mut deltas = String::new();
    let out = rt
        .chat_stream_at(&messages, Some("none"), &mut |d| deltas.push_str(d))
        .ok()?;
    Some(if out.is_empty() { deltas } else { out })
}

/// The reply without the quotes a small model likes to wrap it in.
fn unwrap_reply(reply: &str, typed: &str) -> String {
    let r = reply.trim();
    for (open, close) in [('"', '"'), ('\u{201c}', '\u{201d}'), ('`', '`')] {
        if !typed.trim_start().starts_with(open) {
            if let Some(inner) = r.strip_prefix(open).and_then(|s| s.strip_suffix(close)) {
                return inner.trim().to_string();
            }
        }
    }
    r.to_string()
}

/// The tidied text says the same thing: a different text of about the same
/// length and no more lines, every number, path, URL, quoted span and
/// @-mention kept (none added), as many "not" words, no word added or
/// dropped but small ones, and the same Brief intent, care and domain when
/// sorted again.
pub(crate) fn keeps_meaning(input: &TidyIn<'_>, tidied: &str) -> bool {
    let typed = input.text.trim();
    let (a, b) = (typed.chars().count(), tidied.chars().count());
    if tidied.is_empty() || tidied == typed || a == 0 {
        return false;
    }
    let ratio = b as f64 / a as f64;
    if !(RATIO.0..=RATIO.1).contains(&ratio) || tidied.lines().count() > typed.lines().count() {
        return false;
    }
    if exact_spans(typed) != exact_spans(tidied)
        || negations(typed) != negations(tidied)
        || !same_words(typed, tidied)
    {
        return false;
    }
    let sort = |text: &str| {
        classify(
            &BriefInput {
                text,
                files: usize::from(input.image),
                tools: &[],
                recent: input.recent,
                background: false,
            },
            None,
        )
    };
    // A model-sorted Brief can't be asked again here (no cloud call), so the
    // typed text is sorted the same way as the tidied one instead.
    let want = if input.brief.by == "model" {
        sort(typed)
    } else {
        input.brief.clone()
    };
    let got = sort(tidied);
    (want.intent, want.care, &want.domain) == (got.intent, got.care, &got.domain)
}

fn pattern(cell: &'static std::sync::OnceLock<Regex>, re: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(re).expect("cleanup pattern"))
}

fn numbers(t: &str) -> Vec<String> {
    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    found(pattern(&RE, r"\d+(?:[.,:]\d+)*"), t)
}

fn quoted(t: &str) -> Vec<String> {
    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    found(
        pattern(
            &RE,
            r#""[^"\n]+"|“[^”\n]+”|`[^`\n]+`|(?:^|\s)'[^'\n]+'(?:$|[\s.,!?;:])"#,
        ),
        t,
    )
    .into_iter()
    .map(|s| {
        s.trim()
            .trim_end_matches(['.', ',', '!', '?', ';', ':'])
            .to_string()
    })
    .collect()
}

fn mentions(t: &str) -> Vec<String> {
    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    found(pattern(&RE, r"(?:^|[^\w@])(@[\w.-]*\w)"), t)
        .into_iter()
        .map(|s| s.trim_start_matches(|c: char| c != '@').to_string())
        .collect()
}

fn urls(t: &str) -> Vec<String> {
    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    found(pattern(&RE, r"(?i)\b(?:[a-z][a-z0-9+.-]*://|www\.)\S+"), t)
}

/// `D:/Loadout/saves`, `~/notes`, `C:\Games`, `./run.sh`, `src/main.rs`.
fn paths(t: &str) -> Vec<String> {
    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    found(
        pattern(
            &RE,
            r"(?:\b[A-Za-z]:[\\/]|~[\\/]|\.{1,2}[\\/]|\\\\)\S*|\S+[\\/]\S+",
        ),
        t,
    )
}

fn found(re: &Regex, t: &str) -> Vec<String> {
    re.find_iter(t).map(|m| m.as_str().to_string()).collect()
}

/// Every span that must come back unchanged, sorted, so order and count both count.
fn exact_spans(t: &str) -> Vec<String> {
    let mut all = [numbers(t), quoted(t), mentions(t), urls(t), paths(t)].concat();
    all.sort();
    all
}

/// The same words, spelled better: every tidied word is a typed word, a near
/// miss of one ([`near`]) or its spelled-out shorthand, and every typed word
/// is still there the same way; only [`SMALL_WORDS`] may come or go. Catches
/// "Here is the corrected message:", answers and swapped words.
fn same_words(typed: &str, tidied: &str) -> bool {
    let (typed, tidied) = (word_list(typed), word_list(tidied));
    let spelled = |w: &str, from: &[String], expands: bool| {
        from.iter().any(|f| {
            near(f, w)
                || SHORTHAND.iter().any(|(short, long)| {
                    let (s, l) = if expands {
                        (f.as_str(), w)
                    } else {
                        (w, f.as_str())
                    };
                    s == *short && long.split(' ').any(|x| x == l)
                })
        })
    };
    let kept = |w: &String, from: &[String], expands: bool| {
        SMALL_WORDS.contains(&w.as_str()) || spelled(w, from, expands)
    };
    tidied.iter().all(|w| kept(w, &typed, true)) && typed.iter().all(|w| kept(w, &tidied, false))
}

/// Words a typo fix may add or drop: grammar, "please" and filler.
const SMALL_WORDS: &[&str] = &[
    "a", "an", "the", "to", "is", "are", "am", "was", "i", "im", "it", "its", "of", "and", "in",
    "at", "for", "my", "me", "you", "your", "be", "so", "that", "this", "with", "please", "just",
    "ugh", "lol", "lmao", "haha", "hmm", "um", "uh",
];

/// Chat shorthand and the words it stands for.
const SHORTHAND: &[(&str, &str)] = &[
    ("pls", "please"),
    ("plz", "please"),
    ("u", "you"),
    ("ur", "your youre"),
    ("r", "are"),
    ("thx", "thanks"),
    ("ty", "thank thanks you"),
    ("bc", "because"),
    ("cuz", "because"),
    ("abt", "about"),
    ("rn", "right now"),
];

/// A typo away: the same first letter, and one edit for a word of up to four
/// letters or two for a longer one (a swap of neighbours is one). The first
/// letter keeps "install" and "uninstall", "enable" and "disable" apart.
fn near(a: &str, b: &str) -> bool {
    let longest = a.chars().count().max(b.chars().count());
    a.chars().next() == b.chars().next() && edits(a, b) <= if longest <= 4 { 1 } else { 2 }
}

/// How many words say "not": a dropped or added one flips the ask.
fn negations(t: &str) -> usize {
    const NOT: &[&str] = &[
        "not", "no", "never", "nor", "none", "nothing", "nobody", "without", "cannot", "dont",
        "cant", "wont", "didnt", "doesnt", "isnt", "arent", "wasnt", "werent", "shouldnt",
        "couldnt", "wouldnt", "hasnt", "havent", "hadnt", "aint",
    ];
    word_list(t)
        .iter()
        .filter(|w| NOT.contains(&w.as_str()))
        .count()
}

fn word_list(t: &str) -> Vec<String> {
    t.split_whitespace()
        .map(|w| {
            w.chars()
                .filter(|c| c.is_alphanumeric())
                .flat_map(char::to_lowercase)
                .collect::<String>()
        })
        .filter(|w| !w.is_empty())
        .collect()
}

/// Edit distance with neighbour swaps (optimal string alignment), for short words.
fn edits(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut d = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for (j, cell) in d[0].iter_mut().enumerate() {
        *cell = j;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut best = (d[i - 1][j] + 1)
                .min(d[i][j - 1] + 1)
                .min(d[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                best = best.min(d[i - 2][j - 2] + 1);
            }
            d[i][j] = best;
        }
    }
    d[a.len()][b.len()]
}

/// Put the typed text back into the turn about to be saved: the first user
/// message read `sent` (after any `<cabin-volatile>` block); it reads `typed`
/// instead, so a reopened chat shows what was typed.
pub fn restore_typed(items: &mut [InputItem], sent: &str, typed: &str) {
    let first_user = items.iter_mut().find_map(|item| match item {
        InputItem::Message { role, content } if role == "user" => Some(content),
        _ => None,
    });
    let Some(ContentPart::InputText(text)) = first_user.and_then(|c| c.first_mut()) else {
        return;
    };
    let start = text.len() - crate::prefix_cache::visible_user_text(text).len();
    let at = if text[start..].starts_with(sent) {
        Some(start)
    } else {
        text[start..].find(sent).map(|i| start + i)
    };
    if let Some(at) = at {
        text.replace_range(at..at + sent.len(), typed);
    }
}

#[cfg(not(test))]
fn test_on() -> bool {
    false
}

#[cfg(test)]
thread_local! {
    static TEST_ON: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(test)]
fn test_on() -> bool {
    TEST_ON.with(std::cell::Cell::get)
}

/// Tests only: the switch on for this thread. Dropping it switches it off.
#[cfg(test)]
pub(crate) struct SwitchOn;

#[cfg(test)]
impl SwitchOn {
    pub(crate) fn new() -> Self {
        TEST_ON.with(|c| c.set(true));
        Self
    }
}

#[cfg(test)]
impl Drop for SwitchOn {
    fn drop(&mut self) {
        TEST_ON.with(|c| c.set(false));
    }
}

#[cfg(test)]
#[path = "cleanup_tests.rs"]
mod tests;
