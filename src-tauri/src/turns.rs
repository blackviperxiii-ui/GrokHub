//! The app reads what each message is for, so there is no Chat / Plan / btw
//! choice to make. Nothing running: a big multi-step job plans first and
//! anything else is answered. A reply running: a side question gets its own
//! read-only answer beside the work, a correction steers the work, "stop"
//! stops it, and a new job waits its turn (the page does those last three).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use grokhub_agent::route::intent::{self, TurnCx, TurnRoute};
use grokhub_core::Typed;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};

use crate::{agent, ShellState, CHAT_EVENT};

/// A side answer runs under `<chat>~side-<n>`. The on-device router reads
/// the same mark, so a side answer never asks for a tier.
pub const SIDE_MARK: &str = grokhub_agent::route::local_scale::SIDE_MARK;

/// Each chat's last read message and the route it took, so "no, just
/// answer" or "make a plan first" right after it can fix it.
#[derive(Default)]
pub struct Reads(Mutex<HashMap<String, (String, TurnRoute)>>);

impl Reads {
    /// The reply running in `conversation_id` answers casual talk: its turn
    /// was read as chat (not a plan) and its ask is small talk or a quick
    /// question ([`intent::casual_turn`]).
    fn casual_running(&self, conversation_id: &str) -> bool {
        let reads = self.0.lock().unwrap_or_else(|e| e.into_inner());
        reads
            .get(conversation_id)
            .is_some_and(|(ask, took)| *took == TurnRoute::Chat && intent::casual_turn(ask))
    }
}

/// Each chat's typed marks: `{config}/typed-marks/<hash of the chat id>.json`.
const MARKS_DIR: &str = "typed-marks";
/// A chat keeps the marks of its newest this many messages.
const MARKS_KEPT: usize = 256;
/// One writer at a time reads and rewrites a chat's marks.
static MARKS_LOCK: Mutex<()> = Mutex::new(());

/// Which words of one message of a chat the person typed, for a message
/// some of which they didn't type (a Workboard card's message, a Code tab
/// Resume, a message after a Focus help line or a Pulse card's report, a
/// fix button's prompt sent from the box). It holds no words: the
/// message's hash and size ([`as_sent`]) and where the typed words sit in
/// it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Mark {
    sha: String,
    size: usize,
    at: usize,
    len: usize,
    /// Where the typed words' later parts sit, when the person typed on
    /// both sides of text they didn't ([`Typed::keep_parts`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    more: Vec<(usize, usize)>,
}

impl Mark {
    /// Where each part of the typed words sits in the message.
    fn parts(&self) -> impl Iterator<Item = (usize, usize)> + '_ {
        std::iter::once((self.at, self.len)).chain(self.more.iter().copied())
    }

    /// `body` is this mark's message, or starts with it and goes on past a
    /// line break: a reopened chat shows a message's plus-button file lines
    /// after it.
    fn fits(&self, body: &str) -> bool {
        match (body.get(..self.size), body.get(self.size..)) {
            (Some(head), Some(rest)) => {
                (rest.is_empty() || rest.starts_with('\n'))
                    && grokhub_core::model_download::sha256_hex(head.as_bytes()) == self.sha
            }
            _ => false,
        }
    }
}

fn marks_path(config_dir: &Path, chat: &str) -> PathBuf {
    let name = grokhub_core::model_download::sha256_hex(chat.as_bytes());
    config_dir
        .join(MARKS_DIR)
        .join(format!("{}.json", &name[..32]))
}

fn read_marks(config_dir: &Path, chat: &str) -> Vec<Mark> {
    std::fs::read_to_string(marks_path(config_dir, chat))
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

/// A message as the window can send it again, for its mark: without the
/// attached context the app put after its typed references (a reopened
/// chat shows that), and trimmed.
fn as_sent(text: &str) -> &str {
    let attached = format!("\n\n{}\n", grokhub_core::ATTACHED_CONTEXT_HEADER);
    text.split_once(attached.as_str())
        .map_or(text, |(before, _)| before)
        .trim()
}

/// `text`, a message a turn of `chat` runs or the window is about to send
/// there, of which the person typed `typed`: when some of it isn't theirs,
/// kept with the chat, so the same message sent there again (`/retry`,
/// after a restart, once a model is back) reads only those words.
pub(crate) fn note_sent(
    config_dir: &Path,
    chat: &str,
    text: &str,
    typed: &Typed,
) -> Result<(), String> {
    let body = as_sent(text);
    let words = typed.as_str().trim();
    if words == body {
        return Ok(());
    }
    let mut parts = typed_parts(body, words).into_iter();
    let (at, len) = parts.next().unwrap_or((0, 0));
    let mark = Mark {
        sha: grokhub_core::model_download::sha256_hex(body.as_bytes()),
        size: body.len(),
        at,
        len,
        more: parts.collect(),
    };
    let _one = MARKS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut marks = read_marks(config_dir, chat);
    marks.retain(|m| m.sha != mark.sha);
    marks.push(mark);
    let over = marks.len().saturating_sub(MARKS_KEPT);
    marks.drain(..over);
    let path = marks_path(config_dir, chat);
    let saved = (|| {
        std::fs::create_dir_all(config_dir.join(MARKS_DIR))?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec(&marks)?)?;
        std::fs::rename(&tmp, &path)
    })();
    saved.map_err(|e| format!("Couldn't save which words of this message you typed: {e}"))
}

/// Where `words`, the typed words of `body`, sit in it: in one piece, or
/// line by line in order when they come in parts ([`Typed::keep_parts`]
/// joins those by line breaks). Words it can't place keep nothing.
fn typed_parts(body: &str, words: &str) -> Vec<(usize, usize)> {
    if words.is_empty() {
        return Vec::new();
    }
    if let Some(at) = body.find(words) {
        return vec![(at, words.len())];
    }
    let mut from = 0;
    let mut parts = Vec::new();
    for line in words.split('\n') {
        let Some(at) = body.get(from..).and_then(|rest| rest.find(line)) else {
            return Vec::new();
        };
        parts.push((from + at, line.len()));
        from += at + line.len();
    }
    parts
}

/// The words the person typed in `sent`, a message the window sent in
/// `chat`: those [`note_sent`] kept for that message (the longest noted
/// message it is, or starts with before a line break), else
/// [`typed_words`]. A message that is none of the chat's noted ones, so one
/// the person typed or changed, counts as typed in full.
pub(crate) fn typed_in(config_dir: &Path, chat: &str, sent: &Typed) -> Typed {
    let body = as_sent(sent.as_str());
    match read_marks(config_dir, chat)
        .into_iter()
        .filter(|m| m.fits(body))
        .max_by_key(|m| m.size)
    {
        Some(mark) => sent.keep_parts(|s| {
            let body = as_sent(s);
            mark.parts()
                .filter_map(|(at, len)| body.get(at..at.saturating_add(len)))
                .collect()
        }),
        None => typed_words(sent),
    }
}

/// The words the person typed in `sent`, a message the window sent in
/// `chat`. Sent from its box with texts the window put there for the app, a
/// card or a model (`untyped`: a fix button's prompt, a Pulse card's offer,
/// a resume block Stop put back), the window says what isn't typed: those
/// texts, and any reference to what they name ([`Typed::without`]). The
/// person's words around them, a line added under them too, are typed.
/// Otherwise the chat's marks say ([`typed_in`]).
pub(crate) fn typed_of(config_dir: &Path, chat: &str, sent: &Typed, untyped: &[String]) -> Typed {
    if untyped.is_empty() {
        typed_in(config_dir, chat, sent)
    } else {
        typed_words(sent).without(untyped)
    }
}

/// Deleted chats take their marks with them.
pub(crate) fn forget_marks(config_dir: &Path, chats: &[String]) {
    for chat in chats {
        let _ = std::fs::remove_file(marks_path(config_dir, chat));
    }
}

/// The words the person typed in `sent`, a message as the window sent it:
/// all of it, less a told side answer's answer, which a model wrote
/// ([`intent::person_words`]), and less the attached context a reopened
/// chat shows after it, which the app read.
pub(crate) fn typed_words(sent: &Typed) -> Typed {
    sent.keep(|s| intent::person_words(as_sent(s)))
}

/// A message the window sent, as its turn runs it: after the report of the
/// Pulse card its chat opened from (`carry`), and after a Focus help line
/// before that (`focus`, given the text so far). Neither is the person's
/// typing, so the typed words are `typed` whatever they add.
pub(crate) fn composed(
    sent: &Typed,
    typed: Typed,
    carry: Option<&str>,
    focus: impl FnOnce(&str) -> Option<String>,
) -> (String, Typed) {
    let text = match carry {
        Some(context) => format!(
            "{}{}",
            grokhub_core::card_report::card_carry(context),
            sent.as_str()
        ),
        None => sent.as_str().to_string(),
    };
    let text = match focus(&text) {
        Some(line) => format!("{line}\n{text}"),
        None => text,
    };
    (text, typed)
}

/// The route for a message sent while nothing runs. A plan still waiting
/// on approval keeps planning; "no, just answer" or "make a plan first"
/// right after a turn fixes that turn's read and teaches the reader, on
/// this device only.
pub fn pick_mode(state: &ShellState, conversation_id: &str, text: &str) -> agent::SessionMode {
    let mut reads = state.turn_reads.0.lock().unwrap_or_else(|e| e.into_inner());
    let fix = intent::correction_of_previous(text);
    if let (Some(fix), Some((prev, was))) = (fix, reads.remove(conversation_id)) {
        // A scratch chat teaches the reader nothing.
        if fix != was && !grokhub_agent::scratch::is_scratch(conversation_id) {
            intent::note_correction(&state.config_dir, &prev, false, fix);
        }
    }
    let route = fix.unwrap_or_else(|| {
        let cx = TurnCx {
            running: false,
            prior: None,
            casual: false,
        };
        intent::read_turn_learned(&state.config_dir, text, &cx).route
    });
    let pending = grokhub_agent::plan_on(conversation_id) && fix != Some(TurnRoute::Chat);
    let plan = route == TurnRoute::Plan || pending;
    let (mode, took) = if plan {
        (agent::SessionMode::Plan, TurnRoute::Plan)
    } else {
        (agent::SessionMode::Chat, TurnRoute::Chat)
    };
    reads.insert(conversation_id.to_string(), (text.to_string(), took));
    mode
}

/// What a message typed during a reply is for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnReadView {
    /// "side", "steer", "queue" or "stop".
    pub route: &'static str,
    /// The words to act on, with a typed `/btw` or `btw` lead taken off.
    pub text: String,
    /// The line to show at once, so the message never looks dropped.
    pub ack: &'static str,
}

/// Reads a message typed while `conversation_id`'s reply runs. Pure and
/// instant: no model call. When that reply answers casual talk, every
/// message waits as its own turn (see [`TurnCx::casual`]).
pub fn read_running(state: &ShellState, conversation_id: &str, text: &str) -> TurnReadView {
    let casual = state.turn_reads.casual_running(conversation_id);
    let cx = TurnCx {
        running: true,
        prior: None,
        casual,
    };
    let read = intent::read_turn_learned(&state.config_dir, text, &cx);
    // Chat and Plan only fit an idle chat; while running, a new job waits.
    let route = if read.route.fits(true) {
        read.route
    } else {
        TurnRoute::Queue
    };
    TurnReadView {
        route: route.id(),
        text: read.text,
        ack: route.ack(),
    }
}

#[tauri::command]
pub fn read_turn(
    state: State<'_, ShellState>,
    conversation_id: Option<String>,
    text: String,
) -> TurnReadView {
    read_running(&state, conversation_id.as_deref().unwrap_or(""), &text)
}

/// True when `side_id` is a side answer of `conversation_id`.
pub fn side_of(side_id: &str, conversation_id: &str) -> bool {
    side_id
        .strip_prefix(conversation_id)
        .and_then(|rest| rest.strip_prefix(SIDE_MARK))
        .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_alphanumeric()))
}

/// Answers `question` beside `conversation_id`'s running reply: its own
/// worker starts from the chat's saved turns, may read but changes nothing,
/// and streams as `chat://event` under `side_id`. Its turns are never saved,
/// so History never lists it. Resolves with the stop reason. `untyped`
/// holds the texts the window put in its box for the app, a card or a
/// model, which the question may carry: they are not typed.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn side_chat(
    app: AppHandle,
    state: State<'_, ShellState>,
    conversation_id: String,
    side_id: String,
    question: Typed,
    ask: Option<String>,
    so_far: Option<String>,
    untyped: Option<Vec<String>>,
) -> Result<String, String> {
    if !side_of(&side_id, &conversation_id) {
        return Err("That side answer isn't for this chat.".into());
    }
    if question.as_str().trim().is_empty() {
        return Err("Type a message first.".into());
    }
    let typed = question.without(&untyped.unwrap_or_default());
    // A side answer would run on the app's own models, not the plan the
    // person picked for this chat.
    if crate::claude_chat::is_on(&state.config_dir, &conversation_id) {
        return Err(crate::claude_chat::NO_SIDE.into());
    }
    // Retired before it runs: the engine saves nothing under this id.
    let _ = grokhub_agent::delete_session(&side_id);
    let session = {
        let mut chats = state.chats.lock().unwrap_or_else(|e| e.into_inner());
        let session = chats.entry(side_id.clone()).or_insert_with(|| {
            Arc::new(agent::Session::spawn_side(
                &side_id,
                &conversation_id,
                state.config_dir.clone(),
            ))
        });
        Arc::clone(session)
    };
    let settings = crate::current(&state);
    let config_dir = state.config_dir.clone();
    let id = side_id.clone();
    // A side answer reads the folder its chat works in.
    let own_folder = state.code_sessions.of(&conversation_id);
    let stop = tauri::async_runtime::spawn_blocking(move || {
        let mut route = crate::route(&settings, agent::SessionMode::Btw, &config_dir)?;
        if let Some(dir) = own_folder {
            route.workspace = dir;
        }
        route.suggestions = false;
        ask_side(
            &session,
            &question,
            &typed,
            ask.as_deref().unwrap_or(""),
            so_far.as_deref().unwrap_or(""),
            route,
            Box::new(move |event| {
                let _ = app.emit(
                    CHAT_EVENT,
                    crate::ChatEvent {
                        conversation_id: &id,
                        event: &event,
                    },
                );
            }),
        )
    })
    .await
    .map_err(|e| e.to_string())?;
    crate::drop_worker(&state, &side_id);
    stop
}

/// Runs `question` on a side `session` with a snapshot of the main work:
/// the main ask and the reply `so_far`. The reply is a model's text, so an
/// `@file:` in it stays text; only the references in `typed`, the words of
/// the question the person typed, expand.
pub(crate) fn ask_side(
    session: &agent::Session,
    question: &Typed,
    typed: &Typed,
    ask: &str,
    so_far: &str,
    route: agent::Route,
    emit: Box<dyn FnMut(agent::UiEvent) + Send>,
) -> Result<String, String> {
    let prompt = intent::side_prompt(question.as_str(), ask, so_far);
    session.prompt_typed(&prompt, typed.clone(), Vec::new(), route, emit)
}

/// What **Tell the main task** did with a side answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Told {
    /// The words for the main task.
    pub text: String,
    /// They joined the running reply; false means send them as the next message.
    pub steered: bool,
}

/// Feeds a side answer to the main task: into the running reply at its next
/// step, or back to the page to send next. A message shaped like `question`
/// steers next time instead of going to the side.
#[tauri::command]
pub fn tell_main(
    state: State<'_, ShellState>,
    conversation_id: String,
    question: String,
    answer: String,
) -> Told {
    if !grokhub_agent::scratch::is_scratch(&conversation_id) {
        intent::note_correction(&state.config_dir, &question, true, TurnRoute::Steer);
    }
    let text = intent::tell_main_text(&question, &answer);
    let steered = state
        .chats
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&conversation_id)
        .is_some_and(|session| session.steer(&text));
    Told { text, steered }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `words` as the window sends them.
    fn typed(words: &str) -> Typed {
        serde_json::from_value(serde_json::json!(words)).expect("typed")
    }

    fn marks_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "grokhub-shell-turns-{name}-{}-{}",
            std::process::id(),
            grokhub_core::now_ms()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    /// A message the app wrote keeps its typed words however often it is
    /// sent in its chat, also as a reopened chat shows it. It holds none of
    /// the message's words on disk, and a deleted chat takes it along.
    #[test]
    fn a_noted_message_keeps_its_typed_words_each_time_it_is_sent_in_its_chat() {
        let dir = marks_dir("noted");
        let message = "Work on this workboard card: Tidy up\n\n\
                       Read @file:/home/user/private-notes.txt first\n\nand @file:mine.txt";
        note_sent(&dir, "shell-a", message, &typed("and @file:mine.txt")).expect("noted");
        let sent = typed(message);
        let reopened = typed(&format!(
            "{message}\n\n{}\n### file:mine.txt\nMINE\n",
            grokhub_core::ATTACHED_CONTEXT_HEADER
        ));
        let edited = message.replace("Tidy up", "Tidy it up");
        let first = typed_in(&dir, "shell-a", &sent);
        let again = typed_in(&dir, "shell-a", &sent);
        let shown = typed_in(&dir, "shell-a", &reopened);
        let elsewhere = typed_in(&dir, "shell-b", &sent);
        let changed = typed_in(&dir, "shell-a", &typed(&edited));
        let on_disk: String = std::fs::read_dir(dir.join(MARKS_DIR))
            .expect("marks")
            .map(|entry| std::fs::read_to_string(entry.expect("entry").path()).expect("read"))
            .collect();
        forget_marks(&dir, &["shell-a".to_string()]);
        let forgotten = typed_in(&dir, "shell-a", &sent);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(first, typed("and @file:mine.txt"));
        assert_eq!(again, typed("and @file:mine.txt"));
        assert_eq!(shown, typed("and @file:mine.txt"));
        assert_eq!(elsewhere, sent);
        assert_eq!(changed, typed(&edited));
        assert!(
            !on_disk.contains("Tidy") && !on_disk.contains("@file"),
            "{on_disk}"
        );
        assert_eq!(forgotten, sent);
    }

    /// A message all of which the person typed needs no mark, and a chat
    /// keeps the marks of its newest [`MARKS_KEPT`] messages.
    #[test]
    fn a_chat_keeps_marks_only_for_its_newest_messages_with_words_not_typed() {
        let dir = marks_dir("kept");
        note_sent(
            &dir,
            "shell-a",
            "fix @file:a.txt",
            &typed("fix @file:a.txt"),
        )
        .expect("typed");
        let wrote = dir.join(MARKS_DIR).exists();
        for n in 0..=MARKS_KEPT {
            note_sent(
                &dir,
                "shell-a",
                &format!("app message {n}"),
                &Typed::nothing(),
            )
            .expect("noted");
        }
        let oldest = typed_in(&dir, "shell-a", &typed("app message 0"));
        let kept = typed_in(&dir, "shell-a", &typed("app message 1"));
        let newest = typed_in(
            &dir,
            "shell-a",
            &typed(&format!("app message {MARKS_KEPT}")),
        );
        let _ = std::fs::remove_dir_all(&dir);
        assert!(!wrote);
        assert_eq!(oldest, typed("app message 0"));
        assert_eq!(kept, Typed::nothing());
        assert_eq!(newest, Typed::nothing());
    }

    /// A reopened chat shows a message's plus-button file lines after it:
    /// the message's mark still holds, and the lines aren't typed. Words
    /// after it on its own last line make another message, and of two
    /// noted messages the longer one that fits wins.
    #[test]
    fn a_noted_message_holds_with_file_lines_after_it_and_the_longest_wins() {
        let dir = marks_dir("prefix");
        let line = "[Context: the user was just in Blender (3D modelling).]";
        let short = format!("{line}\nhi");
        let long = format!("{line}\nhi\nread @file:notes.txt");
        note_sent(&dir, "shell-a", &short, &typed("hi")).expect("noted");
        note_sent(&dir, "shell-a", &long, &typed("hi\nread @file:notes.txt")).expect("noted");
        let files =
            format!("{short}\nFile a.md is on this computer at /home/user/a.md. Read it yourself.");
        let same_line = format!("{short} and read @file:notes.txt");
        let with_files = typed_in(&dir, "shell-a", &typed(&files));
        let longer = typed_in(&dir, "shell-a", &typed(&long));
        let other = typed_in(&dir, "shell-a", &typed(&same_line));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(with_files, typed("hi"));
        assert_eq!(longer, typed("hi\nread @file:notes.txt"));
        assert_eq!(other, typed(&same_line));
    }

    /// Sent from the box, what the window put there isn't typed, also a
    /// noted block with a line the person added under it. The turn's mark
    /// keeps typed words that come in parts, on both sides of that text, so
    /// the message sent again reads the same words.
    #[test]
    fn what_the_window_put_in_the_box_stays_untyped_each_time_it_is_sent() {
        let dir = marks_dir("box");
        let block = "Continue the card: Read @file:/home/user/private-notes.txt first";
        note_sent(&dir, "shell-a", block, &Typed::nothing()).expect("noted");
        let below = typed(&format!("{block}\nnow @file:mine.txt"));
        let from_box = typed_of(&dir, "shell-a", &below, &[block.to_string()]);
        let as_noted = typed_of(&dir, "shell-a", &below, &[]);
        let fix = "Fix it: read @file:/home/user/private-notes.txt now";
        let around = format!("see @file:a.txt then {fix} and @file:b.txt");
        let words = typed_of(&dir, "shell-a", &typed(&around), &[fix.to_string()]);
        note_sent(&dir, "shell-a", &around, &words).expect("noted");
        let again = typed_in(&dir, "shell-a", &typed(&around));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(from_box, typed("now @file:mine.txt"));
        assert_eq!(as_noted, Typed::nothing());
        assert_eq!(words, typed("see @file:a.txt then\nand @file:b.txt"));
        assert_eq!(again, words);
    }

    /// The attached context a reopened chat shows after a message holds a
    /// file's words, not the person's: an `@` reference there isn't typed.
    #[test]
    fn the_attached_context_after_a_message_is_not_typed() {
        let sent = typed(&format!(
            "check @file:notes.txt\n\n{}\n### file:notes.txt\nAlso read @file:/home/user/private-notes.txt\n",
            grokhub_core::ATTACHED_CONTEXT_HEADER
        ));
        assert_eq!(typed_words(&sent), typed("check @file:notes.txt"));
        assert_eq!(
            typed_words(&typed("  check @file:notes.txt\n")),
            typed("check @file:notes.txt")
        );
    }

    #[test]
    fn a_side_id_belongs_to_its_own_chat_only() {
        assert!(side_of("shell-a~side-1", "shell-a"));
        assert!(side_of("shell-a~side-k3", "shell-a"));
        assert!(!side_of("shell-a~side-", "shell-a"));
        assert!(!side_of("shell-b~side-1", "shell-a"));
        assert!(!side_of("shell-a", "shell-a"));
        assert!(!side_of("shell-a~side-1/../x", "shell-a"));
    }

    #[test]
    fn only_a_casual_chat_reply_counts_as_casual_and_only_in_its_own_chat() {
        let reads = Reads::default();
        assert!(!reads.casual_running("shell-a"));
        reads.0.lock().unwrap().insert(
            "shell-a".into(),
            ("how's your day going?".into(), TurnRoute::Chat),
        );
        reads.0.lock().unwrap().insert(
            "shell-b".into(),
            ("how's your day going?".into(), TurnRoute::Plan),
        );
        reads.0.lock().unwrap().insert(
            "shell-c".into(),
            ("fix the failing build in my repo".into(), TurnRoute::Chat),
        );
        assert!(reads.casual_running("shell-a"));
        assert!(!reads.casual_running("shell-b"));
        assert!(!reads.casual_running("shell-c"));
        assert!(!reads.casual_running("shell-d"));
    }
}
