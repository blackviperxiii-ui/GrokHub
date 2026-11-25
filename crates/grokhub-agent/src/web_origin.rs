//! Where a run's words came from: a person at this computer, or a paired
//! browser ([`RunOrigin`]). The origin belongs to the run. It is set where a
//! run starts ([`start_run`]: a browser's chat when the browser takes it, a
//! browser's Run now, a run of a job or a card a browser wrote) and kept on
//! file ([`RUNS_FILE`]), so the run's chat stays a browser's after a restart
//! too. Everything the run starts takes it on: a helper runs as
//! `<chat>/<helper>`, a tool that runs other tools on its own thread (a
//! routine's steps) runs them in the caller's run ([`RunScope`]), and a job
//! or a Workboard card whose words a browser run writes is marked
//! ([`JOBS_FILE`], [`CARDS_FILE`]), so each later run of it starts as a
//! browser run too, until a person saves its words again at this computer.
//!
//! Three things decide from the origin, each in one place:
//! - who wrote a job's or a card's words: [`note_job_write`] and
//!   [`note_card_write`], which every agent writer calls before it saves;
//! - the desktop: [`desktop_allowed`], so a browser run has none;
//! - which chats a run may read: [`Readable`], so a browser run reads only
//!   the chats a browser started ([`CHATS_FILE`]).
//!
//! Each file changes under its own lock file, so the app and the background
//! service never lose each other's change. A file that can't be read gives
//! no run, job or card the desktop, and a browser no chat to read.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use serde::de::DeserializeOwned;
use serde::Serialize;

/// Who a run's words came from.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RunOrigin {
    /// A person at this computer: the window, the scheduler, the agent of a
    /// chat started here.
    #[default]
    Window,
    /// A paired browser, or a run that started from one's words.
    Browser,
}

impl RunOrigin {
    /// Both at once: a browser's words anywhere make it a browser's.
    pub fn and(self, other: Self) -> Self {
        if self == Self::Browser || other == Self::Browser {
            Self::Browser
        } else {
            Self::Window
        }
    }
}

/// The jobs whose words a browser run wrote, next to `automations.json`.
pub const JOBS_FILE: &str = "automations-web.json";
/// The Workboard cards whose words a browser run wrote, next to
/// `workboard.json`.
pub const CARDS_FILE: &str = "workboard-web.json";
/// The chats a paired browser started: the ones it may take and read.
pub const CHATS_FILE: &str = "web-chats.json";
/// The runs that started from a browser's words, by chat or session.
pub const RUNS_FILE: &str = "web-runs.json";
/// The newest ids [`CHATS_FILE`] and [`RUNS_FILE`] keep. An older one stays
/// while its chat is saved here or its run goes on in this app; one dropped
/// has no chat left to take, read or type in.
const KEEP_CHATS: usize = 4096;

/// The browser runs started in this process, by chat or session.
static RUNS: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());

thread_local! {
    /// The run whose tool is running on this thread.
    static ON_THREAD: RefCell<Option<String>> = const { RefCell::new(None) };
}

fn live() -> MutexGuard<'static, BTreeSet<String>> {
    RUNS.lock().unwrap_or_else(|err| err.into_inner())
}

/// The chat or run a session is part of: a helper's is `<chat>/<helper>`.
fn root(session: &str) -> &str {
    session.split('/').next().unwrap_or(session)
}

/// `file`'s lock, held while it is read, changed and written.
fn lock(config_dir: &Path, file: &str) -> Result<grokhub_core::work_file::BoardLock, String> {
    grokhub_core::work_file::lock_board(&config_dir.join(file)).map_err(|e| format!("{file}: {e}"))
}

/// `file` as `T`. No file is `T`'s default; one that can't be read or
/// parsed is an error, so nothing writes over it.
fn read<T: DeserializeOwned + Default>(config_dir: &Path, file: &str) -> Result<T, String> {
    match std::fs::read_to_string(config_dir.join(file)) {
        Ok(raw) => serde_json::from_str(&raw).map_err(|e| format!("{file}: {e}")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(T::default()),
        Err(e) => Err(format!("{file}: {e}")),
    }
}

fn write(config_dir: &Path, file: &str, value: &impl Serialize) -> Result<(), String> {
    let raw = serde_json::to_string(value).map_err(|e| e.to_string())?;
    grokhub_core::pulse_writer::write_atomic(&config_dir.join(file), &raw)
}

/// Keeps `id` in `file`, newest last. Past [`KEEP_CHATS`] ids the oldest
/// go, but never one whose chat is saved here or whose browser run goes on
/// in this app.
fn remember(config_dir: &Path, file: &str, id: &str) -> Result<(), String> {
    let _lock = lock(config_dir, file)?;
    let mut ids: Vec<String> = read(config_dir, file)?;
    if ids.iter().any(|kept| kept == id) {
        return Ok(());
    }
    ids.push(id.to_string());
    let over = ids.len().saturating_sub(KEEP_CHATS);
    if over > 0 {
        let live = live().clone();
        let _config = crate::perm::ConfigGuard::set(config_dir);
        let saved = |id: &str| crate::session::session_file(id).is_ok_and(|path| path.exists());
        let mut at = 0;
        ids.retain(|kept| {
            at += 1;
            at > over || live.contains(kept) || saved(kept)
        });
    }
    write(config_dir, file, &ids)
}

/// Run `session` starts with `origin`. A browser run stays one: in this
/// process at once, and on file for every later run in the same chat or
/// session, after a restart too. A run whose origin can't be kept on file
/// should not start.
pub fn start_run(config_dir: &Path, session: &str, origin: RunOrigin) -> Result<(), String> {
    let root = root(session).trim();
    if origin == RunOrigin::Window || root.is_empty() {
        return Ok(());
    }
    live().insert(root.to_string());
    remember(config_dir, RUNS_FILE, root)
}

/// The origin of the run `session` is part of. `None` is the run whose tool
/// is running on this thread ([`RunScope`]), or this computer's when none is.
pub fn of(session: Option<&str>) -> RunOrigin {
    of_in(&crate::perm::config_dir(), session)
}

/// [`of`], with the browser runs and chats read from `config_dir`.
pub fn of_in(config_dir: &Path, session: Option<&str>) -> RunOrigin {
    let session = match session {
        Some(session) => Some(session.to_string()),
        None => ON_THREAD.with(|on| on.borrow().clone()),
    };
    let Some(session) = session.filter(|s| !s.is_empty()) else {
        return RunOrigin::Window;
    };
    let root = root(&session);
    let on_file = |file: &str| match read::<Vec<String>>(config_dir, file) {
        Ok(ids) => ids.iter().any(|id| id == root),
        Err(_) => true,
    };
    if live().contains(root) || on_file(RUNS_FILE) || on_file(CHATS_FILE) {
        RunOrigin::Browser
    } else {
        RunOrigin::Window
    }
}

/// Run `session`'s tool is running on this thread, so a tool it runs here
/// with no session of its own (a routine's step) is part of the same run.
/// Dropping the scope restores the run before it.
pub struct RunScope {
    prev: Option<String>,
}

impl RunScope {
    /// `None` keeps the run already on this thread.
    pub fn enter(session: Option<&str>) -> Self {
        let prev = ON_THREAD.with(|on| {
            let prev = on.borrow().clone();
            if let Some(session) = session {
                *on.borrow_mut() = Some(session.to_string());
            }
            prev
        });
        Self { prev }
    }
}

impl Drop for RunScope {
    fn drop(&mut self) {
        let prev = self.prev.take();
        ON_THREAD.with(|on| *on.borrow_mut() = prev);
    }
}

/// Whether a run from `origin` may use the desktop. The one check: a browser
/// run gets no desktop tools and no look at the screen.
pub fn desktop_allowed(origin: RunOrigin) -> bool {
    origin == RunOrigin::Window
}

/// What [`JOBS_FILE`] keeps for each job whose words a browser run wrote:
/// the fingerprint of the words a person saved at this computer since, or
/// none. A run of the job is a browser's unless the words it runs are those,
/// so the order in which a browser's save and a person's save land never
/// leaves a browser's words with the desktop.
type JobMarks = BTreeMap<String, Option<String>>;

fn fingerprint(words: &str) -> String {
    grokhub_core::model_download::sha256_hex(words.as_bytes())
}

fn change_jobs(config_dir: &Path, edit: impl FnOnce(&mut JobMarks)) -> Result<(), String> {
    let _lock = lock(config_dir, JOBS_FILE)?;
    let mut marks: JobMarks = read(config_dir, JOBS_FILE)?;
    let before = marks.clone();
    edit(&mut marks);
    if marks == before {
        return Ok(());
    }
    write(config_dir, JOBS_FILE, &marks)
}

/// A browser run is about to save job `id`'s words: from now on the job
/// runs as a browser's. Said before the save, so no run finds them unmarked.
pub fn mark_job(config_dir: &Path, id: &str) -> Result<(), String> {
    change_jobs(config_dir, |marks| {
        marks.insert(id.to_string(), None);
    })
}

/// A person at this computer saved `words` as job `id`'s: a job a browser
/// wrote runs as theirs while it runs exactly those words.
pub fn job_saved_here(config_dir: &Path, id: &str, words: &str) -> Result<(), String> {
    change_jobs(config_dir, |marks| {
        if let Some(mark) = marks.get_mut(id) {
            *mark = Some(fingerprint(words));
        }
    })
}

/// The run `session` writes `words` as job `id`'s, which were `before` (none
/// for a new job), as every agent job writer says before it saves them. A
/// browser run's write marks the job. A window run's write of a job that
/// runs a person's words keeps it theirs; it never takes a browser's mark off.
pub fn note_job_write(
    config_dir: &Path,
    id: &str,
    session: Option<&str>,
    before: Option<&str>,
    words: &str,
) -> Result<(), String> {
    match (of_in(config_dir, session), before) {
        (RunOrigin::Browser, _) => mark_job(config_dir, id),
        (RunOrigin::Window, None) => Ok(()),
        (RunOrigin::Window, Some(before)) => {
            let (was, now) = (fingerprint(before), fingerprint(words));
            change_jobs(config_dir, |marks| {
                if let Some(mark) = marks.get_mut(id).filter(|m| m.as_deref() == Some(&was)) {
                    *mark = Some(now);
                }
            })
        }
    }
}

/// Whose words a run of job `id` that runs `words` goes on: a browser's when
/// a browser run wrote the job and a person hasn't saved these words here
/// since, and when [`JOBS_FILE`] can't be read.
pub fn job_origin(config_dir: &Path, id: &str, words: &str) -> RunOrigin {
    match read::<JobMarks>(config_dir, JOBS_FILE) {
        Ok(marks) => match marks.get(id) {
            None => RunOrigin::Window,
            Some(Some(saved)) if *saved == fingerprint(words) => RunOrigin::Window,
            Some(_) => RunOrigin::Browser,
        },
        Err(_) => RunOrigin::Browser,
    }
}

/// The parts of a Workboard card a person saves one at a time at this
/// computer, and a browser run can write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardPart {
    /// Title, detail and goal.
    Words,
    Notes,
    /// The steps, and the next one.
    Steps,
    /// What it waits on.
    Blocker,
}

impl CardPart {
    fn key(self) -> &'static str {
        match self {
            Self::Words => "words",
            Self::Notes => "notes",
            Self::Steps => "steps",
            Self::Blocker => "blocker",
        }
    }
}

/// What [`CARDS_FILE`] keeps for each card part a browser run wrote: a count
/// that only goes up, odd while the part is the browser's. A browser's write
/// makes it odd and new; a person's save makes it even again, but only from
/// the count it saw, so a browser write after that save keeps its mark.
type CardMarks = BTreeMap<String, BTreeMap<String, u64>>;

/// A browser run writes `parts` of card `id`. Said under the board lock,
/// before the card is written, so no run finds those words unmarked.
pub fn mark_card(config_dir: &Path, id: &str, parts: &[CardPart]) -> Result<(), String> {
    let _lock = lock(config_dir, CARDS_FILE)?;
    let mut marks: CardMarks = read(config_dir, CARDS_FILE)?;
    let card = marks.entry(id.to_string()).or_default();
    for part in parts {
        let count = card.entry(part.key().to_string()).or_insert(0);
        *count += if *count % 2 == 1 { 2 } else { 1 };
    }
    write(config_dir, CARDS_FILE, &marks)
}

/// The mark on `part` of card `id` that a person's save sees, read under the
/// board lock with the card it saves; `None` when the part is unmarked (or
/// the file can't be read, which leaves every card marked).
pub fn card_seen(config_dir: &Path, id: &str, part: CardPart) -> Option<u64> {
    let marks: CardMarks = read(config_dir, CARDS_FILE).ok()?;
    marks
        .get(id)
        .and_then(|card| card.get(part.key()))
        .copied()
        .filter(|count| count % 2 == 1)
}

/// A person at this computer saved `part` of card `id`, whose mark was
/// `seen` ([`card_seen`]) when they saved it: the mark comes off unless a
/// browser wrote that part again since.
pub fn card_saved_here(
    config_dir: &Path,
    id: &str,
    part: CardPart,
    seen: Option<u64>,
) -> Result<(), String> {
    let Some(seen) = seen else {
        return Ok(());
    };
    let _lock = lock(config_dir, CARDS_FILE)?;
    let mut marks: CardMarks = read(config_dir, CARDS_FILE)?;
    let Some(count) = marks
        .get_mut(id)
        .and_then(|card| card.get_mut(part.key()))
        .filter(|count| **count == seen)
    else {
        return Ok(());
    };
    *count += 1;
    write(config_dir, CARDS_FILE, &marks)
}

/// The run `session` writes `parts` of card `id`, as every agent card writer
/// says under the board lock before it saves them: a browser run's write
/// marks them. Any other write leaves the marks as they were.
pub fn note_card_write(
    config_dir: &Path,
    id: &str,
    session: Option<&str>,
    parts: &[CardPart],
) -> Result<(), String> {
    match of_in(config_dir, session) {
        RunOrigin::Browser => mark_card(config_dir, id, parts),
        RunOrigin::Window => Ok(()),
    }
}

/// Whose words a run of card `id` goes on: a browser's while any part of it
/// is a browser's, and when [`CARDS_FILE`] can't be read.
pub fn card_origin(config_dir: &Path, id: &str) -> RunOrigin {
    match read::<CardMarks>(config_dir, CARDS_FILE) {
        Ok(marks)
            if !marks
                .get(id)
                .is_some_and(|card| card.values().any(|c| c % 2 == 1)) =>
        {
            RunOrigin::Window
        }
        _ => RunOrigin::Browser,
    }
}

/// The chat ids on file, oldest first. A file that can't be read lists none:
/// a browser run then reads no chat, and no saved chat is a browser's to take.
fn chat_ids(config_dir: &Path) -> Vec<String> {
    read(config_dir, CHATS_FILE).unwrap_or_default()
}

/// A paired browser started (or goes on with) chat `id`.
pub fn note_chat(config_dir: &Path, id: &str) -> Result<(), String> {
    remember(config_dir, CHATS_FILE, id)
}

/// Whether chat `id` is this computer's: saved here and not started in a
/// browser. A chat new to this computer is no one's yet.
pub fn started_here(config_dir: &Path, id: &str) -> bool {
    let _config = crate::perm::ConfigGuard::set(config_dir);
    let saved = crate::session::session_file(id).is_ok_and(|path| path.exists());
    saved && !Readable::for_run(config_dir, RunOrigin::Browser).sees(id)
}

/// The chats a run may read another chat's words from. Every reader of an
/// earlier chat (the agent's chat search, and a browser's Agents board and
/// Reflect) asks this one check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Readable {
    /// A run from this computer: every chat.
    Every,
    /// A browser run: only these, the chats a browser started.
    Only(BTreeSet<String>),
}

impl Readable {
    /// What a run from `origin` may read.
    pub fn for_run(config_dir: &Path, origin: RunOrigin) -> Self {
        match origin {
            RunOrigin::Window => Self::Every,
            RunOrigin::Browser => Self::Only(chat_ids(config_dir).into_iter().collect()),
        }
    }

    /// What the run `session` is part of may read.
    pub fn for_session(config_dir: &Path, session: Option<&str>) -> Self {
        Self::for_run(config_dir, of_in(config_dir, session))
    }

    pub fn sees(&self, chat: &str) -> bool {
        match self {
            Self::Every => true,
            Self::Only(chats) => chats.contains(chat),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gh-web-origin-{name}-{}-{}",
            std::process::id(),
            grokhub_core::now_ms()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    const INBOX: &str = "summarize my inbox";
    const INBOX_FP: &str = "0751050589ed0e198d207b091565d76f629a8f0bc72c3904f6317993dc7c8ac4";
    const SEND: &str = "click Send in Mail";

    /// A job a browser wrote runs as a browser's until a person saves the
    /// words it runs, whichever save lands last: the person's save made
    /// first and the browser's after (the race) still runs as the browser's.
    #[test]
    fn a_job_is_a_browsers_until_a_person_saves_the_words_it_runs() {
        use RunOrigin::{Browser, Window};
        let dir = temp_dir("marks");
        assert_eq!(job_origin(&dir, "auto-1", INBOX), Window);
        mark_job(&dir, "auto-1").unwrap();
        mark_job(&dir, "auto-1").unwrap();
        mark_job(&dir, "auto-2").unwrap();
        let file = || std::fs::read_to_string(dir.join(JOBS_FILE)).unwrap();
        assert_eq!(file(), r#"{"auto-1":null,"auto-2":null}"#);
        assert_eq!(job_origin(&dir, "auto-1", SEND), Browser);
        // The person saves their words here, then the browser's save lands.
        job_saved_here(&dir, "auto-1", INBOX).unwrap();
        assert_eq!(
            file(),
            format!(r#"{{"auto-1":"{INBOX_FP}","auto-2":null}}"#)
        );
        assert_eq!(
            [
                job_origin(&dir, "auto-1", INBOX),
                job_origin(&dir, "auto-1", SEND)
            ],
            [Window, Browser]
        );
        // A browser's write after the person's save takes it back.
        mark_job(&dir, "auto-1").unwrap();
        assert_eq!(job_origin(&dir, "auto-1", INBOX), Browser);
        // Saving here a job no browser wrote leaves no mark.
        job_saved_here(&dir, "auto-3", INBOX).unwrap();
        assert_eq!(file(), r#"{"auto-1":null,"auto-2":null}"#);
        // A file that can't be read: every job is a browser's, and no write
        // goes over it.
        std::fs::write(dir.join(JOBS_FILE), "[not json").unwrap();
        assert_eq!(job_origin(&dir, "auto-3", INBOX), Browser);
        assert!(mark_job(&dir, "auto-3").is_err());
        assert_eq!(file(), "[not json");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A window run's write of a job that runs a person's words keeps it
    /// theirs; it never takes a browser's mark off, and a browser run's
    /// write marks the job.
    #[test]
    fn a_window_runs_write_never_takes_a_browsers_mark_off() {
        use RunOrigin::{Browser, Window};
        let dir = temp_dir("writes");
        start_run(&dir, "shell-web-writes-1", Browser).unwrap();
        let desk = Some("shell-desk-writes-1");
        mark_job(&dir, "auto-1").unwrap();
        job_saved_here(&dir, "auto-1", INBOX).unwrap();
        mark_job(&dir, "auto-2").unwrap();
        let twice = "summarize my inbox twice";
        note_job_write(&dir, "auto-1", desk, Some(INBOX), twice).unwrap();
        note_job_write(&dir, "auto-2", desk, Some(SEND), "click Send twice").unwrap();
        note_job_write(&dir, "auto-4", desk, None, INBOX).unwrap();
        assert_eq!(
            [
                job_origin(&dir, "auto-1", twice),
                job_origin(&dir, "auto-2", "click Send twice"),
                job_origin(&dir, "auto-4", INBOX),
            ],
            [Window, Browser, Window]
        );
        note_job_write(
            &dir,
            "auto-1",
            Some("shell-web-writes-1"),
            Some(twice),
            SEND,
        )
        .unwrap();
        assert_eq!(job_origin(&dir, "auto-1", SEND), Browser);
        assert_eq!(
            std::fs::read_to_string(dir.join(JOBS_FILE)).unwrap(),
            r#"{"auto-1":null,"auto-2":null}"#
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A person's save of a card part takes off only the mark it saw: a
    /// browser's write of that part after the save keeps the card a
    /// browser's, and so does a part the person didn't save.
    #[test]
    fn a_persons_save_takes_off_only_the_card_mark_it_saw() {
        use CardPart::{Notes, Steps, Words};
        use RunOrigin::{Browser, Window};
        let dir = temp_dir("cards");
        let origin = || card_origin(&dir, "card-1");
        assert_eq!((origin(), card_seen(&dir, "card-1", Notes)), (Window, None));
        mark_card(&dir, "card-1", &[Notes]).unwrap();
        assert_eq!(origin(), Browser);
        let seen = card_seen(&dir, "card-1", Notes);
        assert_eq!(seen, Some(1));
        // The browser writes the notes again after the person's save read them.
        mark_card(&dir, "card-1", &[Notes]).unwrap();
        card_saved_here(&dir, "card-1", Notes, seen).unwrap();
        assert_eq!(origin(), Browser);
        let seen = card_seen(&dir, "card-1", Notes);
        assert_eq!(seen, Some(3));
        card_saved_here(&dir, "card-1", Notes, seen).unwrap();
        assert_eq!(origin(), Window);
        // A part the person didn't save stays the browser's.
        mark_card(&dir, "card-1", &[Words, Steps]).unwrap();
        card_saved_here(&dir, "card-1", Words, card_seen(&dir, "card-1", Words)).unwrap();
        assert_eq!(origin(), Browser);
        card_saved_here(&dir, "card-1", Steps, card_seen(&dir, "card-1", Steps)).unwrap();
        assert_eq!(origin(), Window);
        // An older save's count never takes off a newer mark.
        mark_card(&dir, "card-1", &[Notes]).unwrap();
        card_saved_here(&dir, "card-1", Notes, Some(3)).unwrap();
        assert_eq!(origin(), Browser);
        assert_eq!(card_origin(&dir, "card-2"), Window);
        assert_eq!(
            std::fs::read_to_string(dir.join(CARDS_FILE)).unwrap(),
            r#"{"card-1":{"notes":5,"steps":2,"words":2}}"#
        );
        // A file that can't be read: every card is a browser's.
        std::fs::write(dir.join(CARDS_FILE), "[not json").unwrap();
        assert_eq!(card_origin(&dir, "card-2"), Browser);
        assert_eq!(card_seen(&dir, "card-1", Notes), None);
        assert!(mark_card(&dir, "card-2", &[Words]).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A run started from a browser's words is a browser's, and so is a
    /// helper of it, a chat a browser started, a browser run kept on file
    /// (after a restart too), and a tool run on the thread of a browser
    /// run's tool. Nothing else is.
    #[test]
    fn a_run_and_what_it_starts_share_one_origin() {
        let dir = temp_dir("runs");
        start_run(&dir, "shell-auto-web-9-1/helper-2", RunOrigin::Browser).unwrap();
        start_run(&dir, "shell-auto-desk-9-1", RunOrigin::Window).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join(RUNS_FILE)).unwrap(),
            r#"["shell-auto-web-9-1"]"#
        );
        let of = |session: Option<&str>| of_in(&dir, session);
        assert_eq!(of(Some("shell-auto-web-9-1")), RunOrigin::Browser);
        assert_eq!(of(Some("shell-auto-web-9-1/helper-2")), RunOrigin::Browser);
        assert_eq!(of(Some("shell-auto-web-9-10")), RunOrigin::Window);
        assert_eq!(of(Some("shell-auto-desk-9-1")), RunOrigin::Window);
        // On file only, as after a restart.
        note_chat(&dir, "shell-web-chat-9").unwrap();
        std::fs::write(
            dir.join(RUNS_FILE),
            r#"["shell-auto-web-9-1","shell-q-card-9"]"#,
        )
        .unwrap();
        assert_eq!(of(Some("shell-web-chat-9")), RunOrigin::Browser);
        assert_eq!(of(Some("shell-web-chat-9/helper-1")), RunOrigin::Browser);
        assert_eq!(of(Some("shell-q-card-9")), RunOrigin::Browser);
        assert_eq!(of(Some("shell-desk-chat-9")), RunOrigin::Window);
        assert_eq!(of(None), RunOrigin::Window);
        {
            let _run = RunScope::enter(Some("shell-auto-web-9-1"));
            // A step with no session of its own, run by that run's tool.
            let _step = RunScope::enter(None);
            assert_eq!(of(None), RunOrigin::Browser);
            // A window run's tool on the same thread is its own.
            let _desk = RunScope::enter(Some("shell-desk-chat-9"));
            assert_eq!(of(None), RunOrigin::Window);
        }
        assert_eq!(of(None), RunOrigin::Window);
        // A file that can't be read: every run is a browser's.
        std::fs::write(dir.join(RUNS_FILE), "[not json").unwrap();
        assert_eq!(of(Some("shell-desk-chat-9")), RunOrigin::Browser);
        assert!(start_run(&dir, "shell-auto-web-9-2", RunOrigin::Browser).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_browser_run_reads_only_the_chats_a_browser_started() {
        let dir = temp_dir("read");
        note_chat(&dir, "shell-web-1").unwrap();
        let web = Readable::for_run(&dir, RunOrigin::Browser);
        assert_eq!(web, Readable::Only(["shell-web-1".to_string()].into()));
        assert!(web.sees("shell-web-1"));
        assert!(!web.sees("shell-desk-1"));
        assert!(Readable::for_run(&dir, RunOrigin::Window).sees("shell-desk-1"));
        // Noting twice keeps one row.
        note_chat(&dir, "shell-web-1").unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join(CHATS_FILE)).unwrap(),
            r#"["shell-web-1"]"#
        );
        // A file that can't be read: a browser run reads no chat, and no
        // chat gets the desktop.
        std::fs::write(dir.join(CHATS_FILE), "[not json").unwrap();
        assert_eq!(
            Readable::for_run(&dir, RunOrigin::Browser),
            Readable::Only(BTreeSet::new())
        );
        assert_eq!(of_in(&dir, Some("shell-desk-1")), RunOrigin::Browser);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_the_newest_web_chats_are_kept() {
        let dir = temp_dir("keep");
        let ids: Vec<String> = (0..KEEP_CHATS).map(|n| format!("shell-w-{n}")).collect();
        std::fs::write(dir.join(CHATS_FILE), serde_json::to_string(&ids).unwrap()).unwrap();
        note_chat(&dir, "shell-w-new").unwrap();
        let kept = chat_ids(&dir);
        assert_eq!(kept.len(), KEEP_CHATS);
        assert_eq!(kept.first().map(String::as_str), Some("shell-w-1"));
        assert_eq!(kept.last().map(String::as_str), Some("shell-w-new"));
        assert!(!Readable::for_run(&dir, RunOrigin::Browser).sees("shell-w-0"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A browser chat older than the newest [`KEEP_CHATS`] stays a
    /// browser's while it is saved here or its run goes on: a message typed
    /// in it gets no desktop, and the window can't take it. Only an id with
    /// no chat goes.
    #[test]
    fn an_old_browser_chat_with_a_saved_chat_stays_a_browsers() {
        let dir = temp_dir("old");
        let _config = crate::perm::ConfigGuard::set(&dir);
        let saved = crate::session::session_file("shell-w-saved").unwrap();
        std::fs::create_dir_all(saved.parent().unwrap()).unwrap();
        std::fs::write(&saved, "").unwrap();
        live().insert("shell-w-live".to_string());
        // Three old ids kept past the cap before, then the newest.
        let mut ids: Vec<String> = ["shell-w-gone", "shell-w-saved", "shell-w-live"]
            .map(String::from)
            .into();
        ids.extend((0..KEEP_CHATS - 1).map(|n| format!("shell-w-{n}")));
        for file in [CHATS_FILE, RUNS_FILE] {
            std::fs::write(dir.join(file), serde_json::to_string(&ids).unwrap()).unwrap();
        }
        note_chat(&dir, "shell-w-new").unwrap();
        start_run(&dir, "shell-w-new", RunOrigin::Browser).unwrap();
        for file in [CHATS_FILE, RUNS_FILE] {
            let kept: Vec<String> = read(&dir, file).unwrap();
            assert_eq!(kept.len(), KEEP_CHATS + 2, "{file}");
            assert_eq!(kept[..2], ["shell-w-saved", "shell-w-live"], "{file}");
            assert_eq!(kept.last().map(String::as_str), Some("shell-w-new"));
        }
        let origin = |id: &str| of_in(&dir, Some(id));
        assert_eq!(
            [
                origin("shell-w-saved"),
                origin("shell-w-live"),
                origin("shell-w-gone")
            ],
            [RunOrigin::Browser, RunOrigin::Browser, RunOrigin::Window]
        );
        assert!(!desktop_allowed(origin("shell-w-saved")));
        assert!(!started_here(&dir, "shell-w-saved"));
        live().remove("shell-w-live");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
