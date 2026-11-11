//! The Code tab terminal: the user's own shell on a pseudo-terminal, started in
//! the project folder. Output passes through the secret redactor (the
//! project's own secret values, then the usual token patterns) before it
//! reaches the window. Project secrets are never put in this shell's
//! environment, and what is typed or shown is never logged. Mobile builds have
//! no terminal.
//!
//! This is a normal shell: a `git push` typed here is not checked by the Code
//! tab's push guard (`code_ship.rs`), which covers the tab's own buttons.

use crate::secrets as code_secrets;
use serde::Serialize;
use std::path::Path;

/// The Tauri event the terminal's output travels on.
pub const EVENT: &str = "code-term";
/// The most bytes one `write` call sends to the shell.
pub const INPUT_CAP: usize = 65_536;
/// Output with no space or newline in it is held at most this long.
const HOLD_MAX: usize = 512;

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
/// split across two reads is still seen whole.
#[derive(Debug, Default)]
pub struct Chunker {
    pending: Vec<u8>,
    held: Vec<String>,
}

fn utf8_safe_len(bytes: &[u8]) -> usize {
    match std::str::from_utf8(bytes) {
        Ok(_) => bytes.len(),
        Err(e) if e.error_len().is_none() => e.valid_up_to(),
        Err(_) => bytes.len(),
    }
}

impl Chunker {
    /// A chunker that also hides each of `held` (the project's secret values).
    pub fn new(held: Vec<String>) -> Self {
        Self {
            pending: Vec::new(),
            held,
        }
    }

    fn take(&mut self, len: usize) -> String {
        let head: Vec<u8> = self.pending.drain(..len).collect();
        code_secrets::redact(&String::from_utf8_lossy(&head), &self.held)
    }

    /// Feed bytes; get back the redacted text that is safe to show now.
    pub fn push(&mut self, bytes: &[u8]) -> String {
        self.pending.extend_from_slice(bytes);
        let mut cut = self
            .pending
            .iter()
            .rposition(|b| *b <= b' ')
            .map_or(0, |i| i + 1);
        if cut == 0 && self.pending.len() > HOLD_MAX {
            cut = utf8_safe_len(&self.pending);
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
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::mpsc::{channel, RecvTimeoutError};
    use std::sync::{Mutex, MutexGuard};
    use std::time::Duration;

    const QUIET: Duration = Duration::from_millis(30);

    struct Terminal {
        id: u32,
        master: Box<dyn MasterPty + Send>,
        writer: Box<dyn Write + Send>,
        child: Box<dyn Child + Send + Sync>,
    }

    /// The one open terminal of the window, if any.
    #[derive(Default)]
    pub struct Sessions {
        open: Mutex<Option<Terminal>>,
        next: AtomicU32,
    }

    fn lock(m: &Mutex<Option<Terminal>>) -> MutexGuard<'_, Option<Terminal>> {
        m.lock().unwrap_or_else(|e| e.into_inner())
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
        close(sessions);
        let pair = native_pty_system()
            .openpty(size(rows, cols))
            .map_err(|_| "A terminal couldn't be opened on this computer.".to_string())?;
        let mut cmd = CommandBuilder::new(shell);
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
        let writer = pair
            .master
            .take_writer()
            .map_err(|_| "The terminal couldn't be written to.".to_string())?;
        let id = sessions.next.fetch_add(1, Ordering::SeqCst) + 1;

        let (tx, rx) = channel::<Vec<u8>>();
        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 || tx.send(buf[..n].to_vec()).is_err() {
                    break;
                }
            }
        });
        std::thread::spawn(move || {
            let mut chunker = Chunker::new(held);
            let say = |text: String, exited: bool| {
                if !text.is_empty() || exited {
                    emit(Chunk { id, text, exited });
                }
            };
            loop {
                match rx.recv_timeout(QUIET) {
                    Ok(bytes) => say(chunker.push(&bytes), false),
                    Err(RecvTimeoutError::Timeout) => say(chunker.flush(), false),
                    Err(RecvTimeoutError::Disconnected) => {
                        say(chunker.finish(), true);
                        break;
                    }
                }
            }
        });

        *lock(&sessions.open) = Some(Terminal {
            id,
            master: pair.master,
            writer,
            child,
        });
        Ok(id)
    }

    /// Send what the user typed to the shell.
    pub fn write(sessions: &Sessions, id: u32, data: &str) -> Result<(), String> {
        if data.len() > INPUT_CAP {
            return Err("That is too much to paste into the terminal at once.".into());
        }
        let mut guard = lock(&sessions.open);
        match guard.as_mut() {
            Some(t) if t.id == id => t
                .writer
                .write_all(data.as_bytes())
                .and_then(|_| t.writer.flush())
                .map_err(|_| "The terminal has ended.".to_string()),
            _ => Err("The terminal has ended.".into()),
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

    /// End the open terminal's shell, if there is one. True when one was open.
    pub fn close(sessions: &Sessions) -> bool {
        match lock(&sessions.open).take() {
            Some(mut t) => {
                let _ = t.child.kill();
                let _ = t.child.wait();
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
            let mut all = String::new();
            let mut exited = false;
            let end = std::time::Instant::now() + Duration::from_secs(20);
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
    }
}
