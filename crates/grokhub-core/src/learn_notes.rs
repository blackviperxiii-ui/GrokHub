//! What a chat teaches the cabin, as clean lines for USER.md, SOUL.md and
//! MEMORY.md. A user message becomes a note only when it states a lasting
//! fact about the user, corrects how the agent behaves, or asks the cabin to
//! remember something. Everything else (questions, chatter, task requests)
//! is never written, so raw chat can't pile up in the files.
//!
//! Each note has a key. A newer note with the same key replaces the old line
//! ("call me Sam" after "my name is Alex"), so the files describe who the
//! user and the agent are now instead of collecting duplicates.

use crate::adult_guard::{self, Risk};
use crate::learning::is_greeting_chitchat;
use crate::reflect::MemoryEdit;
use crate::{is_plain_text, redact_recall};

/// Messages longer than this are tasks or pastes, not facts.
const NOTE_MAX_CHARS: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteFile {
    /// Who the user is: name, work, setup, games, how they like replies.
    User,
    /// Who the agent is: corrections and habits the user asked for.
    Soul,
    /// Anything the user asked the cabin to remember.
    Memory,
}

impl NoteFile {
    pub fn file_name(self) -> &'static str {
        match self {
            Self::User => "USER.md",
            Self::Soul => "SOUL.md",
            Self::Memory => "MEMORY.md",
        }
    }

    fn heading(self) -> &'static str {
        match self {
            Self::User => "# User",
            Self::Soul => "# Soul",
            Self::Memory => "# Memory",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    pub file: NoteFile,
    /// Same key, same slot: a newer note replaces the line.
    pub key: String,
    /// The line without its `- ` bullet (and, for MEMORY.md, without its date).
    pub text: String,
}

/// Whether `text` may be written to a memory file: one short plain line
/// with nothing for recall redaction to hide (emails, secrets, numbers) and
/// no adult content. User messages and the model's `USER_FACT:` lines both
/// pass through this.
pub fn is_safe_note_text(text: &str) -> bool {
    let text = text.trim();
    !text.is_empty()
        && text.chars().count() <= NOTE_MAX_CHARS
        && !text.contains('\n')
        && is_plain_text(text)
        && redact_recall(text).0 == text
        && adult_guard::check(text) == Risk::Clean
}

/// The note one user message teaches, if any.
pub fn note_from_message(message: &str) -> Option<Note> {
    let msg = message.trim();
    if !is_safe_note_text(msg)
        || msg.starts_with('/')
        || msg.ends_with('?')
        || is_greeting_chitchat(msg)
    {
        return None;
    }
    let body = strip_lead(msg);
    let lower = body.to_ascii_lowercase();
    if let Some(rest) = strip_any(&lower, body, &["remember that ", "remember ", "note that "]) {
        let text = sentence(rest);
        return (!text.is_empty()).then(|| Note {
            file: NoteFile::Memory,
            key: format!("memory:{}", slug(&text, 6)),
            text,
        });
    }
    if let Some(note) = soul_note(&lower, body) {
        return Some(note);
    }
    user_note(&lower, body)
}

/// Corrections and standing asks about the agent's behavior.
fn soul_note(lower: &str, body: &str) -> Option<Note> {
    const NEVER: [&str; 6] = ["don't ", "dont ", "do not ", "never ", "stop ", "quit "];
    const ALWAYS: [&str; 3] = ["always ", "from now on, ", "from now on "];
    let (lead, rest) = if let Some(rest) = strip_any(lower, body, &NEVER) {
        ("Don't", rest)
    } else {
        ("Always", strip_any(lower, body, &ALWAYS)?)
    };
    let rest = sentence(rest);
    let rest = rest.trim_end_matches('.');
    let first = rest
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    // "don't worry about it" and "never mind" are chat, not a habit.
    const CHAT: [&str; 9] = [
        "worry",
        "mind",
        "bother",
        "care",
        "know",
        "think",
        "get",
        "understand",
        "see",
    ];
    if rest.split_whitespace().count() < 2 || CHAT.contains(&first.as_str()) {
        return None;
    }
    let object = lower_first(rest);
    Some(Note {
        file: NoteFile::Soul,
        key: format!("soul:{}", slug(&object, 4)),
        text: format!("{lead} {object}."),
    })
}

/// First-person facts about the user, rewritten in the third person.
fn user_note(lower: &str, body: &str) -> Option<Note> {
    // (prefix, rewrite, slot). A slot keeps one line per topic.
    const RULES: [(&str, &str, Option<&str>); 16] = [
        ("my name is ", "Name: ", Some("name")),
        ("call me ", "Name: ", Some("name")),
        ("my job is ", "Job: ", Some("job")),
        ("i work as ", "Works as ", Some("job")),
        ("i work at ", "Works at ", Some("job")),
        ("i work in ", "Works in ", Some("job")),
        ("i'm a ", "Is a ", None),
        ("i'm an ", "Is an ", None),
        ("i am a ", "Is a ", None),
        ("i am an ", "Is an ", None),
        ("i prefer ", "Prefers ", None),
        ("i like ", "Likes ", None),
        ("i love ", "Loves ", None),
        ("i hate ", "Dislikes ", None),
        ("i play ", "Plays ", None),
        ("i use ", "Uses ", None),
    ];
    for (prefix, rewrite, slot) in RULES {
        let Some(rest) = strip_any(lower, body, &[prefix]) else {
            continue;
        };
        let rest = sentence(rest);
        let rest = rest.trim_end_matches('.');
        let first = rest
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        // "I like that" or "I love you" is a reaction, not a fact.
        const VAGUE: [&str; 9] = [
            "it", "that", "this", "these", "those", "them", "you", "your", "how",
        ];
        if rest.is_empty()
            || rest.split_whitespace().count() > 20
            || VAGUE.contains(&first.as_str())
        {
            return None;
        }
        let rest_lower = rest.to_ascii_lowercase();
        let key = match slot {
            Some(slot) => format!("user:{slot}"),
            None if mentions_replies(&rest_lower) => "user:replies".into(),
            None => format!(
                "user:{}:{}",
                rewrite
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .to_ascii_lowercase(),
                slug(rest, 3)
            ),
        };
        return Some(Note {
            file: NoteFile::User,
            key,
            text: format!("{rewrite}{rest}."),
        });
    }
    None
}

fn mentions_replies(lower: &str) -> bool {
    ["replies", "answers", "responses"]
        .iter()
        .any(|w| lower.split(|c: char| !c.is_alphanumeric()).any(|t| t == *w))
}

/// Drop a polite or chatty lead-in ("please", "ok,", "hey,").
fn strip_lead(msg: &str) -> &str {
    let mut s = msg;
    loop {
        let lower = s.to_ascii_lowercase();
        let Some(rest) = [
            "please ", "ok, ", "okay, ", "hey, ", "also, ", "btw, ", "fyi, ",
        ]
        .iter()
        .find_map(|p| lower.starts_with(p).then(|| &s[p.len()..])) else {
            return s.trim();
        };
        s = rest.trim_start();
    }
}

/// `body` without the first of `prefixes` that `lower` starts with. `lower`
/// is `body` ASCII-lowercased, so byte offsets match.
fn strip_any<'a>(lower: &str, body: &'a str, prefixes: &[&str]) -> Option<&'a str> {
    prefixes
        .iter()
        .find(|p| lower.starts_with(*p))
        .map(|p| body[p.len()..].trim())
}

/// One clean sentence: trimmed, a trailing `!` or `.` folded to one `.`-less
/// clause, and its first letter as typed.
fn sentence(text: &str) -> String {
    text.trim()
        .trim_end_matches(['!', '.', ' '])
        .trim()
        .to_string()
}

fn lower_first(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        // Keep "I", acronyms and names as typed.
        Some(c)
            if c.is_ascii_uppercase() && chars.next().is_some_and(|n| n.is_ascii_lowercase()) =>
        {
            let mut out = c.to_ascii_lowercase().to_string();
            out.push_str(&text[1..]);
            out
        }
        _ => text.to_string(),
    }
}

fn slug(text: &str, words: usize) -> String {
    text.to_ascii_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| w.len() >= 2 && !matches!(*w, "the" | "an" | "my" | "me" | "to" | "of"))
        .take(words)
        .collect::<Vec<_>>()
        .join("-")
}

/// The key of a line already in a file, so a new note can replace it.
pub fn line_key(file: NoteFile, line: &str) -> Option<String> {
    let text = line.trim().strip_prefix("- ")?.trim();
    match file {
        NoteFile::Memory => {
            let text = text.split_once(": ").map_or(text, |(date, rest)| {
                if date.len() == 10 && date.as_bytes()[4] == b'-' {
                    rest
                } else {
                    text
                }
            });
            Some(format!("memory:{}", slug(text, 6)))
        }
        NoteFile::Soul => {
            let rest = text
                .strip_prefix("Don't ")
                .or_else(|| text.strip_prefix("Always "))?;
            Some(format!("soul:{}", slug(rest.trim_end_matches('.'), 4)))
        }
        NoteFile::User => {
            if text.starts_with("Name: ") {
                return Some("user:name".into());
            }
            if ["Job: ", "Works as ", "Works at ", "Works in "]
                .iter()
                .any(|p| text.starts_with(p))
            {
                return Some("user:job".into());
            }
            let (verb, rest) = text.split_once(' ')?;
            let rest = rest.trim_end_matches('.');
            if mentions_replies(&rest.to_ascii_lowercase()) {
                return Some("user:replies".into());
            }
            Some(format!(
                "user:{}:{}",
                verb.to_ascii_lowercase(),
                slug(rest, 3)
            ))
        }
    }
}

/// Lines the old reflect copied into MEMORY.md straight from chat: plain
/// sentences, not headings, bullets, numbered items or quotes, and not a line
/// of `seed` (the starter template). Like the old reflect's filter
/// (`fact_candidates_from`), slash commands and `HOST_`/`VERIFY_` lines are
/// never counted. The learning loop writes dated bullets instead.
pub fn raw_chat_lines(memory_md: &str, seed: &str) -> Vec<String> {
    let template: Vec<&str> = seed.lines().map(str::trim).collect();
    let mut out: Vec<String> = Vec::new();
    for line in memory_md.lines().map(str::trim) {
        let structured = line.is_empty()
            || line.starts_with(['#', '-', '*', '>', '|', '`', '+', '/'])
            || line.starts_with("HOST_")
            || line.starts_with("VERIFY_")
            || is_numbered_item(line)
            || template.contains(&line);
        if structured || line.len() >= 200 || !is_plain_text(line) {
            continue;
        }
        if crate::learning::is_durable_fact(line) && !out.iter().any(|l| l == line) {
            out.push(line.to_string());
        }
    }
    out
}

/// `1. Keep this` or `2) Keep this`: a numbered list item, not chat.
fn is_numbered_item(line: &str) -> bool {
    let digits = line.chars().take_while(char::is_ascii_digit).count();
    digits > 0 && matches!(line[digits..].chars().next(), Some('.' | ')'))
}

/// `memory_md` without the lines in `drop` (compared trimmed).
pub fn without_lines(memory_md: &str, drop: &[String]) -> String {
    memory_md
        .split_inclusive('\n')
        .filter(|l| !drop.iter().any(|d| d == l.trim()))
        .collect()
}

/// Whether `doc` is still the first-run template (or empty).
pub fn is_placeholder(doc: &str) -> bool {
    let body: Vec<&str> = doc
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect();
    body.is_empty() || (body.len() == 1 && body[0].ends_with("Edit this."))
}

/// Write `notes` for `file` into `doc`: replace the line with the same key,
/// else append. A placeholder template is replaced by the heading. MEMORY.md
/// lines carry `day` (`YYYY-MM-DD`). `diff` lists the lines written.
pub fn apply_notes(doc: &str, file: NoteFile, notes: &[Note], day: &str) -> MemoryEdit {
    let mut lines: Vec<String> = if is_placeholder(doc) {
        vec![file.heading().to_string(), String::new()]
    } else {
        doc.lines().map(str::to_string).collect()
    };
    let mut diff = Vec::new();
    for note in notes.iter().filter(|n| n.file == file) {
        let line = match file {
            NoteFile::Memory => format!("- {day}: {}", note.text),
            _ => format!("- {}", note.text),
        };
        let same = |l: &String| line_key(file, l).as_deref() == Some(note.key.as_str());
        if let Some(old) = lines.iter_mut().find(|l| same(l)) {
            let unchanged = match file {
                NoteFile::Memory => true,
                _ => *old == line,
            };
            if unchanged {
                continue;
            }
            diff.push(format!("~ {line}"));
            *old = line;
        } else {
            diff.push(format!("+ {line}"));
            lines.push(line);
        }
    }
    if diff.is_empty() {
        return MemoryEdit {
            next: doc.to_string(),
            diff: String::new(),
        };
    }
    let mut next = lines.join("\n");
    next.push('\n');
    MemoryEdit {
        next,
        diff: diff.join("\n"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_raw_chat_lines_are_found_and_only_they_are_dropped() {
        let seed = "# Memory\n\nLong-term notes.\n";
        let memory = "# Memory\n\nLong-term notes.\n- 2026-10-28: the spare key is under the planter\nI prefer the harbor light on at night\n> a quote I kept\nI prefer the harbor light on at night\nhello\n1. Keep the boat keys on the hook\n2) Water the plants on Sunday\n/remember the dentist on Friday\nHOST_NOTE keep the router on\nVERIFY_ the backup runs nightly\n";
        let raw = raw_chat_lines(memory, seed);
        assert_eq!(
            raw,
            vec!["I prefer the harbor light on at night".to_string()]
        );
        assert_eq!(
            without_lines(memory, &raw),
            "# Memory\n\nLong-term notes.\n- 2026-10-28: the spare key is under the planter\n> a quote I kept\nhello\n1. Keep the boat keys on the hook\n2) Water the plants on Sunday\n/remember the dentist on Friday\nHOST_NOTE keep the router on\nVERIFY_ the backup runs nightly\n"
        );
        assert!(
            raw_chat_lines(seed, seed).is_empty(),
            "the template alone has nothing to tidy"
        );
    }

    fn note(msg: &str) -> Option<(NoteFile, String, String)> {
        note_from_message(msg).map(|n| (n.file, n.key, n.text))
    }

    #[test]
    fn user_facts_become_third_person_lines() {
        assert_eq!(
            note("My name is Sam"),
            Some((NoteFile::User, "user:name".into(), "Name: Sam.".into()))
        );
        assert_eq!(
            note("I'm a night-shift nurse."),
            Some((
                NoteFile::User,
                "user:is:night-shift-nurse".into(),
                "Is a night-shift nurse.".into()
            ))
        );
        assert_eq!(
            note("I work as a line cook"),
            Some((
                NoteFile::User,
                "user:job".into(),
                "Works as a line cook.".into()
            ))
        );
        assert_eq!(
            note("I like short replies with bullet points"),
            Some((
                NoteFile::User,
                "user:replies".into(),
                "Likes short replies with bullet points.".into()
            ))
        );
        assert_eq!(
            note("I play Hades on the Steam Deck"),
            Some((
                NoteFile::User,
                "user:plays:hades-on-steam".into(),
                "Plays Hades on the Steam Deck.".into()
            ))
        );
    }

    #[test]
    fn corrections_go_to_soul() {
        assert_eq!(
            note("please don't use emoji in replies"),
            Some((
                NoteFile::Soul,
                "soul:use-emoji-in-replies".into(),
                "Don't use emoji in replies.".into()
            ))
        );
        assert_eq!(
            note("Always use emoji in replies!"),
            Some((
                NoteFile::Soul,
                "soul:use-emoji-in-replies".into(),
                "Always use emoji in replies.".into()
            ))
        );
        assert_eq!(note("stop"), None);
    }

    #[test]
    fn remember_goes_to_memory() {
        assert_eq!(
            note("remember that the router password lives in the blue notebook"),
            Some((
                NoteFile::Memory,
                "memory:router-password-lives-in-blue-notebook".into(),
                "the router password lives in the blue notebook".into()
            ))
        );
    }

    #[test]
    fn raw_chat_never_becomes_a_note() {
        for msg in [
            "hi",
            "how are you",
            "what's the weather tomorrow?",
            "fix the build script and push it",
            "can you summarize this article for me",
            "/remember x",
            "I use token sk-abcdefghijklmnopqrstuv for the api",
            "line one\nline two",
            "I like that",
            "don't worry about it",
            "never mind",
            &format!("I like {}", "long ".repeat(60)),
        ] {
            assert_eq!(note(msg), None, "{msg}");
        }
    }

    #[test]
    fn a_newer_note_replaces_the_old_line_and_a_template_is_cleared() {
        let seed = "# User\n\nWho you are. Edit this.\n";
        assert!(is_placeholder(seed));
        let n1 = note_from_message("my name is Alex").unwrap();
        let first = apply_notes(seed, NoteFile::User, &[n1], "2026-10-29");
        assert_eq!(first.next, "# User\n\n- Name: Alex.\n");
        assert_eq!(first.diff, "+ - Name: Alex.");
        assert!(!is_placeholder(&first.next));

        let n2 = note_from_message("call me Sam").unwrap();
        let n3 = note_from_message("I play chess").unwrap();
        let second = apply_notes(&first.next, NoteFile::User, &[n2, n3], "2026-10-29");
        assert_eq!(second.next, "# User\n\n- Name: Sam.\n- Plays chess.\n");
        assert_eq!(second.diff, "~ - Name: Sam.\n+ - Plays chess.");

        let again = note_from_message("call me Sam").unwrap();
        let third = apply_notes(&second.next, NoteFile::User, &[again], "2026-10-30");
        assert_eq!(third.next, second.next);
        assert_eq!(third.diff, "");
    }

    #[test]
    fn soul_correction_flips_the_old_habit() {
        let doc = "# Soul\n\nCalm and brief.\n- Always use emoji in replies.\n";
        let flip = note_from_message("don't use emoji in replies").unwrap();
        let edit = apply_notes(doc, NoteFile::Soul, &[flip], "2026-10-29");
        assert_eq!(
            edit.next,
            "# Soul\n\nCalm and brief.\n- Don't use emoji in replies.\n"
        );
    }

    #[test]
    fn memory_lines_are_dated_and_kept_once() {
        let n = note_from_message("remember the spare key is under the planter").unwrap();
        let edit = apply_notes(
            "# Memory\n\nLong-term notes.\n",
            NoteFile::Memory,
            std::slice::from_ref(&n),
            "2026-10-29",
        );
        assert_eq!(
            edit.next,
            "# Memory\n\nLong-term notes.\n- 2026-10-29: the spare key is under the planter\n"
        );
        let again = apply_notes(&edit.next, NoteFile::Memory, &[n], "2026-10-30");
        assert_eq!(again.next, edit.next);
    }

    #[test]
    fn notes_for_another_file_are_ignored() {
        let soul = note_from_message("never use emoji in replies").unwrap();
        let edit = apply_notes(
            "# User\n\n- Plays chess.\n",
            NoteFile::User,
            &[soul],
            "2026-10-29",
        );
        assert_eq!(edit.next, "# User\n\n- Plays chess.\n");
        assert_eq!(edit.diff, "");
    }
}
