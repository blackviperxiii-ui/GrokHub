//! A chat turn on the user's own Claude plan: each turn starts the official
//! `claude` CLI in the chat's folder, sends the message on stdin, and shows
//! its stream as the chat's events. The app never sees a Claude credential;
//! Claude Code signs itself in. See `docs/design/claude-plan/README.md`.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use grokhub_core::wire::AcpEvent;
use serde_json::{json, Value};

use super::stream::{Outcome, Stream};
use crate::client::CancelToken;
use crate::events::Engine;

/// The tools a chat on the plan may use until live approvals land: reading
/// only. Anything else Claude asks for is denied and shows as a failed row.
pub const READ_ONLY_TOOLS: [&str; 3] = ["Read", "Grep", "Glob"];

/// How much of Claude Code's stderr is kept for an error line.
const STDERR_CAP: usize = 4_096;
/// How often a running turn looks at Stop.
const STOP_POLL: Duration = Duration::from_millis(100);

pub struct ClaudePlanEngine {
    cli: PathBuf,
    workspace: PathBuf,
    allowed: Vec<String>,
    /// This chat's Claude Code session: named on the first turn, resumed after.
    session: String,
    /// A turn has started the session, so the next one resumes it.
    started: bool,
    cancel: CancelToken,
}

impl ClaudePlanEngine {
    /// `cli` comes from `claude_plan::find`; `cancel` is the chat's Stop.
    pub fn new(cli: PathBuf, workspace: PathBuf, cancel: CancelToken) -> Self {
        Self {
            cli,
            workspace,
            allowed: READ_ONLY_TOOLS.iter().map(|t| t.to_string()).collect(),
            session: new_session_id(),
            started: false,
            cancel,
        }
    }

    pub fn set_workspace(&mut self, workspace: PathBuf) {
        self.workspace = workspace;
    }

    /// The command line for one turn. Permission prompts go nowhere, so a
    /// tool outside `allowed` is denied rather than waiting on a question
    /// nobody sees; the permission mode is named so a `bypassPermissions`
    /// default in the user's own Claude Code settings doesn't apply here.
    fn args(&self) -> Vec<String> {
        let mut args: Vec<String> = [
            "-p",
            "--input-format",
            "stream-json",
            "--output-format",
            "stream-json",
            "--verbose",
            "--include-partial-messages",
            "--permission-mode",
            "default",
            "--permission-prompts",
            "none",
        ]
        .iter()
        .map(|a| a.to_string())
        .collect();
        args.push(
            if self.started {
                "--resume"
            } else {
                "--session-id"
            }
            .into(),
        );
        args.push(self.session.clone());
        // Last: the flag takes every value after it.
        args.push("--allowedTools".into());
        args.extend(self.allowed.iter().cloned());
        args
    }

    fn run(&mut self, text: &str, image: Option<&str>, emit: &mut dyn FnMut(AcpEvent)) {
        let mut cmd = Command::new(&self.cli);
        cmd.args(self.args())
            .current_dir(&self.workspace)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        grokhub_core::proc_util::hide_windows_console(&mut cmd);
        let mut child = match cmd.spawn() {
            Ok(child) => child,
            Err(err) => return fail(emit, format!("Couldn't start Claude Code: {err}")),
        };
        let (Some(mut stdin), Some(stdout), Some(stderr)) =
            (child.stdin.take(), child.stdout.take(), child.stderr.take())
        else {
            let _ = child.kill();
            let _ = child.wait();
            return fail(emit, "Couldn't read Claude Code's output.".into());
        };
        // A write that fails means Claude Code already quit; its output says why.
        let _ = writeln!(stdin, "{}", prompt_line(text, image));
        drop(stdin);

        let child = Arc::new(Mutex::new(child));
        let finished = Arc::new(AtomicBool::new(false));
        let stopped = Arc::new(AtomicBool::new(false));
        let watcher = {
            let (child, finished, stopped) = (child.clone(), finished.clone(), stopped.clone());
            let cancel = self.cancel.clone();
            thread::spawn(move || {
                while !finished.load(Ordering::SeqCst) {
                    if cancel.is_cancelled() {
                        stopped.store(true, Ordering::SeqCst);
                        if let Ok(mut child) = child.lock() {
                            interrupt(&mut child);
                        }
                        return;
                    }
                    thread::sleep(STOP_POLL);
                }
            })
        };
        let errors = thread::spawn(move || {
            let mut kept = Vec::new();
            let _ = stderr.take(STDERR_CAP as u64).read_to_end(&mut kept);
            String::from_utf8_lossy(&kept).into_owned()
        });

        let mut stream = Stream::new();
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            for event in stream.line(&line) {
                emit(event);
            }
        }
        finished.store(true, Ordering::SeqCst);
        let _ = watcher.join();
        if let Ok(mut child) = child.lock() {
            let _ = child.wait();
        }
        let errors = errors.join().unwrap_or_default();

        if stream.session_id() == Some(self.session.as_str()) {
            self.started = true;
        }
        let ended = *stream.outcome() == Outcome::Done;
        if stopped.load(Ordering::SeqCst) {
            if !ended {
                emit(AcpEvent::Done {
                    stop_reason: "cancelled".into(),
                });
            }
            return;
        }
        let no_result = *stream.outcome() == Outcome::Running;
        if let Err(why) = stream.finish() {
            let why = match last_line(&errors) {
                Some(said) if no_result => format!("{why} It said: {said}"),
                _ => why,
            };
            fail(emit, why);
        }
    }
}

impl Engine for ClaudePlanEngine {
    fn prompt(
        &mut self,
        text: &str,
        image: Option<&str>,
        emit: &mut dyn FnMut(AcpEvent),
    ) -> Result<(), String> {
        self.cancel.reset();
        self.run(text, image, emit);
        Ok(())
    }

    fn cancel(&self) {
        self.cancel.cancel();
    }
}

/// Ends the turn with `why`, the way native turns end on an error.
fn fail(emit: &mut dyn FnMut(AcpEvent), why: String) {
    emit(AcpEvent::Err(why));
    emit(AcpEvent::Done {
        stop_reason: "error".into(),
    });
}

/// Stop: SIGINT, so Claude Code ends the turn and records it. Windows has no
/// SIGINT for a console-less child, so the process is ended and that turn's
/// result is lost.
fn interrupt(child: &mut Child) {
    #[cfg(unix)]
    if let Ok(pid) = libc::pid_t::try_from(child.id()) {
        // SAFETY: kill(2) on our own child's pid, which we haven't waited on.
        unsafe {
            libc::kill(pid, libc::SIGINT);
        }
    }
    #[cfg(not(unix))]
    let _ = child.kill();
}

/// The one stdin line for `--input-format stream-json`. A picture comes as a
/// `data:` URL; anything else isn't one Claude can take, so it's left out.
pub fn prompt_line(text: &str, image: Option<&str>) -> String {
    let mut content = Vec::new();
    if let Some((media_type, data)) = image.and_then(data_url) {
        content.push(json!({
            "type": "image",
            "source": {"type": "base64", "media_type": media_type, "data": data},
        }));
    }
    content.push(json!({"type": "text", "text": text}));
    let line: Value = json!({
        "type": "user",
        "message": {"role": "user", "content": content},
    });
    line.to_string()
}

fn data_url(url: &str) -> Option<(&str, &str)> {
    let (head, data) = url.strip_prefix("data:")?.split_once(',')?;
    let media_type = head.strip_suffix(";base64")?;
    (media_type.starts_with("image/") && !data.is_empty()).then_some((media_type, data))
}

fn last_line(text: &str) -> Option<String> {
    let line = text.lines().rev().map(str::trim).find(|l| !l.is_empty())?;
    Some(line.chars().take(300).collect())
}

/// A random version-4 UUID, the form `--session-id` takes.
fn new_session_id() -> String {
    use ring::rand::SecureRandom;
    let mut b = [0u8; 16];
    if ring::rand::SystemRandom::new().fill(&mut b).is_err() {
        // No system randomness: the clock still keeps chats apart.
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default();
        b = nanos.to_le_bytes();
    }
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let hex: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    /// The session flag and its value in a turn's arguments.
    fn session_flag(args: &[String]) -> (&str, &str) {
        let at = args
            .iter()
            .position(|a| a == "--resume" || a == "--session-id")
            .unwrap();
        (args[at].as_str(), args[at + 1].as_str())
    }

    fn parsed(line: &str) -> Value {
        serde_json::from_str(line).unwrap()
    }

    fn engine(cli: &Path, workspace: &Path) -> ClaudePlanEngine {
        ClaudePlanEngine::new(
            cli.to_path_buf(),
            workspace.to_path_buf(),
            CancelToken::new(),
        )
    }

    #[test]
    fn a_turn_reads_only_and_never_waits_on_a_prompt() {
        let mut e = engine(Path::new("claude"), Path::new("."));
        e.session = "3f1c2a4e-8b7d-4c6e-9a1b-2d3e4f5a6b7c".into();
        let joined = e.args().join(" ");
        assert_eq!(
            joined,
            "-p --input-format stream-json --output-format stream-json --verbose \
             --include-partial-messages --permission-mode default --permission-prompts none \
             --session-id 3f1c2a4e-8b7d-4c6e-9a1b-2d3e4f5a6b7c --allowedTools Read Grep Glob"
        );
        e.started = true;
        assert_eq!(
            session_flag(&e.args()),
            ("--resume", "3f1c2a4e-8b7d-4c6e-9a1b-2d3e4f5a6b7c")
        );
    }

    #[test]
    fn each_chat_gets_its_own_v4_session_id() {
        let (a, b) = (new_session_id(), new_session_id());
        assert_ne!(a, b);
        let parts: Vec<usize> = a.split('-').map(str::len).collect();
        assert_eq!(parts, [8, 4, 4, 4, 12]);
        assert_eq!(&a[14..15], "4");
        assert!(matches!(&a[19..20], "8" | "9" | "a" | "b"), "{a}");
    }

    #[test]
    fn the_message_goes_as_one_stream_json_line_with_its_picture() {
        assert_eq!(
            parsed(&prompt_line("hi \"there\"", None)),
            json!({"type": "user", "message": {"role": "user", "content": [
                {"type": "text", "text": "hi \"there\""},
            ]}})
        );
        assert_eq!(
            parsed(&prompt_line(
                "what is this",
                Some("data:image/png;base64,iVBORw0K")
            )),
            json!({"type": "user", "message": {"role": "user", "content": [
                {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "iVBORw0K"}},
                {"type": "text", "text": "what is this"},
            ]}})
        );
        // Not a picture Claude can take: the text still goes.
        assert_eq!(
            parsed(&prompt_line("x", Some("/home/user/a.png"))),
            json!({"type": "user", "message": {"role": "user", "content": [
                {"type": "text", "text": "x"},
            ]}})
        );
        assert!(!prompt_line("x", None).contains('\n'));
        assert_eq!(data_url("data:text/plain;base64,aGk="), None);
    }

    #[test]
    fn keeps_the_last_thing_claude_code_said_on_stderr() {
        assert_eq!(
            last_line("warming up\nInvalid API key\n\n"),
            Some("Invalid API key".into())
        );
        assert_eq!(last_line("  \n"), None);
    }

    #[cfg(unix)]
    mod with_a_fake_cli {
        use super::*;

        fn scratch(tag: &str) -> PathBuf {
            let dir =
                std::env::temp_dir().join(format!("gh-claude-engine-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            dir
        }

        /// A `claude` that saves its arguments and stdin, then runs `body`.
        fn fake_cli(dir: &Path, body: &str) -> PathBuf {
            use std::os::unix::fs::PermissionsExt;
            let cli = dir.join("claude");
            let script = format!(
                "#!/bin/sh\necho \"$@\" > \"{d}/args\"\npwd > \"{d}/cwd\"\ncat > \"{d}/stdin\"\n{body}\n",
                d = dir.display()
            );
            std::fs::write(&cli, script).unwrap();
            std::fs::set_permissions(&cli, std::fs::Permissions::from_mode(0o755)).unwrap();
            cli
        }

        fn lines_of(lines: &[&str]) -> String {
            lines
                .iter()
                .map(|l| format!("cat <<'EOF'\n{l}\nEOF\n"))
                .collect()
        }

        fn run(e: &mut ClaudePlanEngine, text: &str) -> Vec<AcpEvent> {
            let mut seen = Vec::new();
            e.prompt(text, None, &mut |ev| seen.push(ev)).unwrap();
            seen
        }

        fn init(id: &str) -> String {
            format!(r#"{{"type":"system","subtype":"init","session_id":"{id}"}}"#)
        }

        const SID: &str = "3f1c2a4e-8b7d-4c6e-9a1b-2d3e4f5a6b7c";
        const OK: &str = r#"{"type":"result","subtype":"success","is_error":false,"usage":{"input_tokens":3,"output_tokens":2}}"#;

        #[test]
        fn runs_a_turn_in_the_chat_folder_and_resumes_the_next() {
            let dir = scratch("turn");
            let work = dir.join("work");
            std::fs::create_dir_all(&work).unwrap();
            let delta = r#"{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"pong"}}}"#;
            let cli = fake_cli(&dir, &lines_of(&[&init(SID), delta, OK]));
            let mut e = engine(&cli, &work);
            e.session = SID.into();

            let seen = run(&mut e, "ping");
            assert_eq!(
                seen.iter()
                    .filter(|ev| !matches!(ev, AcpEvent::Usage(_)))
                    .cloned()
                    .collect::<Vec<_>>(),
                vec![
                    AcpEvent::Ready {
                        session_id: SID.into()
                    },
                    AcpEvent::Text("pong".into()),
                    AcpEvent::Done {
                        stop_reason: "end_turn".into()
                    },
                ]
            );
            let read = |f: &str| std::fs::read_to_string(dir.join(f)).unwrap();
            assert_eq!(
                read("cwd").trim(),
                work.canonicalize().unwrap().to_str().unwrap()
            );
            assert_eq!(
                parsed(read("stdin").strip_suffix('\n').unwrap()),
                parsed(&prompt_line("ping", None))
            );
            assert!(
                read("args").contains(&format!("--session-id {SID} ")),
                "{}",
                read("args")
            );

            run(&mut e, "again");
            assert!(
                read("args").contains(&format!("--resume {SID} ")),
                "{}",
                read("args")
            );
        }

        #[test]
        fn a_turn_that_never_started_the_session_starts_it_again() {
            let dir = scratch("nostart");
            let cli = fake_cli(&dir, "echo 'Not logged in' >&2\nexit 1");
            let mut e = engine(&cli, &dir);
            e.session = SID.into();
            let seen = run(&mut e, "hi");
            assert_eq!(
                seen,
                vec![
                    AcpEvent::Err(
                        "Claude Code stopped before it finished the turn. It said: Not logged in"
                            .into()
                    ),
                    AcpEvent::Done {
                        stop_reason: "error".into()
                    },
                ]
            );
            run(&mut e, "hi");
            let args = std::fs::read_to_string(dir.join("args")).unwrap();
            assert!(args.contains(&format!("--session-id {SID} ")), "{args}");
        }

        #[test]
        fn a_missing_cli_ends_the_turn_with_a_plain_error() {
            let dir = scratch("missing");
            let mut e = engine(&dir.join("claude"), &dir);
            let seen = run(&mut e, "hi");
            assert_eq!(seen.len(), 2);
            assert!(
                matches!(&seen[0], AcpEvent::Err(m) if m.starts_with("Couldn't start Claude Code: ")),
                "{seen:?}"
            );
            assert_eq!(
                seen[1],
                AcpEvent::Done {
                    stop_reason: "error".into()
                }
            );
        }

        #[test]
        fn stop_interrupts_claude_code_and_the_turn_reads_cancelled() {
            let dir = scratch("stop");
            // Like Claude Code: SIGINT ends the turn with an error result.
            let body = format!(
                "{}trap '{}; exit 0' INT\nsleep 20 >/dev/null 2>&1 &\nwait\necho never",
                lines_of(&[&init(SID)]),
                r#"echo '{"type":"result","subtype":"error_during_execution","is_error":true}'"#
            );
            let cli = fake_cli(&dir, &body);
            let cancel = CancelToken::new();
            let mut e = ClaudePlanEngine::new(cli, dir.clone(), cancel.clone());
            e.session = SID.into();
            let stopper = {
                let cancel = cancel.clone();
                thread::spawn(move || {
                    thread::sleep(Duration::from_millis(500));
                    cancel.cancel();
                })
            };
            let started = std::time::Instant::now();
            let seen = run(&mut e, "write a long story");
            stopper.join().unwrap();
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "{:?}",
                started.elapsed()
            );
            assert_eq!(
                seen.first(),
                Some(&AcpEvent::Ready {
                    session_id: SID.into()
                })
            );
            assert_eq!(
                seen.last(),
                Some(&AcpEvent::Done {
                    stop_reason: "cancelled".into()
                })
            );
            assert!(
                !seen.iter().any(|ev| matches!(ev, AcpEvent::Err(_))),
                "{seen:?}"
            );
            // The interrupted turn still started the session.
            assert_eq!(session_flag(&e.args()).0, "--resume");
        }
    }
}
