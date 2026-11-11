//! The Code tab terminal: the user's own shell on a pseudo-terminal, started in
//! the project folder. Output passes through the secret redactor (the
//! project's own secret values, then the usual token patterns) before it
//! reaches the window. Project secrets are never put in this shell's
//! environment, and what is typed or shown is never logged. Mobile builds have
//! no terminal.
//!
//! This is a normal shell: a `git push` typed here is not checked by the Code
//! tab's push guard (`code_ship.rs`), which covers the tab's own buttons.

use grokhub_core::redact::{redact_held_secrets, redact_secret_words};
use serde::Serialize;
use std::path::Path;

/// The Tauri event the terminal's output travels on.
pub const EVENT: &str = "code-term";
/// The most bytes one `write` call sends to the shell.
pub const INPUT_CAP: usize = 65_536;
/// Output with no space or newline in it is held at most this long.
const HOLD_MAX: usize = 512;
/// An unfinished key is held whole up to this length. A longer run with no
/// break in it is no key, and is let out.
const TOKEN_MAX: usize = 4096;
/// What a key starts with. Keep in step with `grokhub_core::redact`.
const KEY_PREFIXES: [&[u8]; 7] = [
    b"sk-",
    b"xai-",
    b"ghp_",
    b"github_pat_",
    b"gho_",
    b"ghu_",
    b"ghs_",
];
/// The words whose next word is hidden.
const BEARER_WORDS: [&[u8]; 2] = [b"Bearer ", b"bearer "];
/// An escape sequence is looked for this far back from a cut.
const ESCAPE_MAX: usize = 64;

/// A piece of redacted output, or the news that the shell ended.
#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Chunk {
    pub id: u32,
    pub text: String,
    pub exited: bool,
}

/// Cuts raw shell output into pieces that are safe to redact: it holds back
/// the last unfinished word (and any unfinished UTF-8 character) so a token
/// split across two reads is still seen whole. A cut never falls inside a
/// project secret, inside a key, or between `Bearer` and its value.
#[derive(Debug, Default)]
pub struct Chunker {
    pending: Vec<u8>,
    held: Vec<String>,
    /// What has been shown so far ends inside a word, so a key prefix at the
    /// start of the next piece is the middle of a name, not a key.
    glued: bool,
}

fn utf8_safe_len(bytes: &[u8]) -> usize {
    match std::str::from_utf8(bytes) {
        Ok(_) => bytes.len(),
        Err(e) if e.error_len().is_none() => e.valid_up_to(),
        Err(_) => bytes.len(),
    }
}

/// A character a key can be made of (the same set the redactor reads).
fn key_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~' | b'+' | b'/' | b'=')
}

/// Where a `needle` begins that starts before `cut` and runs past it, whole
/// or cut short by the end of `hay`.
fn straddling(hay: &[u8], cut: usize, needle: &[u8]) -> Option<usize> {
    let from = cut.saturating_sub(needle.len().saturating_sub(1));
    (from..cut).find(|&at| {
        let n = needle.len().min(hay.len() - at);
        hay[at..at + n] == needle[..n]
    })
}

/// Where a key begins that is still unfinished at `cut`: a key prefix with
/// nothing but key characters up to the cut, or the `Bearer` before them.
fn open_key(hay: &[u8], cut: usize) -> Option<usize> {
    let low = cut.saturating_sub(TOKEN_MAX);
    let run = hay[low..cut]
        .iter()
        .rposition(|b| !key_char(*b))
        .map_or(low, |i| low + i + 1);
    if let Some(word) = BEARER_WORDS.iter().find(|w| hay[..run].ends_with(w)) {
        return Some(run - word.len());
    }
    (run..cut).find(|&at| KEY_PREFIXES.iter().any(|p| hay[at..cut].starts_with(p)))
}

/// The escape sequence at the start of `bytes` (`bytes[0]` is ESC): how long
/// it is, and whether its last byte is there.
fn escape_at(bytes: &[u8]) -> (usize, bool) {
    // A control sequence (`ESC [`) has parameter bytes and one final byte;
    // any other escape has intermediate bytes and one final byte.
    let (mut len, inner, last) = match bytes.get(1) {
        Some(b'[') => (2, 0x20..=0x3f, 0x40..=0x7e),
        _ => (1, 0x20..=0x2f, 0x30..=0x7e),
    };
    while bytes.get(len).is_some_and(|b| inner.contains(b)) {
        len += 1;
    }
    let whole = bytes.get(len).is_some_and(|b| last.contains(b));
    (len + usize::from(whole), whole)
}

/// Where an escape sequence begins that `cut` would split, or that is still
/// unfinished at the end of `hay`.
fn open_escape(hay: &[u8], cut: usize) -> Option<usize> {
    let low = cut.saturating_sub(ESCAPE_MAX);
    let at = low + hay[low..cut].iter().rposition(|b| *b == 0x1b)?;
    let (len, whole) = escape_at(&hay[at..]);
    (at + len > cut || (!whole && at + len == hay.len())).then_some(at)
}

/// A character that joins a key prefix to the word before it. Keep in step
/// with `grokhub_core::redact`.
fn word_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '-' | '_' | '.')
}

/// The token patterns on one stretch of text. With `glued`, the text shown
/// just before it ended inside a word, so a key prefix at its very start is
/// the middle of a name.
fn redact_words(text: &str, glued: bool) -> String {
    if !glued {
        return redact_secret_words(text);
    }
    // A `.` in front stands for that word. No pattern starts with one, so it
    // comes back first and unchanged.
    let shown = redact_secret_words(&format!(".{text}"));
    shown.strip_prefix('.').unwrap_or(&shown).to_string()
}

/// The terminal's redaction: the project's own values, then the token
/// patterns. A key prefix counts only at the start of a word, so
/// `task-definitions.json` stays readable. Escape sequences (colours) are
/// stepped over: a key right after one is still at the start of a word.
/// `glued` says whether the piece shown before this one ended inside a word,
/// and is set for the next piece.
fn redact_shown(text: &str, held: &[String], glued: &mut bool) -> String {
    let text = redact_held_secrets(text, held);
    let mut out = String::with_capacity(text.len());
    let mut rest = text.as_str();
    while let Some(esc) = rest.find('\x1b') {
        out.push_str(&redact_words(&rest[..esc], *glued));
        let end = esc + escape_at(&rest.as_bytes()[esc..]).0;
        out.push_str(&rest[esc..end]);
        rest = &rest[end..];
        *glued = false;
    }
    let tail = redact_words(rest, *glued);
    if let Some(last) = tail.chars().next_back() {
        *glued = word_char(last);
    }
    out.push_str(&tail);
    out
}

impl Chunker {
    /// A chunker that also hides each of `held` (the project's secret values).
    pub fn new(held: Vec<String>) -> Self {
        let mut held: Vec<String> = held
            .into_iter()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .collect();
        // Longest first, so a value that contains another is hidden whole.
        held.sort_by_key(|v| std::cmp::Reverse(v.len()));
        Self {
            pending: Vec::new(),
            held,
            glued: false,
        }
    }

    fn take(&mut self, len: usize) -> String {
        let head: Vec<u8> = self.pending.drain(..len).collect();
        redact_shown(&String::from_utf8_lossy(&head), &self.held, &mut self.glued)
    }

    /// Move `cut` back until it splits no project secret, no key prefix, no
    /// unfinished key, no `Bearer` from its value and no escape sequence (a
    /// key right after a colour code must still be seen at the start of a word).
    fn safe_cut(&self, mut cut: usize) -> usize {
        loop {
            let before = cut;
            let needles = self
                .held
                .iter()
                .map(String::as_bytes)
                .chain(KEY_PREFIXES)
                .chain(BEARER_WORDS);
            for needle in needles {
                if let Some(at) = straddling(&self.pending, cut, needle) {
                    cut = at;
                }
            }
            if let Some(at) = open_key(&self.pending, cut) {
                cut = at;
            }
            if let Some(at) = open_escape(&self.pending, cut) {
                cut = at;
            }
            if cut == before {
                return cut;
            }
        }
    }

    /// Feed bytes; get back the redacted text that is safe to show now.
    pub fn push(&mut self, bytes: &[u8]) -> String {
        self.pending.extend_from_slice(bytes);
        let space = self
            .pending
            .iter()
            .rposition(|b| *b <= b' ')
            .map_or(0, |i| i + 1);
        let mut cut = self.safe_cut(space);
        if cut == 0 && self.pending.len() > HOLD_MAX {
            cut = self.safe_cut(utf8_safe_len(&self.pending));
        }
        self.take(cut)
    }

    /// Show what is held, except an unfinished UTF-8 character. Called when
    /// the shell has been quiet for a moment.
    pub fn flush(&mut self) -> String {
        let cut = utf8_safe_len(&self.pending);
        self.take(cut)
    }

    /// Show everything, for the end of the stream.
    pub fn finish(&mut self) -> String {
        let all = self.pending.len();
        self.take(all)
    }
}

/// Put a terminal opening or closing in the activity log. Never what was typed
/// or shown.
pub fn log(config_dir: &Path, workspace: &Path, summary: &str, keep: usize) {
    let repo_key = crate::runs::repo_key(workspace, crate::repo::origin_url(workspace).as_deref());
    let _ = crate::activity::append(
        config_dir,
        &crate::activity::Entry {
            ts_ms: grokhub_core::now_ms(),
            kind: crate::activity::Kind::Terminal,
            repo_key,
            host: String::new(),
            branch: String::new(),
            command: String::new(),
            mode: String::new(),
            approved_by: "user".into(),
            exit: None,
            summary: summary.to_string(),
        },
        keep,
    );
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub use desktop::{close, global, open, resize, write};

#[cfg(not(any(target_os = "android", target_os = "ios")))]
mod desktop {
    use super::{Chunk, Chunker, INPUT_CAP};
    use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
    use std::io::{Read, Write};
    use std::path::Path;
    use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
    use std::sync::mpsc::{channel, sync_channel, RecvTimeoutError, SyncSender, TrySendError};
    use std::sync::{Arc, Mutex, MutexGuard};
    use std::time::{Duration, Instant};

    const QUIET: Duration = Duration::from_millis(30);
    /// After the shell ends, its last output is read until it has been quiet
    /// for `LAST_QUIET`, and for `LAST_WORDS` at most.
    const LAST_QUIET: Duration = Duration::from_millis(100);
    const LAST_WORDS: Duration = Duration::from_millis(300);
    /// How many `write` calls may wait for a program that is not reading.
    const INPUT_QUEUE: usize = 64;
    const BUSY: &str = "The terminal is busy. Try again when the program is reading input.";
    const ENDED: &str = "The terminal has ended.";

    type Shell = Arc<Mutex<Box<dyn Child + Send + Sync>>>;

    struct Terminal {
        id: u32,
        master: Box<dyn MasterPty + Send>,
        /// What was typed, on its way to the thread that writes to the shell.
        input: SyncSender<Vec<u8>>,
        shell: Shell,
        /// Set once the end of the shell has been reported.
        ended: Arc<AtomicBool>,
        /// Set once this terminal has been closed and its handles let go.
        closed: Arc<AtomicBool>,
    }

    /// The one open terminal of the window, if any.
    #[derive(Default)]
    pub struct Sessions {
        open: Mutex<Option<Terminal>>,
        /// Held for the whole of an open, so two opens cannot both start a shell.
        opening: Mutex<()>,
        next: AtomicU32,
    }

    fn lock<T: ?Sized>(m: &Mutex<T>) -> MutexGuard<'_, T> {
        m.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// True once the shell itself has ended, whatever it left running.
    fn gone(shell: &Shell) -> bool {
        !matches!(lock(shell).try_wait(), Ok(None))
    }

    fn size(rows: u16, cols: u16) -> PtySize {
        PtySize {
            rows: rows.clamp(2, 500),
            cols: cols.clamp(2, 1000),
            pixel_width: 0,
            pixel_height: 0,
        }
    }

    fn default_shell() -> String {
        #[cfg(windows)]
        {
            std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".into())
        }
        #[cfg(not(windows))]
        {
            match std::env::var("SHELL") {
                Ok(s) if Path::new(&s).is_file() => s,
                _ => "/bin/sh".into(),
            }
        }
    }

    /// What the shell is started with. On macOS an app opened from the Finder
    /// has a bare PATH, so the shell is a login shell and reads the user's
    /// profile, as other terminals there do.
    fn shell_args() -> &'static [&'static str] {
        if cfg!(target_os = "macos") {
            &["-l"]
        } else {
            &[]
        }
    }

    /// The window's terminal, shared by the commands.
    pub fn global() -> &'static Sessions {
        static SESSIONS: std::sync::LazyLock<Sessions> =
            std::sync::LazyLock::new(Sessions::default);
        &SESSIONS
    }

    /// Open the user's shell in `root`. Any earlier terminal is closed first.
    /// `emit` gets the redacted output until the shell ends.
    /// `held` are the project's secret values, hidden in the output.
    pub fn open(
        sessions: &Sessions,
        root: &Path,
        held: Vec<String>,
        rows: u16,
        cols: u16,
        emit: impl Fn(Chunk) + Send + 'static,
    ) -> Result<u32, String> {
        open_with(sessions, root, &default_shell(), held, rows, cols, emit)
    }

    pub(super) fn open_with(
        sessions: &Sessions,
        root: &Path,
        shell: &str,
        held: Vec<String>,
        rows: u16,
        cols: u16,
        emit: impl Fn(Chunk) + Send + 'static,
    ) -> Result<u32, String> {
        if !root.is_dir() {
            return Err("The project folder isn't there.".into());
        }
        let _opening = lock(&sessions.opening);
        close(sessions);
        let pair = native_pty_system()
            .openpty(size(rows, cols))
            .map_err(|_| "A terminal couldn't be opened on this computer.".to_string())?;
        let mut cmd = CommandBuilder::new(shell);
        cmd.args(shell_args());
        cmd.cwd(root);
        cmd.env("TERM", "xterm-256color");
        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|_| "The shell couldn't start.".to_string())?;
        drop(pair.slave);
        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|_| "The terminal couldn't be read.".to_string())?;
        let mut writer = pair
            .master
            .take_writer()
            .map_err(|_| "The terminal couldn't be written to.".to_string())?;
        let shell: Shell = Arc::new(Mutex::new(child));
        let ended = Arc::new(AtomicBool::new(false));
        let id = sessions.next.fetch_add(1, Ordering::SeqCst) + 1;

        let closed = Arc::new(AtomicBool::new(false));
        // How many bytes the reader has handed to the watcher below.
        let read = Arc::new(AtomicU64::new(0));
        let (tx, rx) = channel::<Vec<u8>>();
        let (handed, shut) = (Arc::clone(&read), Arc::clone(&closed));
        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            // Once the end has been reported nothing more goes to the window,
            // but the output is still read, and dropped, until the terminal
            // is closed: a job that outlived the shell is not left blocked in
            // its write, and a pseudoconsole can close while it is drained.
            let mut wanted = true;
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 {
                    break;
                }
                if wanted {
                    handed.fetch_add(n as u64, Ordering::SeqCst);
                    wanted = tx.send(buf[..n].to_vec()).is_ok();
                }
                if !wanted && shut.load(Ordering::SeqCst) {
                    break;
                }
            }
        });

        // Typed input goes to the shell on its own thread: a program that is
        // not reading blocks that thread, never the caller of `write`.
        let (input, typed) = sync_channel::<Vec<u8>>(INPUT_QUEUE);
        let over = Arc::clone(&ended);
        std::thread::spawn(move || {
            for bytes in typed {
                if over.load(Ordering::SeqCst)
                    || writer
                        .write_all(&bytes)
                        .and_then(|()| writer.flush())
                        .is_err()
                {
                    break;
                }
            }
        });

        let (watched, over) = (Arc::clone(&shell), Arc::clone(&ended));
        std::thread::spawn(move || {
            let mut chunker = Chunker::new(held);
            let say = |text: String, exited: bool| {
                if !text.is_empty() || exited {
                    emit(Chunk { id, text, exited });
                }
            };
            // How many of the reader's bytes have been shown.
            let mut shown = 0u64;
            // The end is taken from the shell itself, not from the end of the
            // output: a job the shell left running keeps the output open.
            let mut checked = Instant::now();
            loop {
                match rx.recv_timeout(QUIET) {
                    Ok(bytes) => {
                        shown += bytes.len() as u64;
                        say(chunker.push(&bytes), false);
                    }
                    Err(RecvTimeoutError::Timeout) => say(chunker.flush(), false),
                    Err(RecvTimeoutError::Disconnected) => break,
                }
                if checked.elapsed() >= QUIET {
                    checked = Instant::now();
                    if gone(&watched) {
                        // What the shell printed last. With no job left the
                        // output ends at once; a job that goes on printing is
                        // not waited for.
                        while checked.elapsed() < LAST_WORDS {
                            match rx.recv_timeout(LAST_QUIET) {
                                Ok(bytes) => {
                                    shown += bytes.len() as u64;
                                    say(chunker.push(&bytes), false);
                                }
                                Err(_) => break,
                            }
                        }
                        // The time limit is on waiting for new output. What
                        // has been read by now is shown whole, however long
                        // the window takes over each piece.
                        let due = read.load(Ordering::SeqCst);
                        while shown < due {
                            let Ok(bytes) = rx.recv() else { break };
                            shown += bytes.len() as u64;
                            say(chunker.push(&bytes), false);
                        }
                        break;
                    }
                }
            }
            over.store(true, Ordering::SeqCst);
            say(chunker.finish(), true);
            // `rx` is dropped here: what a job that outlived the shell prints
            // from now on is read by the reader and no longer reaches the
            // window.
        });

        *lock(&sessions.open) = Some(Terminal {
            id,
            master: pair.master,
            input,
            shell,
            ended,
            closed,
        });
        Ok(id)
    }

    /// Send what the user typed to the shell. Never waits for the shell: when
    /// the program in it is not reading, the input is refused.
    pub fn write(sessions: &Sessions, id: u32, data: &str) -> Result<(), String> {
        if data.len() > INPUT_CAP {
            return Err("That is too much to paste into the terminal at once.".into());
        }
        let guard = lock(&sessions.open);
        match guard.as_ref() {
            Some(t) if t.id == id && !t.ended.load(Ordering::SeqCst) => {
                match t.input.try_send(data.as_bytes().to_vec()) {
                    Ok(()) => Ok(()),
                    Err(TrySendError::Full(_)) => Err(BUSY.into()),
                    Err(TrySendError::Disconnected(_)) => Err(ENDED.into()),
                }
            }
            _ => Err(ENDED.into()),
        }
    }

    pub fn resize(sessions: &Sessions, id: u32, rows: u16, cols: u16) -> Result<(), String> {
        let guard = lock(&sessions.open);
        match guard.as_ref() {
            Some(t) if t.id == id => t
                .master
                .resize(size(rows, cols))
                .map_err(|_| "The terminal couldn't be resized.".to_string()),
            _ => Ok(()),
        }
    }

    /// Kill the shell unless it has already ended. A shell that ended is not
    /// signalled again: its process id may belong to something else by now.
    fn stop(shell: &Shell) {
        let mut child = lock(shell);
        if matches!(child.try_wait(), Ok(None)) {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    /// End the open terminal's shell, if there is one. True when one was open.
    pub fn close(sessions: &Sessions) -> bool {
        // Taken out first, so the kill never runs with the sessions lock held.
        let open = lock(&sessions.open).take();
        match open {
            Some(t) => {
                // The shell is ended before `t` goes: dropping the input lets
                // the writer thread finish, and that writes a last newline.
                stop(&t.shell);
                // The reader is told only after the terminal's handles have
                // gone: until then it drains what is still being printed.
                let closed = Arc::clone(&t.closed);
                drop(t);
                closed.store(true, Ordering::SeqCst);
                true
            }
            None => false,
        }
    }

    #[cfg(all(test, unix))]
    mod tests {
        use super::*;
        use std::sync::mpsc;

        fn collect_until(
            rx: &mpsc::Receiver<Chunk>,
            done: impl Fn(&str, bool) -> bool,
        ) -> (String, bool) {
            collect_for(rx, 20, done)
        }

        fn collect_for(
            rx: &mpsc::Receiver<Chunk>,
            secs: u64,
            done: impl Fn(&str, bool) -> bool,
        ) -> (String, bool) {
            let mut all = String::new();
            let mut exited = false;
            let end = std::time::Instant::now() + Duration::from_secs(secs);
            while std::time::Instant::now() < end {
                if let Ok(c) = rx.recv_timeout(Duration::from_millis(200)) {
                    all.push_str(&c.text);
                    exited |= c.exited;
                    if done(&all, exited) {
                        break;
                    }
                }
            }
            (all, exited)
        }

        fn temp(tag: &str) -> std::path::PathBuf {
            let dir =
                std::env::temp_dir().join(format!("gh-code-term-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            dir.canonicalize().unwrap()
        }

        #[test]
        fn shell_starts_in_the_project_folder_and_hides_a_token() {
            let root = temp("run");
            let sessions = Sessions::default();
            let (tx, rx) = mpsc::channel();
            let id = open_with(&sessions, &root, "/bin/sh", Vec::new(), 24, 80, move |c| {
                let _ = tx.send(c);
            })
            .unwrap();
            assert_eq!(id, 1);
            write(&sessions, id, "pwd\n").unwrap();
            let want = root.to_string_lossy().into_owned();
            let (out, _) = collect_until(&rx, |all, _| all.contains(want.as_str()));
            assert!(out.contains(&want), "pwd output missing: {out:?}");

            write(&sessions, id, "echo ghp_abcdefghijklmnop1234 end\n").unwrap();
            let (out, _) = collect_until(&rx, |all, _| all.contains("[redacted] end"));
            assert!(out.contains("[redacted] end"), "no redaction in {out:?}");
            assert!(
                !out.contains("ghp_abcdefghijklmnop1234"),
                "the token reached the window: {out:?}"
            );

            write(&sessions, id, "exit\n").unwrap();
            let (_, exited) = collect_until(&rx, |_, e| e);
            assert!(exited, "shell end was not reported");
            close(&sessions);
        }

        #[test]
        fn closing_ends_the_shell_and_later_writes_fail() {
            let root = temp("close");
            let sessions = Sessions::default();
            let (tx, rx) = mpsc::channel();
            let id = open_with(&sessions, &root, "/bin/sh", Vec::new(), 24, 80, move |c| {
                let _ = tx.send(c);
            })
            .unwrap();
            assert!(close(&sessions));
            assert!(!close(&sessions));
            let (_, exited) = collect_until(&rx, |_, e| e);
            assert!(exited, "close did not end the shell");
            assert_eq!(
                write(&sessions, id, "echo hi\n").unwrap_err(),
                "The terminal has ended."
            );
        }

        #[test]
        fn a_resize_reaches_the_shell_and_a_held_secret_is_hidden() {
            let root = temp("resize");
            let sessions = Sessions::default();
            let (tx, rx) = mpsc::channel();
            let held = vec!["hunter2secretvalue".to_string()];
            let id = open_with(&sessions, &root, "/bin/sh", held, 24, 80, move |c| {
                let _ = tx.send(c);
            })
            .unwrap();
            resize(&sessions, id, 40, 132).unwrap();
            write(&sessions, id, "stty size\n").unwrap();
            let (out, _) = collect_until(&rx, |all, _| all.contains("40 132"));
            assert!(
                out.contains("40 132"),
                "stty size did not see the resize: {out:?}"
            );

            write(&sessions, id, "echo pw=hunter2secretvalue ok\n").unwrap();
            let (out, _) = collect_until(&rx, |all, _| all.contains("pw=[redacted] ok"));
            assert!(out.contains("pw=[redacted] ok"), "no redaction in {out:?}");
            assert!(
                !out.contains("hunter2secretvalue ok"),
                "the secret reached the window: {out:?}"
            );
            // A stale id is ignored, not an error.
            assert_eq!(resize(&sessions, id + 7, 10, 10), Ok(()));
            assert!(close(&sessions));
        }

        #[test]
        fn opening_again_replaces_the_old_terminal() {
            let root = temp("again");
            let sessions = Sessions::default();
            let (tx, rx) = mpsc::channel();
            let first = open_with(&sessions, &root, "/bin/sh", Vec::new(), 24, 80, move |c| {
                let _ = tx.send(c);
            })
            .unwrap();
            let second =
                open_with(&sessions, &root, "/bin/sh", Vec::new(), 24, 80, |_| {}).unwrap();
            assert_eq!((first, second), (1, 2));
            let (_, exited) = collect_until(&rx, |_, e| e);
            assert!(exited, "the first shell was left running");
            assert_eq!(
                write(&sessions, first, "echo hi\n").unwrap_err(),
                "The terminal has ended."
            );
            write(&sessions, second, "true\n").unwrap();
            assert!(close(&sessions));
        }

        #[test]
        fn a_missing_folder_or_a_huge_paste_is_refused() {
            let sessions = Sessions::default();
            let gone = std::env::temp_dir().join("gh-code-term-no-such-dir");
            assert_eq!(
                open_with(&sessions, &gone, "/bin/sh", Vec::new(), 24, 80, |_| {}).unwrap_err(),
                "The project folder isn't there."
            );
            assert_eq!(
                write(&sessions, 1, &"a".repeat(INPUT_CAP + 1)).unwrap_err(),
                "That is too much to paste into the terminal at once."
            );
        }

        #[test]
        fn the_shell_is_a_login_shell_only_on_macos() {
            let want: &[&str] = if cfg!(target_os = "macos") {
                &["-l"]
            } else {
                &[]
            };
            assert_eq!(shell_args(), want);
        }

        #[test]
        fn a_paste_while_the_program_is_not_reading_does_not_block() {
            let root = temp("paste");
            let sessions = Sessions::default();
            let (tx, rx) = mpsc::channel();
            let id = open_with(&sessions, &root, "/bin/sh", Vec::new(), 24, 80, move |c| {
                let _ = tx.send(c);
            })
            .unwrap();
            write(&sessions, id, "sleep 5\n").unwrap();
            std::thread::sleep(Duration::from_millis(300));

            // 1,024 lines of 64 bytes: the most one paste may hold.
            let paste = format!("{}\r", "x".repeat(63)).repeat(1024);
            assert_eq!(paste.len(), INPUT_CAP);
            let began = std::time::Instant::now();
            assert_eq!(write(&sessions, id, &paste), Ok(()));
            // sleep is not reading, so what is typed next piles up and is then refused.
            let refused = (0..200).find_map(|_| write(&sessions, id, "y\r").err());
            assert!(
                began.elapsed() < Duration::from_secs(1),
                "writing waited for the program: {:?}",
                began.elapsed()
            );
            assert_eq!(
                refused.as_deref(),
                Some("The terminal is busy. Try again when the program is reading input.")
            );

            let began = std::time::Instant::now();
            assert!(close(&sessions));
            assert!(
                began.elapsed() < Duration::from_secs(2),
                "closing waited for the stuck paste: {:?}",
                began.elapsed()
            );
            let (_, exited) = collect_for(&rx, 5, |_, e| e);
            assert!(exited, "close did not end the shell");
            assert_eq!(
                write(&sessions, id, "y\r").unwrap_err(),
                "The terminal has ended."
            );
        }

        #[test]
        fn the_shell_ending_is_noticed_while_a_job_still_holds_the_terminal() {
            let root = temp("job");
            let sessions = Sessions::default();
            let (tx, rx) = mpsc::channel();
            let id = open_with(&sessions, &root, "/bin/sh", Vec::new(), 24, 80, move |c| {
                let _ = tx.send(c);
            })
            .unwrap();
            write(&sessions, id, "sleep 8 &\n").unwrap();
            write(&sessions, id, "exit\n").unwrap();
            let (out, exited) = collect_for(&rx, 5, |_, e| e);
            assert!(exited, "shell end was not reported: {out:?}");
            assert_eq!(
                write(&sessions, id, "true\n").unwrap_err(),
                "The terminal has ended."
            );
            assert!(close(&sessions));
        }

        #[test]
        fn closing_stops_the_output_of_a_job_that_outlives_the_shell() {
            let root = temp("loop");
            let sessions = Sessions::default();
            let (tx, rx) = mpsc::channel();
            let id = open_with(&sessions, &root, "/bin/sh", Vec::new(), 24, 80, move |c| {
                let _ = tx.send(c);
            })
            .unwrap();
            // A job in the background that prints for about six seconds.
            let job = "(i=0; while [ $i -lt 30 ]; do echo tick; sleep 0.2; i=$((i+1)); done) &\n";
            write(&sessions, id, job).unwrap();
            let (out, _) = collect_until(&rx, |all, _| all.matches("tick\r\n").count() >= 2);
            assert!(
                out.matches("tick\r\n").count() >= 2,
                "the job did not print: {out:?}"
            );

            assert!(close(&sessions));
            let (_, exited) = collect_for(&rx, 3, |_, e| e);
            assert!(exited, "close did not report the end while the job ran");
            assert_eq!(
                rx.recv_timeout(Duration::from_secs(1)).ok(),
                None,
                "the job's output still reached the window after the end"
            );
        }

        #[test]
        fn output_read_before_the_shell_ended_is_shown_whole_by_a_slow_window() {
            let root = temp("tail");
            let sessions = Sessions::default();
            let (tx, rx) = mpsc::channel();
            let id = open_with(&sessions, &root, "/bin/sh", Vec::new(), 24, 80, move |c| {
                // A window that takes a microsecond over each byte it is given.
                std::thread::sleep(Duration::from_micros(c.text.len() as u64));
                let _ = tx.send(c);
            })
            .unwrap();
            write(&sessions, id, "seq 1 200000; exit\n").unwrap();
            let (out, exited) = collect_for(&rx, 60, |_, e| e);
            assert!(exited, "shell end was not reported");
            // The first number can share its line with the prompt.
            let numbers = out
                .lines()
                .filter(|l| {
                    l.rsplit(' ')
                        .next()
                        .is_some_and(|w| w.parse::<u32>().is_ok())
                })
                .count();
            let tail = &out[out.len().saturating_sub(40)..];
            assert_eq!(numbers, 200_000, "the listing ends early: {tail:?}");
            assert!(
                out.contains("\r\n199999\r\n200000\r\n"),
                "the last lines are missing: {tail:?}"
            );
            close(&sessions);
        }

        #[test]
        fn the_shell_ending_is_reported_while_a_job_prints_without_a_pause() {
            let root = temp("flood");
            let sessions = Sessions::default();
            let (tx, rx) = mpsc::channel();
            let id = open_with(&sessions, &root, "/bin/sh", Vec::new(), 24, 80, move |c| {
                let _ = tx.send(c);
            })
            .unwrap();
            // A job that prints about every 20 ms for some eight seconds.
            // It stops at the first print that fails, so the close ends it.
            let job =
                "(i=0; while [ $i -lt 400 ] && echo tick; do sleep 0.02; i=$((i+1)); done) &\n";
            write(&sessions, id, job).unwrap();
            let (out, _) = collect_until(&rx, |all, _| all.matches("tick\r\n").count() >= 2);
            assert!(
                out.matches("tick\r\n").count() >= 2,
                "the job did not print: {out:?}"
            );
            write(&sessions, id, "exit\n").unwrap();
            let (_, exited) = collect_for(&rx, 3, |_, e| e);
            assert!(exited, "the printing job held back the news of the end");
            assert!(close(&sessions));
        }

        #[test]
        fn a_job_that_outlives_the_shell_is_not_left_blocked_in_its_output() {
            let root = temp("drain");
            let sessions = Sessions::default();
            let (tx, rx) = mpsc::channel();
            let id = open_with(&sessions, &root, "/bin/sh", Vec::new(), 24, 80, move |c| {
                let _ = tx.send(c);
            })
            .unwrap();
            // A second after the shell has gone the job prints far more than
            // a terminal buffers, then leaves a file to say it got through.
            let job = "(sleep 1; yes | head -c 300000; : > printed) &\n";
            write(&sessions, id, job).unwrap();
            write(&sessions, id, "exit\n").unwrap();
            let (_, exited) = collect_for(&rx, 5, |_, e| e);
            assert!(exited, "shell end was not reported");

            let printed = root.join("printed");
            let end = std::time::Instant::now() + Duration::from_secs(8);
            while !printed.exists() && std::time::Instant::now() < end {
                std::thread::sleep(Duration::from_millis(50));
            }
            let got_through = printed.exists();
            assert_eq!(
                rx.try_recv().ok(),
                None,
                "the job's output reached the window after the end"
            );
            assert!(close(&sessions));
            assert!(got_through, "the job was left blocked in its write");
        }

        #[test]
        fn closing_cuts_off_a_job_that_goes_on_printing() {
            let root = temp("cut");
            let sessions = Sessions::default();
            let (tx, rx) = mpsc::channel();
            let id = open_with(&sessions, &root, "/bin/sh", Vec::new(), 24, 80, move |c| {
                let _ = tx.send(c);
            })
            .unwrap();
            // Prints for some twenty seconds unless a print fails; then it
            // leaves a file with how far it got.
            let job = "(trap '' HUP; i=0; while [ $i -lt 100 ] && echo tick; do sleep 0.2; \
                       i=$((i+1)); done; echo $i > stopped) &\n";
            write(&sessions, id, job).unwrap();
            let (out, _) = collect_until(&rx, |all, _| all.matches("tick\r\n").count() >= 2);
            assert!(
                out.matches("tick\r\n").count() >= 2,
                "the job did not print: {out:?}"
            );

            assert!(close(&sessions));
            let stopped = root.join("stopped");
            let end = std::time::Instant::now() + Duration::from_secs(5);
            while !stopped.exists() && std::time::Instant::now() < end {
                std::thread::sleep(Duration::from_millis(50));
            }
            assert!(
                stopped.exists(),
                "the job went on printing into a closed terminal"
            );
        }

        #[test]
        fn two_opens_at_the_same_moment_leave_one_shell() {
            use std::os::unix::fs::PermissionsExt;
            use std::sync::{Arc, Barrier};

            let root = temp("race");
            // A stand-in shell that ignores its input, so only a kill ends it.
            let shell = root.join("quiet-shell");
            std::fs::write(&shell, "#!/bin/sh\nexec sleep 20\n").unwrap();
            std::fs::set_permissions(&shell, std::fs::Permissions::from_mode(0o755)).unwrap();
            let shell = shell.to_string_lossy().into_owned();

            let sessions = Arc::new(Sessions::default());
            let (tx, rx) = mpsc::channel::<u32>();
            for _ in 0..10 {
                let gate = Arc::new(Barrier::new(2));
                let pair: Vec<_> = (0..2)
                    .map(|_| {
                        let (sessions, gate) = (Arc::clone(&sessions), Arc::clone(&gate));
                        let (root, shell, tx) = (root.clone(), shell.clone(), tx.clone());
                        std::thread::spawn(move || {
                            gate.wait();
                            open_with(&sessions, &root, &shell, Vec::new(), 24, 80, move |c| {
                                if c.exited {
                                    let _ = tx.send(c.id);
                                }
                            })
                            .unwrap()
                        })
                    })
                    .collect();
                for t in pair {
                    t.join().unwrap();
                }
            }
            assert!(close(&sessions));

            let mut ended = Vec::new();
            let end = std::time::Instant::now() + Duration::from_secs(5);
            while ended.len() < 20 && std::time::Instant::now() < end {
                if let Ok(id) = rx.recv_timeout(Duration::from_millis(100)) {
                    ended.push(id);
                }
            }
            ended.sort_unstable();
            assert_eq!(
                ended,
                (1..=20).collect::<Vec<u32>>(),
                "a shell was left running"
            );
        }
    }
}

#[cfg(any(target_os = "android", target_os = "ios"))]
mod mobile {
    /// Phones have no terminal.
    pub const NONE: &str = "The terminal is only on the desktop app.";
}
#[cfg(any(target_os = "android", target_os = "ios"))]
pub use mobile::NONE;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_token_split_across_reads_is_redacted_whole() {
        let mut c = Chunker::default();
        assert_eq!(c.push(b"token ghp_abcdef"), "token ");
        assert_eq!(c.push(b"ghijklmnop12 done\n"), "[redacted] done\n");
        assert_eq!(c.flush(), "");
    }

    #[test]
    fn quiet_time_shows_the_held_word_and_the_end_shows_the_rest() {
        let mut c = Chunker::default();
        assert_eq!(c.push(b"$"), "");
        assert_eq!(c.flush(), "$");
        assert_eq!(c.push(b"ab"), "");
        assert_eq!(c.finish(), "ab");
    }

    #[test]
    fn a_split_utf8_character_is_not_broken() {
        let mut c = Chunker::default();
        assert_eq!(c.push(&[0xe2, 0x82]), "");
        assert_eq!(c.flush(), "");
        assert_eq!(c.push(&[0xac, b' ']), "\u{20ac} ");
    }

    #[test]
    fn a_long_unbroken_run_is_not_held_forever() {
        let mut c = Chunker::default();
        let out = c.push("a".repeat(HOLD_MAX + 1).as_bytes());
        assert_eq!(out.len(), HOLD_MAX + 1);
        assert_eq!(c.flush(), "");
    }

    #[test]
    fn a_held_project_secret_is_hidden_too() {
        let mut c = Chunker::new(vec!["abcd1234XYZ".to_string()]);
        assert_eq!(c.push(b"token=abcd1234XYZ ok\n"), "token=[redacted] ok\n");
    }

    #[test]
    fn opening_and_closing_is_logged_without_any_text() {
        let config = std::env::temp_dir().join(format!("gh-code-term-log-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&config);
        std::fs::create_dir_all(&config).unwrap();
        log(
            &config,
            &config,
            "Opened a terminal in the project folder.",
            100,
        );
        let rows = crate::activity::list(&config, 10);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].kind, crate::activity::Kind::Terminal);
        assert_eq!(rows[0].summary, "Opened a terminal in the project folder.");
    }

    #[test]
    fn ordinary_names_with_a_key_prefix_stay_readable() {
        let mut c = Chunker::default();
        assert_eq!(
            c.push(b"task-runner.rs  sk-short\r\n"),
            "task-runner.rs  sk-short\r\n"
        );
        assert_eq!(
            c.push(b"task-definitions.json  fix/task-list-rendering  flask-debugtoolbar\r\n"),
            "task-definitions.json  fix/task-list-rendering  flask-debugtoolbar\r\n"
        );
        assert_eq!(
            c.push(b"src/task-queue/worker.rs  risk-assessment-2024.md  mask-utils.test.ts\r\n"),
            "src/task-queue/worker.rs  risk-assessment-2024.md  mask-utils.test.ts\r\n"
        );
        assert_eq!(
            c.push(b"KEY=sk-abcdefghijklmnopqrstuv\n"),
            "KEY=[redacted]\n"
        );
    }

    #[test]
    fn a_name_with_a_key_prefix_stays_readable_when_an_escape_code_follows_it() {
        // A shell that redraws its line prints the name, then "erase to the
        // end of the line".
        let mut c = Chunker::default();
        let mut out = c.push(b"cat task-definitions.json\x1b[K");
        out.push_str(&c.flush());
        assert_eq!(out, "cat task-definitions.json\x1b[K");

        // Coloured output whose newline comes with the next read.
        let mut c = Chunker::default();
        let mut out = c.push(b"\x1b[01;32mtask-definitions.json\x1b[0m");
        out.push_str(&c.push(b"\r\n"));
        assert_eq!(out, "\x1b[01;32mtask-definitions.json\x1b[0m\r\n");

        let mut c = Chunker::default();
        let mut out = c.push(b"* \x1b[32mfix/disk-partition-table\x1b[m");
        out.push_str(&c.flush());
        assert_eq!(out, "* \x1b[32mfix/disk-partition-table\x1b[m");

        // A key in the same place is still hidden.
        let mut c = Chunker::default();
        let mut out = c.push(b"KEY=sk-abcdefghijklmnopqrstuv\x1b[K");
        out.push_str(&c.flush());
        assert_eq!(out, "KEY=[redacted]\x1b[K");
    }

    #[test]
    fn a_name_that_arrives_in_two_pieces_stays_readable() {
        // Tab completion: `ta`, then the rest of the name.
        let mut c = Chunker::default();
        assert_eq!(c.push(b"cat ta"), "cat ");
        assert_eq!(c.flush(), "ta");
        assert_eq!(c.push(b"sk-definitions.json "), "sk-definitions.json ");

        // A key that starts a piece is still hidden: after a space, after a
        // colour code, and after a project secret.
        let mut c = Chunker::new(vec!["hunter2secretvalue".to_string()]);
        assert_eq!(c.push(b"echo "), "echo ");
        assert_eq!(c.push(b"sk-abcdefghijklmnopqrstuv "), "[redacted] ");
        assert_eq!(c.push(b"ta\x1b[0m"), "ta");
        assert_eq!(c.flush(), "\x1b[0m");
        assert_eq!(c.push(b"sk-abcdefghijklmnopqrstuv\n"), "[redacted]\n");
        assert_eq!(c.push(b"hunter2secretvalue"), "");
        assert_eq!(c.flush(), "[redacted]");
        assert_eq!(c.push(b"sk-abcdefghijklmnopqrstuv\n"), "[redacted]\n");
        assert_eq!(c.flush(), "");
    }

    #[test]
    fn a_key_right_after_a_colour_code_is_still_hidden() {
        let mut c = Chunker::default();
        assert_eq!(
            c.push(b"\x1b[32msk-abcdefghijklmnopqrstuv\x1b[0m\n"),
            "\x1b[32m[redacted]\x1b[0m\n"
        );
        assert_eq!(
            c.push(b"KEY=\x1b[38;5;186msk-abcdefghijklmnopqrstuv\x1b(B\x1b[m\n"),
            "KEY=\x1b[38;5;186m[redacted]\x1b(B\x1b[m\n"
        );
        assert_eq!(
            c.push(b"\x1b(Bsk-abcdefghijklmnopqrstuv\n"),
            "\x1b(B[redacted]\n"
        );
        // A colour code does not make a glued name a key.
        assert_eq!(
            c.push(b"\x1b[1mtask-definitions.json\x1b[0m\n"),
            "\x1b[1mtask-definitions.json\x1b[0m\n"
        );
    }

    #[test]
    fn a_key_split_after_a_colour_code_is_still_hidden() {
        let mut c = Chunker::default();
        assert_eq!(c.push(b"KEY=\x1b[32msk-abcdefgh"), "KEY=");
        assert_eq!(
            c.push(b"ijklmnopqrstuv\x1b[0m\n"),
            "\x1b[32m[redacted]\x1b[0m\n"
        );
        // The read can end inside the colour code too.
        assert_eq!(c.push(b"KEY=\x1b[3"), "KEY=");
        assert_eq!(
            c.push(b"2msk-abcdefghijklmnopqrstuv\x1b[0m\n"),
            "\x1b[32m[redacted]\x1b[0m\n"
        );
        assert_eq!(c.flush(), "");
    }

    #[test]
    fn a_held_value_split_inside_a_long_unbroken_run_stays_hidden() {
        let mut c = Chunker::new(vec!["hunter2secretvalue".to_string()]);
        let run = "x".repeat(600);
        assert_eq!(c.push(format!("{run}hunter2se").as_bytes()), run);
        assert_eq!(c.push(b"cretvalue\n"), "[redacted]\n");
        assert_eq!(c.flush(), "");
    }

    #[test]
    fn a_key_split_inside_a_long_unbroken_run_stays_hidden() {
        let mut c = Chunker::default();
        let run = format!("{{\"a\":\"{}\",\"k\":\"", "x".repeat(600));
        assert_eq!(c.push(format!("{run}ghp_abcdef").as_bytes()), run);
        assert_eq!(c.push(b"ghijklmnop12\"}\n"), "[redacted]\"}\n");

        // The cut can fall inside the prefix itself.
        assert_eq!(c.push(format!("{run}gh").as_bytes()), run);
        assert_eq!(c.push(b"p_abcdefghijklmnop12\"}\n"), "[redacted]\"}\n");
        assert_eq!(c.flush(), "");
    }

    #[test]
    fn a_long_key_is_held_whole_but_an_endless_run_is_let_out() {
        let mut c = Chunker::default();
        assert_eq!(c.push(format!("sk-{}", "a".repeat(600)).as_bytes()), "");
        assert_eq!(c.push(b"bcd end\n"), "[redacted] end\n");

        // Far longer than any key: shown as hidden, not kept in memory.
        assert_eq!(
            c.push(format!("sk-{}", "a".repeat(TOKEN_MAX + 1)).as_bytes()),
            "[redacted]"
        );
        assert_eq!(c.flush(), "");
    }

    #[test]
    fn a_bearer_value_split_from_its_word_is_redacted_whole() {
        let mut c = Chunker::default();
        assert_eq!(c.push(b"Authorization: Bearer abcdefgh"), "Authorization: ");
        assert_eq!(c.push(b"ijklmnop1234\n"), "[redacted]\n");

        assert_eq!(c.push(b"authorization: bearer "), "authorization: ");
        assert_eq!(c.push(b"abcdefghijklmnop1234\r\n"), "[redacted]\r\n");

        // An ordinary sentence is only held until the next word.
        assert_eq!(c.push(b"the bearer "), "the ");
        assert_eq!(c.push(b"of news\n"), "bearer of news\n");
        assert_eq!(c.flush(), "");
    }

    #[test]
    fn a_held_value_with_spaces_split_across_reads_stays_hidden() {
        let mut c = Chunker::new(vec!["correct horse battery".to_string()]);
        assert_eq!(c.push(b"pw: correct horse"), "pw: ");
        assert_eq!(c.push(b" battery ok\n"), "[redacted] ok\n");

        // A look-alike start is shown once the next word proves it is not the value.
        assert_eq!(c.push(b"a correct horse"), "a ");
        assert_eq!(c.push(b" race\n"), "correct horse race\n");
        assert_eq!(c.flush(), "");
    }
}
