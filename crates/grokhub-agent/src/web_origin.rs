//! Where a run's words came from: a person at this computer, or a paired
//! browser ([`RunOrigin`]). The origin belongs to the run. It is set where a
//! run starts ([`start_run`]: a browser's chat when the browser takes it, a
//! browser's Run now, a run of a job a browser wrote), and everything the run
//! starts takes it on: a helper runs as `<chat>/<helper>`, a tool that runs
//! other tools on its own thread (a routine's steps) runs them in the
//! caller's run ([`RunScope`]), and a job a browser run writes is marked
//! ([`JOBS_FILE`]), so each later run of it starts as a browser run too. A
//! chat a browser started ([`CHATS_FILE`]) stays a browser's, in every run
//! in it, after a restart too.
//!
//! Three things decide from the origin, each in one place:
//! - who wrote a job's words: [`note_job_write`], which every job writer
//!   calls before it saves them;
//! - the desktop: [`desktop_allowed`], so a browser run has none;
//! - which chats a run may read: [`Readable`], so a browser run reads only
//!   the chats a browser started.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Mutex;

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
/// The chats a paired browser started.
pub const CHATS_FILE: &str = "web-chats.json";
/// The newest chat ids kept. An older browser chat counts as the window's
/// for taking it ([`started_here`]), so a browser can't pick it up again,
/// and a browser run no longer reads it.
const KEEP_CHATS: usize = 4096;

/// One change to [`JOBS_FILE`] at a time.
static JOBS: Mutex<()> = Mutex::new(());
/// One change to [`CHATS_FILE`] at a time.
static CHATS: Mutex<()> = Mutex::new(());
/// The browser runs started in this process, by session.
static RUNS: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());

thread_local! {
    /// The run whose tool is running on this thread.
    static ON_THREAD: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// The chat or run a session is part of: a helper's is `<chat>/<helper>`.
fn root(session: &str) -> &str {
    session.split('/').next().unwrap_or(session)
}

/// Run `session` starts with `origin`. A browser run stays one until the app
/// quits, so a helper or a background step that outlives a turn keeps it.
pub fn start_run(session: &str, origin: RunOrigin) {
    if origin == RunOrigin::Browser && !session.is_empty() {
        RUNS.lock()
            .unwrap_or_else(|err| err.into_inner())
            .insert(root(session).to_string());
    }
}

/// The origin of the run `session` is part of. `None` is the run whose tool
/// is running on this thread ([`RunScope`]), or this computer's when none is.
pub fn of(session: Option<&str>) -> RunOrigin {
    of_in(&crate::perm::config_dir(), session)
}

/// [`of`], with the chats a browser started read from `config_dir`.
pub fn of_in(config_dir: &Path, session: Option<&str>) -> RunOrigin {
    let session = match session {
        Some(session) => Some(session.to_string()),
        None => ON_THREAD.with(|on| on.borrow().clone()),
    };
    let Some(session) = session.filter(|s| !s.is_empty()) else {
        return RunOrigin::Window;
    };
    let root = root(&session);
    let started = RUNS
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .contains(root);
    if started || chat_started_on_web(config_dir, root) {
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

/// The jobs a browser run wrote. No file is none; one that can't be read is
/// an error, so a run treats its job as a browser's.
fn job_ids(config_dir: &Path) -> Result<Vec<String>, String> {
    match std::fs::read_to_string(config_dir.join(JOBS_FILE)) {
        Ok(raw) => serde_json::from_str(&raw).map_err(|e| format!("{JOBS_FILE}: {e}")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e.to_string()),
    }
}

/// Job `id`'s words are now `origin`'s: a browser's marks the job, and this
/// computer's takes the mark off. Only a person saving the words at this
/// computer passes [`RunOrigin::Window`]; an agent's write goes through
/// [`note_job_write`].
pub fn note_job(config_dir: &Path, id: &str, origin: RunOrigin) -> Result<(), String> {
    let web = origin == RunOrigin::Browser;
    let _store = JOBS.lock().unwrap_or_else(|err| err.into_inner());
    let mut ids = job_ids(config_dir)?;
    if ids.iter().any(|kept| kept == id) == web {
        return Ok(());
    }
    ids.retain(|kept| kept != id);
    if web {
        ids.push(id.to_string());
    }
    let raw = serde_json::to_string(&ids).map_err(|e| e.to_string())?;
    grokhub_core::pulse_writer::write_atomic(&config_dir.join(JOBS_FILE), &raw)
}

/// The run `session` writes job `id`'s words, as every agent job writer says
/// before it saves them. A browser run's write marks the job, so no run ever
/// finds a browser's words unmarked. Any other write leaves the mark as it
/// was: an agent's write never takes one off.
pub fn note_job_write(config_dir: &Path, id: &str, session: Option<&str>) -> Result<(), String> {
    match of_in(config_dir, session) {
        RunOrigin::Browser => note_job(config_dir, id, RunOrigin::Browser),
        RunOrigin::Window => Ok(()),
    }
}

/// Whose words job `id` runs: a browser's when a browser run wrote them, and
/// when [`JOBS_FILE`] can't be read.
pub fn job_origin(config_dir: &Path, id: &str) -> RunOrigin {
    match job_ids(config_dir) {
        Ok(ids) if !ids.iter().any(|kept| kept == id) => RunOrigin::Window,
        _ => RunOrigin::Browser,
    }
}

/// The chat ids on file, oldest first. A file that can't be read lists none:
/// a browser run then reads no chat, and no saved chat is a browser's to take.
fn chat_ids(config_dir: &Path) -> Vec<String> {
    std::fs::read_to_string(config_dir.join(CHATS_FILE))
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

/// A paired browser started (or goes on with) chat `id`.
pub fn note_chat(config_dir: &Path, id: &str) -> Result<(), String> {
    let _store = CHATS.lock().unwrap_or_else(|err| err.into_inner());
    let mut ids = chat_ids(config_dir);
    if ids.iter().any(|kept| kept == id) {
        return Ok(());
    }
    ids.push(id.to_string());
    let extra = ids.len().saturating_sub(KEEP_CHATS);
    ids.drain(..extra);
    let raw = serde_json::to_string(&ids).map_err(|e| e.to_string())?;
    grokhub_core::pulse_writer::write_atomic(&config_dir.join(CHATS_FILE), &raw)
}

/// Whether a paired browser started chat `id`.
pub fn chat_started_on_web(config_dir: &Path, id: &str) -> bool {
    chat_ids(config_dir).iter().any(|kept| kept == id)
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

    #[test]
    fn a_mark_is_kept_once_and_a_broken_file_marks_every_job() {
        let dir = temp_dir("marks");
        assert_eq!(job_origin(&dir, "auto-1"), RunOrigin::Window);
        note_job(&dir, "auto-1", RunOrigin::Browser).unwrap();
        note_job(&dir, "auto-1", RunOrigin::Browser).unwrap();
        note_job(&dir, "auto-2", RunOrigin::Browser).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join(JOBS_FILE)).unwrap(),
            r#"["auto-1","auto-2"]"#
        );
        note_job(&dir, "auto-1", RunOrigin::Window).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join(JOBS_FILE)).unwrap(),
            r#"["auto-2"]"#
        );
        assert_eq!(job_origin(&dir, "auto-1"), RunOrigin::Window);
        std::fs::write(dir.join(JOBS_FILE), "[not json").unwrap();
        assert_eq!(job_origin(&dir, "auto-1"), RunOrigin::Browser);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A run started from a browser's words is a browser's, and so is a
    /// helper of it, a chat a browser started (after a restart too), and a
    /// tool run on the thread of a browser run's tool. Nothing else is.
    #[test]
    fn a_run_and_what_it_starts_share_one_origin() {
        let dir = temp_dir("runs");
        start_run("shell-auto-web-9-1", RunOrigin::Browser);
        start_run("shell-auto-desk-9-1", RunOrigin::Window);
        let of = |session: Option<&str>| of_in(&dir, session);
        assert_eq!(of(Some("shell-auto-web-9-1")), RunOrigin::Browser);
        assert_eq!(of(Some("shell-auto-web-9-1/helper-2")), RunOrigin::Browser);
        assert_eq!(of(Some("shell-auto-web-9-10")), RunOrigin::Window);
        assert_eq!(of(Some("shell-auto-desk-9-1")), RunOrigin::Window);
        // On file only, as after a restart.
        note_chat(&dir, "shell-web-chat-9").unwrap();
        assert_eq!(of(Some("shell-web-chat-9")), RunOrigin::Browser);
        assert_eq!(of(Some("shell-web-chat-9/helper-1")), RunOrigin::Browser);
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
        // A file that can't be read: a browser run reads no chat.
        std::fs::write(dir.join(CHATS_FILE), "[not json").unwrap();
        assert_eq!(
            Readable::for_run(&dir, RunOrigin::Browser),
            Readable::Only(BTreeSet::new())
        );
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
        assert!(!chat_started_on_web(&dir, "shell-w-0"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
