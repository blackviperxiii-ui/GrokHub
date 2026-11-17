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

use grokhub_core::wire::{AcpEvent, GrokUsage};
use serde_json::{json, Value};

use super::stream::{Headroom, Outcome, Stream};
use crate::client::{CancelToken, ContentPart, InputItem, Usage};
use crate::events::Engine;
use crate::run::SteerQueue;

/// The only tools a chat on the plan has until live approvals land. Claude
/// Code's own rules still decide each call: reading inside the chat's folder
/// needs no approval, and reading outside it would, so it's refused.
pub const READ_ONLY_TOOLS: [&str; 3] = ["Read", "Grep", "Glob"];

/// How much of the end of Claude Code's stderr is kept for an error line.
const STDERR_TAIL: usize = 4_096;
/// How often a running turn looks at Stop.
const STOP_POLL: Duration = Duration::from_millis(100);

pub struct ClaudePlanEngine {
    cli: PathBuf,
    workspace: PathBuf,
    /// This chat's Claude Code session: named on the first turn, resumed after.
    session: String,
    /// A turn has started the session, so the next one resumes it.
    started: bool,
    cancel: CancelToken,
    /// Words sent while a turn runs. Claude Code can't take them mid-turn, so
    /// they go back to the chat at the turn's end, each its own message.
    steer: SteerQueue,
    /// The chat whose history file each turn is saved to, as native turns
    /// are; `None` saves nothing.
    chat: Option<String>,
    /// The plan's newest limit reading and when it came (Unix seconds),
    /// until the host takes it.
    headroom: Option<(Headroom, u64)>,
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// How a saved turn names its model and meter in History.
pub const MODEL: &str = "claude-plan";
pub const METER: &str = "Claude plan";

impl ClaudePlanEngine {
    /// `cli` comes from `claude_plan::find`; `cancel` is the chat's Stop.
    pub fn new(cli: PathBuf, workspace: PathBuf, cancel: CancelToken, steer: SteerQueue) -> Self {
        Self {
            cli,
            workspace,
            session: new_session_id(),
            started: false,
            cancel,
            steer,
            chat: None,
            headroom: None,
        }
    }

    /// Saves each turn to `chat`'s history, so it shows in History and the
    /// app's own models can read it if the chat is switched back.
    pub fn for_chat(mut self, chat: &str) -> Self {
        self.chat = Some(chat.to_string());
        self
    }

    /// Goes on with a Claude Code session an earlier run of the app started
    /// for this chat.
    pub fn resume_session(&mut self, session: &str) {
        self.session = session.to_string();
        self.started = true;
    }

    /// The session the next turn resumes, once a turn started one.
    pub fn session(&self) -> Option<&str> {
        self.started.then_some(self.session.as_str())
    }

    /// The folder turns run in, which the session lives under.
    pub fn workspace(&self) -> &std::path::Path {
        &self.workspace
    }

    /// Claude Code keeps a session under its folder, so a new folder starts
    /// a new session.
    pub fn set_workspace(&mut self, workspace: PathBuf) {
        if workspace != self.workspace {
            self.new_session();
        }
        self.workspace = workspace;
    }

    /// Where `claude` is now, found again for each turn.
    pub fn set_cli(&mut self, cli: PathBuf) {
        self.cli = cli;
    }

    /// The plan's limit reading from the last turn that sent one, once.
    pub fn take_headroom(&mut self) -> Option<(Headroom, u64)> {
        self.headroom.take()
    }

    /// Starts a new Claude Code session on the next turn.
    pub fn start_new_session(&mut self) {
        self.new_session();
    }

    fn new_session(&mut self) {
        self.session = new_session_id();
        self.started = false;
    }

    /// The command line for one turn. Only the read-only tools exist, and no
    /// MCP server from the user's own config is loaded. Permission prompts go
    /// nowhere, so a call that would ask is refused rather than waiting on a
    /// question nobody sees; the permission mode is named so a
    /// `bypassPermissions` default in the user's Claude Code settings doesn't
    /// apply here.
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
            "--strict-mcp-config",
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
        args.push("--tools".into());
        args.extend(READ_ONLY_TOOLS.iter().map(|t| t.to_string()));
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
        let errors = thread::spawn(move || tail_of(stderr, STDERR_TAIL));

        let mut stream = Stream::new();
        let mut said_error = false;
        let mut reply = String::new();
        let mut usage = None;
        let mut after_tool = false;
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            let events = stream.line(&line);
            if let Some(h) = stream.take_fresh_headroom() {
                // Stamped as it comes, so a chat that ends later with an
                // older reading doesn't replace a newer one from another.
                self.headroom = Some((h, unix_now()));
            }
            for event in events {
                said_error |= matches!(event, AcpEvent::Err(_));
                match &event {
                    AcpEvent::Text(text) => {
                        // Text after a tool call is the next message: kept apart.
                        if after_tool && !reply.is_empty() {
                            reply.push_str("\n\n");
                        }
                        after_tool = false;
                        reply.push_str(text);
                    }
                    AcpEvent::Tool(_) => after_tool = true,
                    AcpEvent::Usage(used) => usage = Some(used.clone()),
                    _ => {}
                }
                emit(event);
            }
        }
        finished.store(true, Ordering::SeqCst);
        let _ = watcher.join();
        if let Ok(mut child) = child.lock() {
            let _ = child.wait();
        }
        let errors = errors.join().unwrap_or_default();

        self.save_turn(text, &reply, usage.as_ref());
        let resumed = self.started;
        if stream.session_id() == Some(self.session.as_str()) {
            self.started = true;
        } else if resumed && *stream.outcome() != Outcome::Done {
            // The session couldn't be resumed (moved or removed): the next
            // message starts a new one rather than failing the same way.
            self.new_session();
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
            if said_error {
                // The stream already said why in plain words.
                return emit(AcpEvent::Done {
                    stop_reason: "error".into(),
                });
            }
            let why = match last_line(&errors) {
                Some(said) if no_result => format!("{why} It said: {said}"),
                _ => why,
            };
            fail(emit, why);
        }
    }
}

impl ClaudePlanEngine {
    /// The turn's message and reply go to the chat's history file with its
    /// usage; a turn that wrote no reply (stopped early, or failed) isn't kept.
    fn save_turn(&self, text: &str, reply: &str, used: Option<&GrokUsage>) {
        let Some(chat) = &self.chat else { return };
        if reply.trim().is_empty() {
            return;
        }
        let message = |role: &str, said: &str| InputItem::Message {
            role: role.into(),
            content: vec![ContentPart::InputText(said.to_string())],
        };
        let usage = used.map(plan_usage).unwrap_or_default();
        let _ = crate::session::record_turn(
            chat,
            &self.workspace.display().to_string(),
            MODEL,
            &[message("user", text), message("assistant", reply)],
            &usage,
            METER,
            text,
        );
    }
}

/// A plan turn's tokens as the history counts them: Claude's input count
/// leaves out cached tokens, which the app's count includes. A plan costs no
/// money per turn, so no cost is kept.
fn plan_usage(used: &GrokUsage) -> Usage {
    Usage {
        input_tokens: used
            .input_tokens
            .saturating_add(used.cache_read_input_tokens)
            .saturating_add(used.cache_creation_input_tokens),
        output_tokens: used.output_tokens,
        reasoning_tokens: used.reasoning_tokens,
        cost_in_usd_ticks: 0,
        cached_tokens: used.cache_read_input_tokens,
    }
}

impl Engine for ClaudePlanEngine {
    /// A Stop that came before the turn started (while the chat checked
    /// Claude Code's sign-in) ends it before anything is sent. The chat
    /// worker clears Stop when it takes the message.
    fn prompt(
        &mut self,
        text: &str,
        image: Option<&str>,
        emit: &mut dyn FnMut(AcpEvent),
    ) -> Result<(), String> {
        let steer = self.steer.clone();
        hand_back(&steer, emit);
        if self.cancel.is_cancelled() {
            emit(AcpEvent::Done {
                stop_reason: "cancelled".into(),
            });
            return Ok(());
        }
        self.run(text, image, &mut |event| {
            if matches!(event, AcpEvent::Done { .. }) {
                hand_back(&steer, emit);
            }
            emit(event);
        });
        Ok(())
    }

    fn cancel(&self) {
        self.cancel.cancel();
    }
}

fn hand_back(steer: &SteerQueue, emit: &mut dyn FnMut(AcpEvent)) {
    let left = steer.drain();
    if !left.is_empty() {
        emit(AcpEvent::Unsteered(left));
    }
}

/// Ends the turn with `why`, the way native turns end on an error.
fn fail(emit: &mut dyn FnMut(AcpEvent), why: String) {
    emit(AcpEvent::Err(why));
    emit(AcpEvent::Done {
        stop_reason: "error".into(),
    });
}

/// Reads `from` to its end, so Claude Code never writes into a closed pipe,
/// and keeps the last `cap` bytes.
fn tail_of(mut from: impl Read, cap: usize) -> String {
    let mut kept: Vec<u8> = Vec::new();
    let mut buf = [0u8; 8_192];
    while let Ok(n) = from.read(&mut buf) {
        if n == 0 {
            break;
        }
        kept.extend_from_slice(&buf[..n]);
        if kept.len() > cap {
            kept.drain(..kept.len() - cap);
        }
    }
    String::from_utf8_lossy(&kept).into_owned()
}

/// Stop: SIGINT, so Claude Code ends the turn and records it. Windows has no
/// SIGINT for a console-less child, so the whole process tree is ended (an
/// npm install's `claude.cmd` runs the real CLI as its own child, which holds
/// the pipes) and that turn's result is lost.
fn interrupt(child: &mut Child) {
    #[cfg(unix)]
    if let Ok(pid) = libc::pid_t::try_from(child.id()) {
        // SAFETY: kill(2) on our own child's pid, which we haven't waited on.
        unsafe {
            libc::kill(pid, libc::SIGINT);
        }
    }
    #[cfg(windows)]
    {
        let mut tree = Command::new("taskkill");
        tree.args(["/F", "/T", "/PID", &child.id().to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        grokhub_core::proc_util::hide_windows_console(&mut tree);
        let _ = tree.status();
        let _ = child.kill();
    }
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
            SteerQueue::new(),
        )
    }

    #[test]
    fn a_turn_has_only_the_read_tools_and_never_waits_on_a_prompt() {
        let mut e = engine(Path::new("claude"), Path::new("."));
        e.session = "3f1c2a4e-8b7d-4c6e-9a1b-2d3e4f5a6b7c".into();
        let joined = e.args().join(" ");
        assert_eq!(
            joined,
            "-p --input-format stream-json --output-format stream-json --verbose \
             --include-partial-messages --permission-mode default --permission-prompts none \
             --strict-mcp-config --session-id 3f1c2a4e-8b7d-4c6e-9a1b-2d3e4f5a6b7c \
             --tools Read Grep Glob"
        );
        assert!(
            !joined.contains("--allowedTools"),
            "an allow rule would reach outside the folder"
        );
        e.started = true;
        assert_eq!(
            session_flag(&e.args()),
            ("--resume", "3f1c2a4e-8b7d-4c6e-9a1b-2d3e4f5a6b7c")
        );
    }

    #[test]
    fn another_folder_starts_another_session() {
        let mut e = engine(Path::new("claude"), Path::new("/home/user/my-app"));
        e.started = true;
        let first = e.session.clone();
        e.set_workspace(PathBuf::from("/home/user/my-app"));
        assert_eq!(session_flag(&e.args()), ("--resume", first.as_str()));
        e.set_workspace(PathBuf::from("/home/user/other-app"));
        let args = e.args();
        let (flag, id) = session_flag(&args);
        assert_eq!(flag, "--session-id");
        assert_ne!(id, first);
    }

    #[test]
    fn stderr_is_read_to_its_end_and_only_its_tail_kept() {
        let long = format!("{}\nInvalid API key\n", "x".repeat(20_000));
        let kept = tail_of(long.as_bytes(), 64);
        assert_eq!(kept.len(), 64);
        assert!(kept.ends_with("xxx\nInvalid API key\n"), "{kept:?}");
        assert_eq!(tail_of("short".as_bytes(), 64), "short");
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
        fn a_turn_hands_its_plan_limit_reading_to_the_host_once() {
            let dir = scratch("headroom");
            let limit = r#"{"type":"rate_limit_event","rate_limit_info":{"unifiedWindows":{"seven_day":{"utilization":0.43,"resetsAt":1795341600,"status":"allowed"}}}}"#;
            let cli = fake_cli(&dir, &lines_of(&[&init(SID), limit, OK]));
            let mut e = engine(&cli, &dir);
            e.session = SID.into();
            let before = unix_now();
            run(&mut e, "hi");
            let (reading, seen_at) = e.take_headroom().expect("a reading");
            assert_eq!(
                reading,
                Headroom {
                    window: "seven_day".into(),
                    used_pct: Some(43),
                    resets_at: Some(1_795_341_600),
                    limited: false,
                }
            );
            assert!((before..=unix_now()).contains(&seen_at), "{seen_at}");
            assert_eq!(e.take_headroom(), None);

            let quiet = fake_cli(&dir, &lines_of(&[&init(SID), OK]));
            e.set_cli(quiet);
            run(&mut e, "again");
            assert_eq!(
                e.take_headroom(),
                None,
                "no reading this turn, none handed over"
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
            let mut e = ClaudePlanEngine::new(cli, dir.clone(), cancel.clone(), SteerQueue::new());
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
        #[test]
        fn a_stop_before_the_turn_starts_sends_nothing() {
            let dir = scratch("early-stop");
            let cli = fake_cli(&dir, &lines_of(&[&init(SID), OK]));
            let cancel = CancelToken::new();
            cancel.cancel();
            let mut e = ClaudePlanEngine::new(cli, dir.clone(), cancel, SteerQueue::new());
            assert_eq!(
                run(&mut e, "hi"),
                vec![AcpEvent::Done {
                    stop_reason: "cancelled".into()
                }]
            );
            assert!(!dir.join("stdin").exists(), "Claude Code never ran");
        }

        #[test]
        fn words_sent_during_a_turn_come_back_before_it_ends() {
            let dir = scratch("steer");
            let body = format!("{}sleep 1\n{}", lines_of(&[&init(SID)]), lines_of(&[OK]));
            let cli = fake_cli(&dir, &body);
            let steer = SteerQueue::new();
            let mut e = ClaudePlanEngine::new(cli, dir.clone(), CancelToken::new(), steer.clone());
            steer.push("left from before");
            let late = {
                let steer = steer.clone();
                thread::spawn(move || {
                    thread::sleep(Duration::from_millis(300));
                    steer.push("also check b.txt");
                })
            };
            let seen = run(&mut e, "hi");
            late.join().unwrap();
            let shown: Vec<AcpEvent> = seen
                .into_iter()
                .filter(|ev| !matches!(ev, AcpEvent::Usage(_)))
                .collect();
            assert_eq!(
                shown,
                vec![
                    AcpEvent::Unsteered(vec!["left from before".into()]),
                    AcpEvent::Ready {
                        session_id: SID.into()
                    },
                    AcpEvent::Unsteered(vec!["also check b.txt".into()]),
                    AcpEvent::Done {
                        stop_reason: "end_turn".into()
                    },
                ]
            );
        }

        #[test]
        fn a_signed_out_turn_says_so_once_in_plain_words() {
            let dir = scratch("signed-out");
            let cli = fake_cli(
                &dir,
                &lines_of(&[
                    &init(SID),
                    r#"{"type":"assistant","parent_tool_use_id":null,"error":"authentication_failed","message":{"content":[{"type":"text","text":"Invalid API key · Please run /login"}]}}"#,
                    r#"{"type":"result","subtype":"success","is_error":true,"result":"Invalid API key · Please run /login","usage":{}}"#,
                ]),
            );
            let mut e = engine(&cli, &dir);
            e.session = SID.into();
            let errors: Vec<AcpEvent> = run(&mut e, "hi")
                .into_iter()
                .filter(|ev| matches!(ev, AcpEvent::Err(_) | AcpEvent::Done { .. }))
                .collect();
            assert_eq!(
                errors,
                vec![
                    AcpEvent::Err(
                        "Claude Code isn't signed in. In a terminal, run claude auth login, then try again."
                            .into()
                    ),
                    AcpEvent::Done {
                        stop_reason: "error".into()
                    },
                ]
            );
        }

        #[test]
        fn a_session_that_cant_be_resumed_starts_a_new_one_next_time() {
            let dir = scratch("lost-session");
            let cli = fake_cli(
                &dir,
                "echo 'No conversation found with session ID' >&2\nexit 1",
            );
            let mut e = engine(&cli, &dir);
            e.session = SID.into();
            e.started = true;
            let seen = run(&mut e, "hi");
            assert_eq!(
                seen.last(),
                Some(&AcpEvent::Done {
                    stop_reason: "error".into()
                })
            );
            let args = std::fs::read_to_string(dir.join("args")).unwrap();
            assert!(args.contains(&format!("--resume {SID} ")), "{args}");
            let next = e.args();
            let (flag, id) = session_flag(&next);
            assert_eq!(flag, "--session-id");
            assert_ne!(id, SID);
        }

        #[test]
        fn a_chatty_stderr_never_stops_the_turn() {
            let dir = scratch("chatty");
            let body = format!(
                "head -c 200000 /dev/zero | tr '\\0' x >&2\n{}",
                lines_of(&[&init(SID), OK])
            );
            let cli = fake_cli(&dir, &body);
            let mut e = engine(&cli, &dir);
            e.session = SID.into();
            assert_eq!(
                run(&mut e, "hi").last(),
                Some(&AcpEvent::Done {
                    stop_reason: "end_turn".into()
                })
            );
        }
        #[test]
        fn each_turn_is_saved_to_the_chat_history_and_a_failed_one_is_not() {
            let dir = scratch("history");
            let _config = crate::perm::ConfigGuard::set(&dir);
            let delta = r#"{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"pong"}}}"#;
            let ok = r#"{"type":"result","subtype":"success","is_error":false,"usage":{"input_tokens":3,"output_tokens":2,"cache_read_input_tokens":40}}"#;
            let cli = fake_cli(&dir, &lines_of(&[&init(SID), delta, ok]));
            let chat = "shell-plan-history";
            let mut e = engine(&cli, &dir).for_chat(chat);
            e.session = SID.into();
            assert_eq!(e.session(), None);
            run(&mut e, "ping");
            assert_eq!(e.session(), Some(SID));

            let info = crate::session::load_session(chat).unwrap();
            assert_eq!(
                crate::session::message_pairs(&info),
                vec![
                    ("user".to_string(), "ping".to_string()),
                    ("assistant".to_string(), "pong".to_string()),
                ]
            );
            assert_eq!(info.model, MODEL);
            assert_eq!(info.meter, METER);
            // Claude's input count leaves out the cached 40; the app's includes them.
            assert_eq!(
                (
                    info.usage.input_tokens,
                    info.usage.output_tokens,
                    info.usage.cached_tokens
                ),
                (43, 2, 40)
            );

            // A turn with no reply leaves the file as it was.
            std::fs::write(&cli, "#!/bin/sh\ncat >/dev/null\nexit 1\n").unwrap();
            run(&mut e, "again");
            let info = crate::session::load_session(chat).unwrap();
            assert_eq!(crate::session::message_pairs(&info).len(), 2);
        }

        #[test]
        fn a_session_from_an_earlier_run_is_resumed() {
            let dir = scratch("resume-saved");
            let cli = fake_cli(&dir, &lines_of(&[&init(SID), OK]));
            let mut e = engine(&cli, &dir);
            e.resume_session(SID);
            run(&mut e, "hi");
            let args = std::fs::read_to_string(dir.join("args")).unwrap();
            assert!(args.contains(&format!("--resume {SID} ")), "{args}");
        }
        #[test]
        fn a_reply_around_a_tool_call_is_saved_as_separate_paragraphs() {
            let dir = scratch("paragraphs");
            let _config = crate::perm::ConfigGuard::set(&dir);
            let say = |t: &str| {
                format!(
                    r#"{{"type":"stream_event","event":{{"type":"content_block_delta","delta":{{"type":"text_delta","text":"{t}"}}}}}}"#
                )
            };
            let call = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"t1","name":"Glob","input":{"pattern":"*.txt"}}]}}"#;
            let done = r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t1","content":"a.txt"}]}}"#;
            let cli = fake_cli(
                &dir,
                &lines_of(&[
                    &init(SID),
                    &say("Let me "),
                    &say("look."),
                    call,
                    done,
                    &say("One file."),
                    OK,
                ]),
            );
            let chat = "shell-plan-paragraphs";
            let mut e = engine(&cli, &dir).for_chat(chat);
            e.session = SID.into();
            run(&mut e, "how many?");
            let info = crate::session::load_session(chat).unwrap();
            assert_eq!(
                crate::session::message_pairs(&info)[1],
                (
                    "assistant".to_string(),
                    "Let me look.\n\nOne file.".to_string()
                )
            );
        }
    }
}
