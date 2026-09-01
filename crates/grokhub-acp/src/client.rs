use crate::protocol::{
    elicit_accept, elicit_cancel, elicit_decline, encode_line, initialize_params, method_not_found,
    parse_elicit, parse_elicit_complete, parse_permission, parse_session_update, permission_allow,
    permission_allow_always, permission_deny, pick_auth_method, prompt_params_with_image, request,
    response, session_load_params, session_new_params, AcpEvent, JsonRpc,
};
use crate::protocol::SessionMode;
use crate::{
    agent_args, cabin_grok_home, cabin_leader_socket, find_grok, grok_home, grok_stdout_timeout,
    prepare_cabin_grok_home,
};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStderr, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

/// Default cap on `initialize` / `authenticate` / `session/new` so a silent
/// `grok` cannot freeze the cabin UI thread.
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(12);

/// Pull the human message out of a JSON-RPC error object or string.
pub fn jsonrpc_error_text(err: &Value) -> String {
    if let Some(s) = err.as_str() {
        let t = s.trim();
        if !t.is_empty() {
            return t.to_string();
        }
    }
    if let Some(msg) = err.get("message").and_then(|v| v.as_str()) {
        let t = msg.trim();
        if !t.is_empty() {
            if let Some(data) = err.get("data").and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty()) {
                if !t.contains(data) {
                    return format!("{t}: {data}");
                }
            }
            return t.to_string();
        }
    }
    if let Some(data) = err.get("data").and_then(|v| v.as_str()) {
        let t = data.trim();
        if !t.is_empty() {
            return t.to_string();
        }
    }
    err.to_string()
}

pub fn is_session_cwd_error(err: &str) -> bool {
    let l = err.to_ascii_lowercase();
    l.contains("os error 28")
        || l.contains("no space left")
        || l.contains("disk is full")
        || l.contains("os error 13")
        || l.contains("permission denied")
        || l.contains("cannot write")
        || l.contains("os error 30")
        || l.contains("read-only")
}

pub fn explain_handshake_error(raw: &str, cwd: &Path) -> String {
    let raw = raw.trim();
    let loc = cwd.display();
    let load = raw.to_ascii_lowercase().contains("session/load");
    let verb = if load { "session/load" } else { "session/new" };
    if raw.is_empty() {
        return format!("ACP {verb} failed in {loc}");
    }
    let l = raw.to_ascii_lowercase();
    if l.contains("no space left") || l.contains("os error 28") {
        return format!(
            "ACP {verb} failed: disk is full while starting Grok Build in {loc}. Free space (check ~/.grok and {loc}), then send again."
        );
    }
    if l.contains("permission denied") || l.contains("os error 13") || l.contains("read-only") {
        return format!(
            "ACP {verb} failed: Grok Build cannot write in {loc}. Bind a folder you own (sidebar) or /project bind ~/GrokHub-Work, then send again."
        );
    }
    if l.contains("unauthorized (401)") || l.contains("invalid or expired") {
        return format!(
            "Grok Build auth failed (401). Run grok login, then send again.\n{raw}"
        );
    }
    if (l.contains("agent closed") || l.contains("agent exited")) && !l.contains("during handshake") {
        return format!("Grok Build agent exited: {raw}");
    }
    if raw.starts_with("ACP ") {
        return raw.to_string();
    }
    format!("ACP {verb} failed: {raw}")
}

/// Create `path` and probe a write so session/new does not start on a missing or read-only tree.
pub fn ensure_session_cwd(path: &Path) -> Result<PathBuf, String> {
    if let Ok(held) = cwd_probe_cache().lock() {
        if let Some((p, at, inflight)) = held.as_ref() {
            if p == path {
                let hit = path.to_path_buf();
                let fresh = at.elapsed() < Duration::from_secs(5);
                let busy = *inflight;
                drop(held);
                if !fresh && !busy {
                    kick_session_cwd(path.to_path_buf());
                }
                return Ok(hit);
            }
        }
    }
    ensure_session_cwd_now(path)
}

fn ensure_session_cwd_now(path: &Path) -> Result<PathBuf, String> {
    std::fs::create_dir_all(path)
        .map_err(|e| format!("ACP cwd {}: {e}", path.display()))?;
    let probe = path.join(".grokhub-cwd-ok");
    match std::fs::write(&probe, b"ok") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            if let Ok(mut held) = cwd_probe_cache().lock() {
                *held = Some((path.to_path_buf(), Instant::now(), false));
            }
            Ok(path.to_path_buf())
        }
        Err(e) => {
            let _ = std::fs::remove_file(&probe);
            if let Ok(mut held) = cwd_probe_cache().lock() {
                if let Some(slot) = held.as_mut() {
                    if slot.0 == path {
                        slot.2 = false;
                    }
                }
            }
            Err(format!("ACP cwd {}: {e}", path.display()))
        }
    }
}

fn kick_session_cwd(path: PathBuf) {
    if let Ok(mut held) = cwd_probe_cache().lock() {
        if let Some(slot) = held.as_mut() {
            if slot.0 == path {
                if slot.2 {
                    return;
                }
                slot.2 = true;
            }
        }
    }
    thread::spawn(move || {
        let _ = ensure_session_cwd_now(&path);
    });
}

fn cwd_probe_cache() -> &'static Mutex<Option<(PathBuf, Instant, bool)>> {
    static C: OnceLock<Mutex<Option<(PathBuf, Instant, bool)>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(None))
}

enum Cmd {
    Prompt { text: String, image: Option<String> },
    Cancel,
    Permission { id: Value, allow: bool, always: bool },
    Elicit {
        id: Value,
        outcome: &'static str,
        content: Option<Value>,
    },
    Reject { id: Value },
    Ack { id: Value },
    Shutdown,
}

/// Long-lived `grok agent stdio` session.
pub struct AcpHandle {
    child: Arc<Mutex<Option<Child>>>,
    cmd: Sender<Cmd>,
    pub events: Receiver<AcpEvent>,
    pub session_id: String,
    pub cwd: PathBuf,
}

#[derive(Debug, Clone)]
pub struct SpawnOpts {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub api_key: Option<String>,
    /// Console key for `XAI_API_KEY`. Never a grok-login JWT.
    pub xai_api_key: Option<String>,
    pub always_approve: bool,
    pub auto: bool,
    pub session_mode: SessionMode,
    pub reasoning_effort: Option<String>,
    pub extra_env: Vec<(String, String)>,
    pub handshake_timeout: Option<Duration>,
    pub resume: Option<String>,
    pub skip_cabin_home: bool,
    pub worktree: bool,
}

impl SpawnOpts {
    pub fn grok(
        cwd: PathBuf,
        api_key: Option<String>,
        always_approve: bool,
        auto: bool,
        session_mode: SessionMode,
        reasoning_effort: Option<String>,
    ) -> Result<Self, String> {
        let program = find_grok().ok_or_else(|| {
            "Grok Build CLI missing — install from x.ai/cli or set GROKHUB_GROK".to_string()
        })?;
        Ok(Self {
            args: agent_args(always_approve, reasoning_effort.as_deref()),
            program,
            cwd,
            api_key,
            xai_api_key: None,
            always_approve,
            auto,
            session_mode,
            reasoning_effort,
            extra_env: Vec::new(),
            handshake_timeout: None,
            resume: None,
            skip_cabin_home: false,
            worktree: false,
        })
    }

    pub fn with_resume(mut self, id: Option<String>) -> Self {
        let id = id.and_then(|s| {
            let t = s.trim().to_string();
            if t.is_empty() {
                None
            } else {
                Some(t)
            }
        });
        self.resume = id;
        // ACP session/load is the resume path. CLI --resume plus a later
        // session/new on the same child mixed sessions.
        self.args = agent_args(self.always_approve, self.reasoning_effort.as_deref());
        self
    }

    pub fn with_xai_api_key(mut self, key: Option<String>) -> Self {
        self.xai_api_key = key.and_then(|s| {
            let t = s.trim().to_string();
            if t.is_empty() {
                None
            } else {
                Some(t)
            }
        });
        self
    }
}

fn write_msg(stdin: &mut impl Write, msg: &JsonRpc) -> Result<(), String> {
    stdin
        .write_all(encode_line(msg).as_bytes())
        .map_err(|e| e.to_string())?;
    stdin.flush().map_err(|e| e.to_string())
}

fn drain_stderr(stderr: ChildStderr) -> Arc<Mutex<String>> {
    let tail = Arc::new(Mutex::new(String::new()));
    let slot = tail.clone();
    thread::spawn(move || {
        let mut reader = stderr;
        let mut buf = [0u8; 512];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Ok(n) => {
                    if let Ok(mut held) = slot.lock() {
                        let chunk = String::from_utf8_lossy(&buf[..n]);
                        for ch in chunk.chars() {
                            if ch == '\0' || ch == '\u{fffd}' {
                                continue;
                            }
                            held.push(ch);
                        }
                        trim_stderr_tail(&mut held, STDERR_TAIL_CAP);
                    }
                }
                Err(_) => break,
            }
        }
    });
    tail
}

const STDERR_TAIL_CAP: usize = 4096;

/// Keep roughly the last `cap` bytes of the stderr tail.
///
/// `String::drain` panics unless the index is a char boundary, and grok writes multi-byte
/// spinner and box-drawing glyphs, so a byte offset lands mid-character two times in three.
/// That panic used to poison the tail mutex and, worse, kill the only thread draining
/// stderr — after which the child blocked on a full pipe and the turn hung forever.
fn trim_stderr_tail(held: &mut String, cap: usize) {
    if held.len() <= cap * 2 {
        return;
    }
    let mut cut = held.len() - cap;
    while cut < held.len() && !held.is_char_boundary(cut) {
        cut += 1;
    }
    held.drain(..cut);
}

fn format_exit_status(st: ExitStatus) -> String {
    if let Some(code) = st.code() {
        return format!("exit {code}");
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(sig) = st.signal() {
            return format!("signal {sig}");
        }
    }
    st.to_string()
}

/// SIGTERM (128+15). The GUI/leader kills `grok agent stdio` this way.
pub fn is_sigterm_status(s: &str) -> bool {
    let l = s.to_ascii_lowercase();
    l.contains("exit 143")
        || l.contains("signal 15")
        || l.contains("sigterm")
        || (l.contains("agent closed") && l.contains("143"))
}

fn wait_status_text(child: &Arc<Mutex<Option<Child>>>) -> Option<String> {
    let mut slot = child.lock().ok()?;
    let c = slot.as_mut()?;
    match c.try_wait() {
        Ok(Some(st)) => Some(format_exit_status(st)),
        Ok(None) => {
            thread::sleep(Duration::from_millis(30));
            c.try_wait()
                .ok()
                .flatten()
                .map(format_exit_status)
        }
        Err(_) => None,
    }
}

fn with_stderr(msg: String, tail: &Arc<Mutex<String>>) -> String {
    let extra = tail
        .lock()
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    match extra {
        Some(e) => format!("{msg}\n{e}"),
        None => msg,
    }
}

fn read_until_result(
    stdin: &mut impl Write,
    reader: &mut BufReader<impl Read>,
    want: u64,
    pending_perm: &mut Vec<Value>,
    pending_elicit: &mut Vec<Value>,
) -> Result<Value, String> {
    let mut buf = String::new();
    loop {
        buf.clear();
        let n = reader.read_line(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            return Err("agent closed during handshake".into());
        }
        let line = buf.trim();
        if line.is_empty() {
            continue;
        }
        let msg: JsonRpc = serde_json::from_str(line).map_err(|e| format!("acp json: {e}"))?;
        if let Some(method) = &msg.method {
            if method == "session/update" {
                // Consume load replay. Do not treat it as a live turn.
                continue;
            }
            if method == "session/request_permission" {
                if let Some(id) = msg.id {
                    pending_perm.push(id);
                }
                continue;
            }
            if method == "x.ai/mcp/elicit" {
                if let Some(id) = msg.id {
                    pending_elicit.push(id);
                }
                continue;
            }
            if method == "x.ai/mcp/elicit_complete" {
                continue;
            }
            if let Some(id) = rpc_reply_id(msg.id.clone()) {
                if method.starts_with("_x.ai/") {
                    let _ = write_msg(stdin, &response(id, json!({})));
                } else {
                    let _ = write_msg(stdin, &method_not_found(id));
                }
                continue;
            }
            continue;
        }
        if msg.id.as_ref().and_then(|v| v.as_u64()) == Some(want) {
            if let Some(err) = msg.error {
                return Err(jsonrpc_error_text(&err));
            }
            return Ok(msg.result.unwrap_or(json!({})));
        }
    }
}

struct HandshakeOk {
    stdin: std::process::ChildStdin,
    reader: BufReader<std::process::ChildStdout>,
    session_id: String,
    next_id: u64,
    pending_perm: Vec<Value>,
    pending_elicit: Vec<Value>,
}

fn handshake(
    mut stdin: std::process::ChildStdin,
    stdout: std::process::ChildStdout,
    api_key: &str,
    cwd: &str,
    always_approve: bool,
    auto: bool,
    session_mode: SessionMode,
    resume: Option<String>,
) -> Result<HandshakeOk, String> {
    let mut reader = BufReader::new(stdout);
    let mut next_id = 1u64;
    let mut pending_perm = Vec::new();
    let mut pending_elicit = Vec::new();
    write_msg(&mut stdin, &request(next_id, "initialize", initialize_params()))?;
    let init = read_until_result(
        &mut stdin,
        &mut reader,
        next_id,
        &mut pending_perm,
        &mut pending_elicit,
    )?;
    next_id += 1;
    let methods = init.get("authMethods").cloned().unwrap_or(json!([]));
    if let Some(method_id) = pick_auth_method(&methods, api_key) {
        write_msg(
            &mut stdin,
            &request(
                next_id,
                "authenticate",
                json!({ "methodId": method_id, "_meta": { "headless": true } }),
            ),
        )?;
        let _ = read_until_result(
            &mut stdin,
            &mut reader,
            next_id,
            &mut pending_perm,
            &mut pending_elicit,
        )?;
        next_id += 1;
    }
    let resume_id = resume.and_then(|s| {
        let t = s.trim().to_string();
        if t.is_empty() {
            None
        } else {
            Some(t)
        }
    });
    let created = if let Some(id) = resume_id.clone() {
        write_msg(
            &mut stdin,
            &request(
                next_id,
                "session/load",
                session_load_params(cwd, &id, always_approve, auto, session_mode),
            ),
        )?;
        match read_until_result(
            &mut stdin,
            &mut reader,
            next_id,
            &mut pending_perm,
            &mut pending_elicit,
        ) {
            Ok(v) => {
                next_id += 1;
                v
            }
            Err(e) => {
                return Err(format!("session/load failed: {e}"));
            }
        }
    } else {
        write_msg(
            &mut stdin,
            &request(
                next_id,
                "session/new",
                session_new_params(cwd, always_approve, auto, session_mode),
            ),
        )?;
        let v = read_until_result(
            &mut stdin,
            &mut reader,
            next_id,
            &mut pending_perm,
            &mut pending_elicit,
        )?;
        next_id += 1;
        v
    };
    let session_id = created
        .get("sessionId")
        .or_else(|| created.get("session_id"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
        .or(resume_id)
        .ok_or("session/new missing sessionId")?;
    Ok(HandshakeOk {
        stdin,
        reader,
        session_id,
        next_id,
        pending_perm,
        pending_elicit,
    })
}

#[cfg(unix)]
fn ignore_sigpipe() {
    // SIGPIPE=13, SIG_IGN=1. exec preserves SIG_IGN, so a closed log pipe
    // cannot kill Grok Build mid-turn.
    extern "C" {
        fn signal(signum: i32, handler: usize) -> usize;
    }
    unsafe {
        let _ = signal(13, 1);
    }
}

/// JSON-RPC notifications sometimes include `"id": null`. That is not a request.
fn rpc_reply_id(id: Option<Value>) -> Option<Value> {
    match id {
        Some(v) if !v.is_null() => Some(v),
        _ => None,
    }
}

fn isolate_spawned_grok() {
    ignore_sigpipe();
    // The cabin GUI holds DRI/Wayland fds. Grok Build inherits them and its
    // leader has SIGTERM'd the stdio child (exit 143) ~70ms after inference.
    let mut extra = Vec::new();
    if let Ok(dir) = std::fs::read_dir("/proc/self/fd") {
        for ent in dir.flatten() {
            if let Ok(n) = ent.file_name().to_string_lossy().parse::<i32>() {
                if n > 2 {
                    extra.push(n);
                }
            }
        }
    }
    extern "C" {
        fn close(fd: i32) -> i32;
        fn setsid() -> i32;
    }
    unsafe {
        for fd in extra {
            let _ = close(fd);
        }
        let _ = setsid();
    }
}

/// Spawn and handshake. Puts `XAI_API_KEY` on the child when provided.
pub fn connect(opts: SpawnOpts) -> Result<AcpHandle, String> {
    let timeout = opts.handshake_timeout.unwrap_or(HANDSHAKE_TIMEOUT);
    let cwd_path = ensure_session_cwd(&opts.cwd)
        .map_err(|e| explain_handshake_error(&e, &opts.cwd))?;
    let mut cmd = Command::new(&opts.program);
    cmd.args(&opts.args)
        .current_dir(&cwd_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("GROK_NO_AUTO_UPDATE", "1");
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        unsafe {
            cmd.pre_exec(|| {
                isolate_spawned_grok();
                Ok(())
            });
        }
    }
    if let Some(key) = &opts.xai_api_key {
        if !key.is_empty() {
            cmd.env("XAI_API_KEY", key);
        }
    }
    for (k, v) in &opts.extra_env {
        cmd.env(k, v);
    }
    // Interactive `grok` owns ~/.grok/leader.sock. Sharing it SIGTERMs this
    // child (exit 143) when the CLI leader evicts a "stale" stdio agent.
    if opts.args.iter().any(|a| a == "stdio")
        && !opts.extra_env.iter().any(|(k, _)| k == "GROK_LEADER_SOCKET")
    {
        if let Some(sock) = cabin_leader_socket() {
            cmd.arg("--leader-socket").arg(&sock);
            cmd.env("GROK_LEADER_SOCKET", &sock);
        }
    }
    if opts.worktree && !opts.args.iter().any(|a| a == "--worktree") {
        cmd.arg("--worktree");
    }
    if opts.args.iter().any(|a| a == "stdio")
        && !opts.skip_cabin_home
        && !opts.extra_env.iter().any(|(k, _)| k == "GROK_HOME")
    {
        if let Some(dir) = prepare_cabin_grok_home() {
            cmd.env("GROK_HOME", &dir);
        }
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("spawn {}: {e}", opts.program.display()))?;
    let stdin = child.stdin.take().ok_or("agent stdin")?;
    let stdout = child.stdout.take().ok_or("agent stdout")?;
    let stderr_tail = child.stderr.take().map(drain_stderr);
    let api_key = opts.api_key.clone().unwrap_or_default();
    let cwd = cwd_path.display().to_string();
    let always_approve = opts.always_approve;
    let auto = opts.auto;
    let session_mode = opts.session_mode;
    let resume = opts.resume.clone();

    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = tx.send(handshake(
            stdin,
            stdout,
            &api_key,
            &cwd,
            always_approve,
            auto,
            session_mode,
            resume,
        ));
    });
    let hs = match rx.recv_timeout(timeout) {
        Ok(Ok(hs)) => hs,
        Ok(Err(e)) => {
            let _ = child.kill();
            thread::spawn(move || {
                let _ = child.wait();
            });
            let e = match &stderr_tail {
                Some(t) => with_stderr(e, t),
                None => e,
            };
            return Err(explain_handshake_error(&e, &cwd_path));
        }
        Err(_) => {
            let _ = child.kill();
            thread::spawn(move || {
                let _ = child.wait();
            });
            let msg = "ACP handshake timed out — grok never answered initialize".to_string();
            return Err(match &stderr_tail {
                Some(t) => with_stderr(msg, t),
                None => msg,
            });
        }
    };
    let HandshakeOk {
        stdin,
        mut reader,
        session_id,
        next_id,
        pending_perm,
        pending_elicit,
    } = hs;

    let stdin = Arc::new(Mutex::new(stdin));
    let id_gen = Arc::new(Mutex::new(next_id));
    let swallow_load = Arc::new(AtomicBool::new(
        opts.resume
            .as_ref()
            .is_some_and(|s| !s.trim().is_empty()),
    ));
    let prompt_rpc = Arc::new(Mutex::new(None::<u64>));
    let (cmd_tx, cmd_rx) = mpsc::channel::<Cmd>();
    let (evt_tx, evt_rx) = mpsc::channel();
    let _ = evt_tx.send(AcpEvent::Ready {
        session_id: session_id.clone(),
    });

    let sid = session_id.clone();
    let stdin_w = stdin.clone();
    let ids = id_gen.clone();
    let swallow_w = swallow_load.clone();
    let prompt_w = prompt_rpc.clone();
    thread::spawn(move || {
        let swallow_load = swallow_w;
        for cmd in cmd_rx {
            let mut stdin = match stdin_w.lock() {
                Ok(s) => s,
                Err(_) => return,
            };
            match cmd {
                Cmd::Shutdown => return,
                Cmd::Cancel => {
                    swallow_load.store(true, Ordering::SeqCst);
                    let id = {
                        let mut n = ids.lock().unwrap();
                        let id = *n;
                        *n += 1;
                        id
                    };
                    let _ = write_msg(
                        &mut *stdin,
                        &request(id, "session/cancel", json!({ "sessionId": sid })),
                    );
                }
                Cmd::Prompt { text, image } => {
                    swallow_load.store(false, Ordering::SeqCst);
                    let id = {
                        let mut n = ids.lock().unwrap();
                        let id = *n;
                        *n += 1;
                        id
                    };
                    if let Ok(mut held) = prompt_w.lock() {
                        *held = Some(id);
                    }
                    let _ = write_msg(
                        &mut *stdin,
                        &request(
                            id,
                            "session/prompt",
                            prompt_params_with_image(&sid, &text, image.as_deref()),
                        ),
                    );
                }
                Cmd::Permission { id, allow, always } => {
                    let msg = if allow {
                        if always {
                            permission_allow_always(id)
                        } else {
                            permission_allow(id)
                        }
                    } else {
                        permission_deny(id)
                    };
                    let _ = write_msg(&mut *stdin, &msg);
                }
                Cmd::Elicit {
                    id,
                    outcome,
                    content,
                } => {
                    let msg = match outcome {
                        "accept" => elicit_accept(id, content),
                        "decline" => elicit_decline(id),
                        _ => elicit_cancel(id),
                    };
                    let _ = write_msg(&mut *stdin, &msg);
                }
                Cmd::Reject { id } => {
                    let _ = write_msg(&mut *stdin, &method_not_found(id));
                }
                Cmd::Ack { id } => {
                    let _ = write_msg(&mut *stdin, &response(id, json!({})));
                }
            }
        }
    });

    let allow_pending = opts.always_approve || opts.auto;
    for id in pending_perm {
        let _ = cmd_tx.send(Cmd::Permission {
            id,
            allow: allow_pending,
            always: opts.always_approve,
        });
    }
    for id in pending_elicit {
        let _ = cmd_tx.send(Cmd::Elicit {
            id,
            outcome: "cancel",
            content: None,
        });
    }

    let cmd_tx_r = cmd_tx.clone();
    let prompt_r = prompt_rpc.clone();
    let stderr_live = stderr_tail.clone();
    let child_slot = Arc::new(Mutex::new(Some(child)));
    let child_live = child_slot.clone();
    thread::spawn(move || {
        let mut buf = String::new();
        loop {
            buf.clear();
            match reader.read_line(&mut buf) {
                Ok(0) => {
                    let status = wait_status_text(&child_live);
                    let mut msg = "agent closed".to_string();
                    if let Some(st) = status {
                        msg = format!("{msg} ({st})");
                    }
                    let msg = match &stderr_live {
                        Some(t) => with_stderr(msg, t),
                        None => msg,
                    };
                    let _ = evt_tx.send(AcpEvent::Err(msg));
                    return;
                }
                Ok(_) => {
                    let line = buf.trim();
                    if line.is_empty() {
                        continue;
                    }
                    let msg: JsonRpc = match serde_json::from_str(line) {
                        Ok(m) => m,
                        Err(_) => {
                            // Grok Build emits _x.ai chatter and huge model-catalog
                            // lines. Killing the session here SIGKILLs grok mid-turn.
                            continue;
                        }
                    };
                    if let Some(method) = &msg.method {
                        if method == "session/update" {
                            if swallow_load.load(Ordering::SeqCst) {
                                continue;
                            }
                            if let Some(ev) =
                                parse_session_update(msg.params.as_ref().unwrap_or(&json!({})))
                            {
                                let _ = evt_tx.send(ev);
                            }
                            continue;
                        }
                        if method == "session/request_permission" {
                            if swallow_load.load(Ordering::SeqCst) {
                                if let Some(id) = msg.id {
                                    let _ = cmd_tx_r.send(Cmd::Permission {
                                        id,
                                        allow: false,
                                        always: false,
                                    });
                                }
                                continue;
                            }
                            if let Some(id) = msg.id {
                                let _ = evt_tx.send(AcpEvent::Permission(parse_permission(
                                    id,
                                    msg.params.as_ref().unwrap_or(&json!({})),
                                )));
                            }
                            continue;
                        }
                        if method == "x.ai/mcp/elicit" {
                            if swallow_load.load(Ordering::SeqCst) {
                                if let Some(id) = msg.id {
                                    let _ = cmd_tx_r.send(Cmd::Elicit {
                                        id,
                                        outcome: "cancel",
                                        content: None,
                                    });
                                }
                                continue;
                            }
                            if let Some(id) = msg.id {
                                let _ = evt_tx.send(AcpEvent::Elicit(parse_elicit(
                                    id,
                                    msg.params.as_ref().unwrap_or(&json!({})),
                                )));
                            }
                            continue;
                        }
                        if method == "x.ai/mcp/elicit_complete" {
                            let (elicitation_id, server_name) =
                                parse_elicit_complete(msg.params.as_ref().unwrap_or(&json!({})));
                            let _ = evt_tx.send(AcpEvent::ElicitComplete {
                                elicitation_id,
                                server_name,
                            });
                            continue;
                        }
                        if let Some(id) = rpc_reply_id(msg.id.clone()) {
                            if method.starts_with("_x.ai/") {
                                let _ = cmd_tx_r.send(Cmd::Ack { id });
                            } else {
                                let _ = cmd_tx_r.send(Cmd::Reject { id });
                            }
                            continue;
                        }
                        continue;
                    }
                    if msg.result.is_some() || msg.error.is_some() {
                        let rpc = msg
                            .id
                            .as_ref()
                            .and_then(|v| v.as_u64())
                            .or_else(|| {
                                msg.id
                                    .as_ref()
                                    .and_then(|v| v.as_i64())
                                    .and_then(|n| u64::try_from(n).ok())
                            });
                        let prompt = prompt_r.lock().ok().and_then(|g| *g);
                        if rpc.is_none() || rpc != prompt {
                            continue;
                        }
                        if let Ok(mut held) = prompt_r.lock() {
                            *held = None;
                        }
                    }
                    if msg.result.is_some() {
                        let reason = msg
                            .result
                            .as_ref()
                            .and_then(|r| r.get("stopReason").or_else(|| r.get("stop_reason")))
                            .and_then(|v| v.as_str())
                            .unwrap_or("end_turn")
                            .to_string();
                        let _ = evt_tx.send(AcpEvent::Done {
                            stop_reason: reason,
                        });
                    }
                    if let Some(err) = msg.error {
                        let _ = evt_tx.send(AcpEvent::Err(jsonrpc_error_text(&err)));
                    }
                }
                Err(e) => {
                    let _ = evt_tx.send(AcpEvent::Err(e.to_string()));
                    return;
                }
            }
        }
    });

    Ok(AcpHandle {
        child: child_slot,
        cmd: cmd_tx,
        events: evt_rx,
        session_id,
        cwd: cwd_path,
    })
}

impl AcpHandle {
    pub fn prompt(&self, text: &str) -> Result<(), String> {
        self.prompt_with_image(text, None)
    }

    pub fn prompt_with_image(&self, text: &str, image: Option<&str>) -> Result<(), String> {
        self.cmd
            .send(Cmd::Prompt {
                text: text.to_string(),
                image: image.filter(|s| !s.trim().is_empty()).map(|s| s.to_string()),
            })
            .map_err(|e| e.to_string())
    }

    pub fn cancel(&self) -> Result<(), String> {
        self.cmd.send(Cmd::Cancel).map_err(|e| e.to_string())
    }

    pub fn answer_permission(&self, id: Value, allow: bool) -> Result<(), String> {
        self.cmd
            .send(Cmd::Permission {
                id,
                allow,
                always: false,
            })
            .map_err(|e| e.to_string())
    }

    pub fn answer_permission_always(&self, id: Value) -> Result<(), String> {
        self.cmd
            .send(Cmd::Permission {
                id,
                allow: true,
                always: true,
            })
            .map_err(|e| e.to_string())
    }

    pub fn answer_elicit(
        &self,
        id: Value,
        outcome: &'static str,
        content: Option<Value>,
    ) -> Result<(), String> {
        self.cmd
            .send(Cmd::Elicit {
                id,
                outcome,
                content,
            })
            .map_err(|e| e.to_string())
    }

    pub fn try_recv(&self) -> Result<AcpEvent, TryRecvError> {
        self.events.try_recv()
    }
}

impl Drop for AcpHandle {
    fn drop(&mut self) {
        {
            use std::io::Write;
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open("/tmp/grokhub-acp-drop.log")
            {
                let _ = writeln!(
                    f,
                    "==== AcpHandle::drop {:?} ====\n{}",
                    std::time::SystemTime::now(),
                    std::backtrace::Backtrace::force_capture()
                );
            }
        }
        let _ = self.cmd.send(Cmd::Shutdown);
        let child = self.child.lock().ok().and_then(|mut slot| slot.take());
        if let Some(mut child) = child {
            thread::spawn(move || {
                if child.try_wait().ok().flatten().is_none() {
                    let _ = child.kill();
                }
                let _ = child.wait();
            });
        }
    }
}

/// One headless `grok -p` turn. The cabin chat stays bound to `session_id`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SingleTurn {
    pub session_id: String,
    pub text: String,
    pub thought: String,
    pub usage: crate::stream::GrokUsage,
    pub stop_reason: String,
}

pub fn parse_single_turn(stdout: &str) -> Result<SingleTurn, String> {
    let trimmed = stdout.trim();
    let json = if let Some(i) = trimmed.find('{') {
        &trimmed[i..]
    } else {
        trimmed
    };
    let v: Value = serde_json::from_str(json).map_err(|e| format!("grok -p json: {e}"))?;
    let session_id = v
        .get("sessionId")
        .or_else(|| v.get("session_id"))
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if session_id.is_empty() {
        return Err("grok -p missing sessionId".into());
    }
    let text = v
        .get("text")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let thought = v
        .get("thought")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if text.is_empty() && thought.is_empty() {
        return Err("grok -p empty reply".into());
    }
    let mut usage = crate::stream::parse_usage(&v);
    if usage.stop_reason.is_empty() {
        usage.stop_reason = v
            .get("stopReason")
            .or_else(|| v.get("stop_reason"))
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
    }
    let stop_reason = usage.stop_reason.clone();
    Ok(SingleTurn {
        session_id,
        text,
        thought,
        usage,
        stop_reason,
    })
}

/// Spawn `grok -p` in the cabin grok home. Not `agent stdio` — that child of
/// the GUI is SIGTERM'd (exit 143) while pushing the model catalog.
pub fn run_single_turn(
    prompt: &str,
    cwd: &Path,
    resume: Option<&str>,
    always_approve: bool,
    auto: bool,
) -> Result<SingleTurn, String> {
    run_single_turn_full(prompt, cwd, resume, always_approve, auto, None, None, false)
}

pub fn run_single_turn_full(
    prompt: &str,
    cwd: &Path,
    resume: Option<&str>,
    always_approve: bool,
    auto: bool,
    model: Option<&str>,
    effort: Option<&str>,
    plan: bool,
) -> Result<SingleTurn, String> {
    match grok_p_once(
        prompt,
        cwd,
        resume,
        always_approve,
        auto,
        model,
        effort,
        plan,
        None,
        false,
    ) {
        Ok(t) => Ok(t),
        Err(e) if resume.is_some() && session_resume_is_missing(&e) => grok_p_once(
            prompt,
            cwd,
            None, // resume: None — session lived in ~/.grok, not cabin GROK_HOME
            always_approve,
            auto,
            model,
            effort,
            plan,
            None,
            false,
        ),
        Err(e) => Err(e),
    }
}

/// Live `grok -p --output-format streaming-json`. Halt kills `pid`.
pub fn spawn_grok_p_stream(
    prompt: &str,
    cwd: &Path,
    resume: Option<&str>,
    always_approve: bool,
    auto: bool,
    model: Option<&str>,
    effort: Option<&str>,
    plan: bool,
    image: Option<&str>,
    fork: bool,
    skip_cabin_home: bool,
    worktree: bool,
) -> Result<(u32, mpsc::Receiver<crate::stream::GrokPEvent>), String> {
    let mut child = grok_p_child(
        prompt,
        cwd,
        resume,
        always_approve,
        auto,
        model,
        effort,
        plan,
        image,
        fork,
        skip_cabin_home,
        worktree,
    )?;
    let pid = child.id();
    let stdout = child.stdout.take().ok_or("grok -p stdout")?;
    // `grok_p_child` pipes stderr, but nothing here used to read it. Once grok filled the
    // 64KB pipe buffer it blocked in `write`, stopped emitting stdout tokens, and the turn
    // hung with no timeout above us. Drain it, and keep the tail for the error message.
    let stderr_tail = child.stderr.take().map(drain_stderr);
    let grok_home = cabin_grok_home();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let reader = BufReader::new(stdout);
        let mut text = String::new();
        let mut thought = String::new();
        let mut session_id = String::new();
        let mut usage = crate::stream::GrokUsage::default();
        let mut stop_reason = String::new();
        for line in reader.lines() {
            let Ok(line) = line else { break };
            match crate::stream::parse_stream_line(&line) {
                Some(crate::stream::GrokPEvent::Text(d)) => {
                    text.push_str(&d);
                    if tx.send(crate::stream::GrokPEvent::Text(d)).is_err() {
                        crate::stream::kill_pid(pid);
                        return;
                    }
                }
                Some(crate::stream::GrokPEvent::Thought(d)) => {
                    thought.push_str(&d);
                    if tx.send(crate::stream::GrokPEvent::Thought(d)).is_err() {
                        crate::stream::kill_pid(pid);
                        return;
                    }
                }
                Some(ev @ crate::stream::GrokPEvent::Tool(_))
                | Some(ev @ crate::stream::GrokPEvent::Plan(_))
                | Some(ev @ crate::stream::GrokPEvent::Compact { .. })
                | Some(ev @ crate::stream::GrokPEvent::Commands(_))
                | Some(ev @ crate::stream::GrokPEvent::Task { .. })
                | Some(ev @ crate::stream::GrokPEvent::Recovering(_)) => {
                    if tx.send(ev).is_err() {
                        crate::stream::kill_pid(pid);
                        return;
                    }
                }
                Some(crate::stream::GrokPEvent::Usage(u)) => {
                    usage.merge(&u);
                    if tx.send(crate::stream::GrokPEvent::Usage(u)).is_err() {
                        crate::stream::kill_pid(pid);
                        return;
                    }
                }
                Some(crate::stream::GrokPEvent::End(t)) => {
                    if !t.session_id.is_empty() {
                        session_id = t.session_id;
                    }
                    if !t.text.is_empty() && text.is_empty() {
                        text = t.text;
                    }
                    usage.merge(&t.usage);
                    if !t.stop_reason.is_empty() {
                        stop_reason = t.stop_reason;
                    }
                }
                Some(crate::stream::GrokPEvent::Err(e)) => {
                    let _ = tx.send(crate::stream::GrokPEvent::Err(e));
                    let _ = child.wait();
                    return;
                }
                None => {}
            }
        }
        let status = child.wait();
        if session_id.is_empty() {
            let st = status
                .ok()
                .map(format_exit_status)
                .unwrap_or_else(|| "grok -p missing sessionId".into());
            if is_sigterm_status(&st) {
                return;
            }
            let st = match stderr_tail.as_ref() {
                Some(tail) => with_stderr(st, tail),
                None => st,
            };
            let _ = tx.send(crate::stream::GrokPEvent::Err(st));
            return;
        }
        if let Some(home) = grok_home {
            if let Some(sig) = load_session_signals(&home, &session_id) {
                usage.merge(&sig);
            }
        }
        let _ = tx.send(crate::stream::GrokPEvent::End(SingleTurn {
            session_id,
            text: text.trim().to_string(),
            thought: thought.trim().to_string(),
            usage,
            stop_reason,
        }));
    });
    Ok((pid, rx))
}

fn grok_p_child(
    prompt: &str,
    cwd: &Path,
    resume: Option<&str>,
    always_approve: bool,
    auto: bool,
    model: Option<&str>,
    effort: Option<&str>,
    plan: bool,
    image: Option<&str>,
    fork: bool,
    skip_cabin_home: bool,
    worktree: bool,
) -> Result<Child, String> {
    let program = find_grok().ok_or_else(|| {
        "Grok Build CLI missing — install from x.ai/cli or set GROKHUB_GROK".to_string()
    })?;
    let cwd_path = ensure_session_cwd(cwd)?;
    let mut args = crate::locate::single_turn_args_full(
        prompt,
        &cwd_path.display().to_string(),
        resume,
        always_approve,
        auto,
        model,
        effort,
        plan,
    );
    if image.is_some() {
        args = crate::locate::with_prompt_json(args, &crate::stream::prompt_json(prompt, image));
    }
    args = crate::locate::with_fork_session(args, fork);
    args = crate::locate::with_worktree(args, worktree);
    let mut cmd = Command::new(&program);
    cmd.args(&args)
        .current_dir(&cwd_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("GROK_NO_AUTO_UPDATE", "1");
    if always_approve {
        cmd.env("GROK_DEFAULT_SELECTED_PERMISSION", "always_allow_all_sessions");
    } else if auto {
        cmd.env("GROK_DEFAULT_SELECTED_PERMISSION", "allow_command_always");
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        unsafe {
            cmd.pre_exec(|| {
                isolate_spawned_grok();
                Ok(())
            });
        }
    }
    if !skip_cabin_home {
        if let Some(dir) = prepare_cabin_grok_home() {
            cmd.env("GROK_HOME", &dir);
        }
    }
    if let Some(sock) = cabin_leader_socket() {
        cmd.env("GROK_LEADER_SOCKET", &sock);
    }
    cmd.spawn().map_err(|e| format!("spawn grok -p: {e}"))
}

fn grok_p_once(
    prompt: &str,
    cwd: &Path,
    resume: Option<&str>,
    always_approve: bool,
    auto: bool,
    model: Option<&str>,
    effort: Option<&str>,
    plan: bool,
    image: Option<&str>,
    fork: bool,
) -> Result<SingleTurn, String> {
    let mut child = grok_p_child(
        prompt,
        cwd,
        resume,
        always_approve,
        auto,
        model,
        effort,
        plan,
        image,
        fork,
        false,
        false,
    )?;
    let pid = child.id();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });
    let out = match rx.recv_timeout(Duration::from_secs(600)) {
        Ok(Ok(o)) => o,
        Ok(Err(e)) => return Err(e.to_string()),
        Err(_) => {
            crate::stream::kill_pid(pid);
            return Err("grok -p timed out".into());
        }
    };
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
    if let Ok(t) = crate::stream::fold_stream(&stdout) {
        return Ok(t);
    }
    if let Ok(t) = parse_single_turn(&stdout) {
        return Ok(t);
    }
    let extra = if stderr.is_empty() {
        format_exit_status(out.status)
    } else {
        stderr
    };
    Err(format!("grok -p failed ({extra})"))
}

/// Alpha `grok -p --resume` looks in GROK_HOME. A cabin-isolated home misses
/// sessions that live in `~/.grok`, then 404s on the remote restore.
pub fn session_resume_is_missing(err: &str) -> bool {
    let l = err.to_ascii_lowercase();
    (l.contains("not found locally") && l.contains("404"))
        || l.contains("session get failed: 404")
        || (l.contains("not found locally") && l.contains("restoring conversation"))
}

pub fn session_id_in_home(home: &Path, id: &str) -> bool {
    let id = id.trim();
    if id.is_empty() || !looks_like_session_id(id) {
        return false;
    }
    dir_has_named_session(&home.join("sessions"), id, 0)
}

pub fn cabin_has_session(id: &str) -> bool {
    let Some(home) = crate::locate::cabin_grok_home() else {
        return false;
    };
    session_id_in_home(&home, id)
}

fn dir_has_named_session(dir: &Path, id: &str, depth: u8) -> bool {
    if depth > 6 {
        return false;
    }
    if dir.join(id).is_dir() {
        return true;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    for e in entries.flatten() {
        let path = e.path();
        if path.is_dir() && dir_has_named_session(&path, id, depth + 1) {
            return true;
        }
    }
    false
}

fn looks_like_session_id(id: &str) -> bool {
    if id.len() < 8 {
        return false;
    }
    let dashes = id.bytes().filter(|b| *b == b'-').count();
    if dashes >= 2 && id.len() >= 16 && id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-') {
        return true;
    }
    if id.starts_with("sess_") {
        return id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
    }
    false
}

fn session_summary_after_meta(rest: &str) -> String {
    let mut toks: Vec<&str> = rest.split_whitespace().collect();
    while toks.first().is_some_and(|t| {
        let dateish = t.len() >= 8 && t.bytes().all(|b| b.is_ascii_digit() || b == b'-') && t.contains('-');
        dateish || matches!(*t, "local" | "remote" | "cloud")
    }) {
        toks.remove(0);
    }
    let s = toks.join(" ");
    if is_placeholder_session_title(&s) {
        String::new()
    } else {
        s
    }
}

pub fn is_placeholder_session_title(s: &str) -> bool {
    let t = s.trim();
    t.is_empty()
        || t.eq_ignore_ascii_case("(no summary)")
        || t.eq_ignore_ascii_case("(no label)")
        || t.eq_ignore_ascii_case("session")
}

/// Cabin History label: Grok Build session name unless the user renamed the tab.
pub fn preferred_history_title(
    cabin_title: &str,
    title_locked: bool,
    grok_title: Option<&str>,
    grok_id: Option<&str>,
) -> String {
    if !title_locked {
        if let Some(title) = grok_title.map(str::trim).filter(|t| !t.is_empty()) {
            let id = grok_id.unwrap_or("").trim();
            if !is_placeholder_session_title(title) && title != id {
                return title.to_string();
            }
        }
    }
    cabin_title.to_string()
}

/// First real `<user_query>` line from a Grok Build `chat_history.jsonl`.
pub fn session_title_from_chat_history(text: &str) -> Option<String> {
    if let Some(t) = title_from_jsonl_records(text) {
        return Some(t);
    }
    title_from_user_query_blocks(text)
}

fn title_from_jsonl_records(text: &str) -> Option<String> {
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if v.get("type").and_then(|t| t.as_str()) != Some("user") {
            continue;
        }
        for blob in json_user_blobs(&v) {
            if let Some(t) = title_from_user_query_blocks(&blob) {
                return Some(t);
            }
        }
    }
    None
}

fn json_user_blobs(v: &Value) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(c) = v.get("content") {
        collect_json_text(c, &mut out);
    }
    out
}

fn collect_json_text(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::String(s) => out.push(s.clone()),
        Value::Array(a) => {
            for x in a {
                collect_json_text(x, out);
            }
        }
        Value::Object(m) => {
            if let Some(Value::String(s)) = m.get("text") {
                out.push(s.clone());
            } else if let Some(c) = m.get("content") {
                collect_json_text(c, out);
            }
        }
        _ => {}
    }
}

fn title_from_user_query_blocks(text: &str) -> Option<String> {
    for chunk in text.split("<user_query>").skip(1) {
        if !chunk.contains("</user_query>") {
            continue;
        }
        let Some(inner) = chunk.split("</user_query>").next() else {
            continue;
        };
        if inner.contains("<work_policy>")
            || inner.contains("<user_info>")
            || inner.contains("<system-reminder>")
        {
            continue;
        }
        let normalized = inner.replace("\\n", "\n");
        let Some(line) = normalized.lines().map(str::trim).find(|l| !l.is_empty()) else {
            continue;
        };
        if line.eq_ignore_ascii_case("tag.") {
            continue;
        }
        return Some(line.chars().take(80).collect());
    }
    None
}

/// Parse `grok sessions list` table / JSON / empty placeholder.
pub fn parse_session_list(text: &str) -> Vec<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() || trimmed.to_ascii_lowercase().starts_with("no sessions found") {
        return Vec::new();
    }
    if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
        if let Some(rows) = session_rows_from_json(&v) {
            return rows;
        }
    }
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('(') {
            continue;
        }
        if line.to_ascii_uppercase().starts_with("SESSION ID") {
            continue;
        }
        let Some(id) = line.split_whitespace().next() else {
            continue;
        };
        if !looks_like_session_id(id) {
            continue;
        }
        let rest = line[id.len()..].trim();
        let summary = session_summary_after_meta(rest);
        if summary.is_empty() {
            out.push(id.to_string());
        } else {
            out.push(format!("{id}  {summary}"));
        }
    }
    out
}

fn session_rows_from_json(v: &Value) -> Option<Vec<String>> {
    let arr = v
        .as_array()
        .or_else(|| v.get("sessions").and_then(|x| x.as_array()))
        .or_else(|| v.get("data").and_then(|x| x.as_array()))?;
    let mut out = Vec::new();
    for x in arr {
        if let Some(s) = x.as_str() {
            if !s.is_empty() {
                out.push(s.to_string());
            }
            continue;
        }
        let Some(id) = x
            .get("id")
            .or_else(|| x.get("sessionId"))
            .or_else(|| x.get("session_id"))
            .and_then(|s| s.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
        else {
            continue;
        };
        let title = x
            .get("title")
            .or_else(|| x.get("summary"))
            .or_else(|| x.get("name"))
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .trim();
        if title.is_empty() {
            out.push(id.to_string());
        } else {
            out.push(format!("{id}  {title}"));
        }
    }
    Some(out)
}

fn sessions_list_text(bin: &Path, cwd: &Path, isolate: bool) -> String {
    let args = ["sessions", "list", "-n", "50"];
    if isolate {
        grok_stdout_timeout(bin, cwd, &args, 8).unwrap_or_default()
    } else {
        grokhub_acp_user_stdout(bin, cwd, &args, 8).unwrap_or_default()
    }
}

fn grokhub_acp_user_stdout(
    bin: &Path,
    cwd: &Path,
    args: &[&str],
    secs: u64,
) -> Result<String, String> {
    crate::locate::grok_user_stdout_timeout(bin, cwd, args, secs)
}

fn grok_cmd_text(bin: &Path, cwd: &Path, args: &[&str], secs: u64) -> Result<String, String> {
    let cabin = grok_stdout_timeout(bin, cwd, args, secs);
    if let Ok(t) = cabin {
        if !t.trim().is_empty() && !t.to_ascii_lowercase().contains("unrecognized subcommand") {
            return Ok(t);
        }
    }
    grokhub_acp_user_stdout(bin, cwd, args, secs)
}

/// List Grok Build CLI sessions (`grok sessions list`). User `~/.grok` only —
/// not cabin GROK_HOME and not a disk walk of subagent dirs.
pub fn list_sessions(bin: &Path, cwd: &Path) -> Result<Vec<String>, String> {
    Ok(parse_session_list(&sessions_list_text(bin, cwd, false)))
}

/// Permanently drop a Grok Build session (`grok sessions delete <id>`).
/// Always delete in the user CLI home so History stays 1:1 with `grok sessions`.
pub fn delete_session(bin: &Path, cwd: &Path, id: &str) -> Result<(), String> {
    let id = id.trim();
    if id.is_empty() {
        return Err("empty session id".into());
    }
    let args = ["sessions", "delete", id];
    let user = grokhub_acp_user_stdout(bin, cwd, &args, 20);
    // Cabin GROK_HOME is not History. Do not let that extra delete block the
    // ~/.grok refresh — a miss there used to wait out the timeout and the
    // session row came back from `grok sessions list`.
    let bin = bin.to_path_buf();
    let cwd = cwd.to_path_buf();
    let extra = id.to_string();
    thread::spawn(move || {
        let _ = grok_stdout_timeout(&bin, &cwd, &["sessions", "delete", &extra], 12);
    });
    user.map(|_| ())
}

/// Persisted per-turn tokens and cost (`grok usage <id>`, 1.0.14+).
pub fn session_usage(bin: &Path, cwd: &Path, id: &str) -> Result<String, String> {
    let id = id.trim();
    if id.is_empty() {
        return Err("empty session id".into());
    }
    grokhub_acp_user_stdout(bin, cwd, &["usage", id], 12)
}

/// Dump one Grok session transcript. Alpha uses `grok export`; older builds used `sessions show`.
pub fn show_session(bin: &Path, cwd: &Path, id: &str) -> Result<String, String> {
    let id = id.trim();
    if id.is_empty() {
        return Err("empty session id".into());
    }
    grok_cmd_text(bin, cwd, &["export", id], 20)
        .or_else(|_| grok_cmd_text(bin, cwd, &["sessions", "show", id], 12))
        .or_else(|_| grok_cmd_text(bin, cwd, &["session", "show", id], 12))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrokSession {
    pub id: String,
    pub title: String,
    pub path: Option<PathBuf>,
    /// Worktree `grok sessions list` used when this row came from the CLI.
    pub cwd: Option<PathBuf>,
    /// True when the session files live under cabin GROK_HOME (safe to `--resume`).
    pub cabin: bool,
}

pub fn split_session_row(row: &str) -> GrokSession {
    let row = row.trim();
    let (id, rest) = row
        .split_once(char::is_whitespace)
        .map(|(a, b)| (a.to_string(), b.trim().to_string()))
        .unwrap_or_else(|| (row.to_string(), String::new()));
    GrokSession {
        title: if rest.is_empty() {
            id.clone()
        } else {
            rest
        },
        id,
        path: None,
        cwd: None,
        cabin: false,
    }
}

/// Transcript turns from a Grok session markdown dump.
pub fn parse_session_markdown(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut role = String::new();
    let mut body = String::new();
    let flush = |role: &mut String, body: &mut String, out: &mut Vec<(String, String)>| {
        let t = body.trim();
        if !role.is_empty() && !t.is_empty() {
            out.push((role.clone(), t.to_string()));
        }
        role.clear();
        body.clear();
    };
    for line in text.lines() {
        let t = line.trim();
        let lower = t.to_ascii_lowercase();
        let heading = t.trim_start_matches('#').trim();
        let heading_l = heading.to_ascii_lowercase();
        let next = if heading_l == "user" || heading_l == "human" || lower.starts_with("user:") || lower.starts_with("**user**")
        {
            Some("user")
        } else if heading_l == "assistant"
            || heading_l == "grok"
            || lower.starts_with("assistant:")
            || lower.starts_with("grok:")
            || lower.starts_with("**assistant**")
            || lower.starts_with("**grok**")
        {
            Some("assistant")
        } else {
            None
        };
        if let Some(r) = next {
            flush(&mut role, &mut body, &mut out);
            role = r.into();
            if let Some((_, rest)) = t.split_once(':') {
                let rest = rest.trim().trim_matches('*').trim();
                if !rest.is_empty() && !rest.eq_ignore_ascii_case("user") && !rest.eq_ignore_ascii_case("assistant") {
                    body.push_str(rest);
                    body.push('\n');
                }
            }
            continue;
        }
        if t.starts_with('#') {
            continue;
        }
        if !role.is_empty() {
            body.push_str(line);
            body.push('\n');
        }
    }
    flush(&mut role, &mut body, &mut out);
    out
}

fn session_title_from_markdown(text: &str, fallback: &str) -> String {
    for line in text.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("# ") {
            let name = rest.trim();
            if !name.is_empty()
                && !name.eq_ignore_ascii_case("user")
                && !name.eq_ignore_ascii_case("assistant")
                && !name.eq_ignore_ascii_case("session")
            {
                return name.chars().take(80).collect();
            }
        }
    }
    fallback.to_string()
}

/// On-disk Grok Build sessions (`~/.grok/sessions/<id>/`, `~/.grok/memory/*/sessions/*.md`).
pub fn discover_session_files() -> Vec<GrokSession> {
    let Some(home) = grok_home() else {
        return Vec::new();
    };
    discover_session_files_in(&home)
}

/// Read Grok Build 1.0.12 `signals.json` (context window + used tokens).
pub fn load_session_signals(home: &Path, session_id: &str) -> Option<crate::stream::GrokUsage> {
    let id = session_id.trim();
    if id.is_empty() {
        return None;
    }
    find_signals_json(&home.join("sessions"), id, 0)
}

fn find_signals_json(dir: &Path, id: &str, depth: u8) -> Option<crate::stream::GrokUsage> {
    if depth > 4 {
        return None;
    }
    let direct = dir.join(id).join("signals.json");
    if direct.is_file() {
        let raw = std::fs::read_to_string(&direct).ok()?;
        return crate::stream::parse_signals_json(&raw);
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return None;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            if let Some(u) = find_signals_json(&p, id, depth + 1) {
                return Some(u);
            }
        }
    }
    None
}

pub fn discover_session_files_in(home: &Path) -> Vec<GrokSession> {
    let mut out = Vec::new();
    let roots = [
        home.join("memory"),
        home.join("sessions"),
        home.join("worktrees"),
    ];
    for root in roots {
        walk_session_md(&root, 0, &mut out);
    }
    walk_session_dirs(&home.join("sessions"), 0, &mut out);
    out
}

fn walk_session_dirs(dir: &Path, depth: u8, out: &mut Vec<GrokSession>) {
    if depth > 6 || out.len() >= 80 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let path = e.path();
        if !path.is_dir() {
            continue;
        }
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        if looks_like_session_id(name) {
            if out.iter().any(|s| s.id == name) {
                continue;
            }
            let title = session_title_from_dir(&path, name);
            out.push(GrokSession {
                id: name.to_string(),
                title,
                path: Some(path.join("chat_history.jsonl")),
                cwd: None,
                cabin: false,
            });
            continue;
        }
        walk_session_dirs(&path, depth + 1, out);
    }
}

fn session_title_from_dir(dir: &Path, fallback: &str) -> String {
    let raw = read_file_capped(&dir.join("summary.json"), SESSION_MD_CAP);
    if let Ok(v) = serde_json::from_str::<Value>(&raw) {
        if let Some(t) = v
            .get("session_summary")
            .or_else(|| v.get("title"))
            .or_else(|| v.pointer("/info/title"))
            .and_then(|x| x.as_str())
            .map(str::trim)
            .filter(|t| !is_placeholder_session_title(t))
        {
            return t.chars().take(80).collect();
        }
    }
    if let Some(t) = session_title_from_jsonl_path(&dir.join("chat_history.jsonl")) {
        return t;
    }
    fallback.to_string()
}

fn session_title_from_jsonl_path(path: &Path) -> Option<String> {
    let f = std::fs::File::open(path).ok()?;
    let reader = BufReader::new(f);
    let mut seen = 0usize;
    for line in reader.lines() {
        let Ok(line) = line else {
            continue;
        };
        seen = seen.saturating_add(line.len());
        if let Some(t) = session_title_from_chat_history(&line) {
            return Some(t);
        }
        if seen > 256 * 1024 {
            break;
        }
    }
    None
}

fn walk_session_md(dir: &Path, depth: u8, out: &mut Vec<GrokSession>) {
    if depth > 6 || out.len() >= 80 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let path = e.path();
        if path.is_dir() {
            walk_session_md(&path, depth + 1, out);
            continue;
        }
        if path.extension().and_then(|s| s.to_str()) != Some("md") {
            continue;
        }
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        if stem.eq_ignore_ascii_case("readme")
            || stem.eq_ignore_ascii_case("skill")
            || stem.is_empty()
        {
            continue;
        }
        let id = if looks_like_session_id(&stem) {
            stem.clone()
        } else if let Some(parent) = path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|s| s.to_str())
        {
            if looks_like_session_id(parent) {
                parent.to_string()
            } else {
                continue;
            }
        } else {
            continue;
        };
        if out.iter().any(|s| s.id == id && s.path.is_some()) {
            continue;
        }
        let text = read_file_capped(&path, SESSION_MD_CAP);
        out.push(GrokSession {
            id: id.clone(),
            title: session_title_from_markdown(&text, &id),
            path: Some(path),
            cwd: None,
            cabin: false,
        });
    }
}

const SESSION_MD_CAP: usize = 8 * 1024;

fn read_file_capped(path: &Path, cap: usize) -> String {
    let mut f = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return String::new(),
    };
    let mut buf = vec![0u8; cap];
    let n = match f.read(&mut buf) {
        Ok(n) => n,
        Err(_) => return String::new(),
    };
    String::from_utf8_lossy(&buf[..n]).into_owned()
}

pub fn merge_grok_sessions(listed: &[String], files: Vec<GrokSession>) -> Vec<GrokSession> {
    let mut out: Vec<GrokSession> = listed.iter().map(|r| split_session_row(r)).collect();
    for f in files {
        if let Some(hit) = out.iter_mut().find(|s| s.id == f.id) {
            if hit.path.is_none() {
                hit.path = f.path;
            }
            let listed_blank = hit.title == hit.id || is_placeholder_session_title(&hit.title);
            let file_real = f.title != f.id && !is_placeholder_session_title(&f.title);
            if listed_blank && file_real {
                hit.title = f.title;
            }
        } else {
            out.push(f);
        }
    }
    out
}

pub fn inspect_json(bin: &Path, cwd: &Path) -> Result<Value, String> {
    let text = crate::locate::grok_user_stdout_timeout(bin, cwd, &["inspect", "--json"], 20)?;
    serde_json::from_str(text.trim()).map_err(|e| e.to_string())
}

pub fn wait_event(rx: &Receiver<AcpEvent>, timeout: Duration) -> Result<AcpEvent, String> {
    let start = std::time::Instant::now();
    loop {
        match rx.try_recv() {
            Ok(ev) => return Ok(ev),
            Err(TryRecvError::Disconnected) => return Err("acp channel closed".into()),
            Err(TryRecvError::Empty) => {
                if start.elapsed() > timeout {
                    return Err("acp wait timeout".into());
                }
                thread::sleep(Duration::from_millis(10));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawn_opts_missing_grok() {
        let prev = std::env::var_os("GROKHUB_GROK");
        let path_prev = std::env::var_os("PATH");
        let home_prev = std::env::var_os("HOME");
        std::env::set_var("GROKHUB_GROK", "/definitely/missing/grok");
        std::env::set_var("PATH", "/empty-grok-path");
        let fake_home = std::env::temp_dir().join(format!(
            "grokhub-no-grok-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&fake_home).unwrap();
        std::env::set_var("HOME", &fake_home);
        let err = SpawnOpts::grok(
            std::env::temp_dir(),
            None,
            false,
            false,
            SessionMode::Chat,
            None,
        )
        .unwrap_err();
        let _ = std::fs::remove_dir_all(&fake_home);
        if let Some(p) = prev {
            std::env::set_var("GROKHUB_GROK", p);
        } else {
            std::env::remove_var("GROKHUB_GROK");
        }
        if let Some(p) = path_prev {
            std::env::set_var("PATH", p);
        }
        if let Some(p) = home_prev {
            std::env::set_var("HOME", p);
        }
        assert!(err.contains("x.ai/cli"), "{err}");
    }

    #[test]
    fn parse_session_list_table_and_empty() {
        assert!(parse_session_list("No sessions found.\n").is_empty());
        assert!(parse_session_list("").is_empty());
        let table = "\n(no label)\nSESSION ID                            CREATED     UPDATED     STATUS      SUMMARY\n01a01b0f-7e06-74b1-8f22-5236c9d57d45  2026-08-19  2026-08-19  local  Ping Test Requesting Only Pong Reply\n";
        let rows = parse_session_list(table);
        assert_eq!(rows.len(), 1);
        assert!(rows[0].contains("01a01b0f-7e06-74b1-8f22-5236c9d57d45"), "{rows:?}");
        assert!(rows[0].contains("Ping Test"), "{rows:?}");
        let blank = "01a01b0f-7e06-74b1-8f22-5236c9d57d45  2026-08-21  2026-08-21  local  (no summary)\n";
        let rows = parse_session_list(blank);
        assert_eq!(rows, vec!["01a01b0f-7e06-74b1-8f22-5236c9d57d45".to_string()]);
        assert_eq!(
            session_title_from_chat_history("<user_query>\nfix the dock\n</user_query>").as_deref(),
            Some("fix the dock")
        );
        let grok_jsonl = concat!(
            r#"{"type":"user","content":[{"type":"text","text":"the <user_query> tag.\n\n<work_policy>\n- Keep every explicit requirement"}]}"#,
            "\n",
            r#"{"type":"user","content":[{"type":"text","text":"<user_query>\ntest\n</user_query>"}],"prompt_index":0}"#,
            "\n",
        );
        assert_eq!(
            session_title_from_chat_history(grok_jsonl).as_deref(),
            Some("test"),
            "title must skip the system-prompt <user_query> mention"
        );
        assert_eq!(
            preferred_history_title("Chat", false, Some("Night cabin"), Some("abc")),
            "Night cabin"
        );
        assert_eq!(
            preferred_history_title("My name", true, Some("Night cabin"), Some("abc")),
            "My name"
        );
        assert_eq!(
            preferred_history_title("Chat", false, Some("abc"), Some("abc")),
            "Chat"
        );
        assert_eq!(
            preferred_history_title("Chat", false, Some("(no summary)"), Some("abc")),
            "Chat"
        );
        let json = r#"[{"id":"abc-def-ghi-jkl-mnop","title":"Hi"}]"#;
        assert_eq!(
            parse_session_list(json),
            vec!["abc-def-ghi-jkl-mnop  Hi".to_string()]
        );
        let wrapped = r#"{"sessions":[{"sessionId":"01a01b0f-7e06-74b1-8f22-5236c9d57d45","summary":"Night"}]}"#;
        let rows = parse_session_list(wrapped);
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert!(rows[0].contains("01a01b0f-7e06-74b1-8f22-5236c9d57d45"), "{rows:?}");
        assert!(rows[0].contains("Night"), "{rows:?}");
        let alpha = "\n(no label)\nSESSION ID                            CREATED     UPDATED     STATUS      SUMMARY\n01a03f9b-df3e-7b23-a839-58e031e50ef4  2026-08-26  2026-08-26  local  (no summary)\n01a03f93-8752-7422-9919-0b7529a1c3b9  2026-08-26  2026-08-26  local  Reply with exactly: alpha-ok\n";
        let alpha_rows = parse_session_list(alpha);
        assert_eq!(alpha_rows.len(), 2, "{alpha_rows:?}");
        assert!(alpha_rows[0].contains("01a03f9b"), "{alpha_rows:?}");
        assert!(alpha_rows[1].contains("alpha-ok"), "{alpha_rows:?}");
        let list_fn = include_str!("client.rs")
            .split("pub fn list_sessions(")
            .nth(1)
            .and_then(|s| s.split("pub fn show_session(").next())
            .expect("list_sessions");
        assert!(
            !list_fn.contains("--json"),
            "alpha removed sessions list --json: {list_fn}"
        );
        assert!(
            list_fn.contains("sessions_list_text(bin, cwd, false)")
                && !list_fn.contains("sessions_list_text(bin, cwd, true)"),
            "History must list the user CLI home, not isolated cabin GROK_HOME: {list_fn}"
        );
        assert!(
            list_fn.contains("grokhub_acp_user_stdout"),
            "delete must hit grok sessions delete in ~/.grok: {list_fn}"
        );
        let del_fn = include_str!("client.rs")
            .split("pub fn delete_session(")
            .nth(1)
            .and_then(|s| s.split("pub fn show_session(").next())
            .expect("delete_session");
        let user = del_fn.find("grokhub_acp_user_stdout").expect("user delete");
        let spawn = del_fn.find("thread::spawn").expect("cabin delete off the History path");
        let cabin = del_fn.find("grok_stdout_timeout").expect("cabin grok-home delete");
        assert!(
            user < spawn && spawn < cabin,
            "cabin GROK_HOME delete must not block ~/.grok History: {del_fn}"
        );
        let show_fn = include_str!("client.rs")
            .split("pub fn show_session(")
            .nth(1)
            .and_then(|s| s.split("pub fn split_session_row(").next())
            .expect("show_session");
        assert!(
            show_fn.contains(r#"["export", id]"#),
            "alpha uses grok export for session transcripts: {show_fn}"
        );
    }

    #[test]
    fn session_files_use_uuid_parent_not_plan_stem() {
        let root = std::env::temp_dir().join(format!(
            "grokhub-sess-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let id = "01a01b0f-7e06-74b1-8f22-5236c9d57d45";
        let dir = root.join("sessions").join(id);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("plan.md"),
            "# Night cabin\n\n## User\nHi\n\n## Assistant\nHello.\n",
        )
        .unwrap();
        let found = discover_session_files_in(&root);
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].id, id);
        assert_eq!(found[0].title, "Night cabin");
        let jsonl_root = std::env::temp_dir().join(format!(
            "grokhub-jsonl-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let jsonl_id = "01a02504-ff87-7df1-8f3f-098e415ab465";
        let jsonl_dir = jsonl_root.join("sessions").join("%2Fwork").join(jsonl_id);
        std::fs::create_dir_all(&jsonl_dir).unwrap();
        std::fs::write(
            jsonl_dir.join("chat_history.jsonl"),
            concat!(
                r#"{"type":"user","content":[{"type":"text","text":"the <user_query> tag.\n\n<work_policy>\n- Keep"}]}"}"#,
                "\n",
                r#"{"type":"user","content":[{"type":"text","text":"<user_query>\nfix history names\n</user_query>"}]}"#,
                "\n",
            ),
        )
        .unwrap();
        let jsonl_found = discover_session_files_in(&jsonl_root);
        let _ = std::fs::remove_dir_all(&jsonl_root);
        assert_eq!(jsonl_found.len(), 1, "{jsonl_found:?}");
        assert_eq!(jsonl_found[0].id, jsonl_id);
        assert_eq!(jsonl_found[0].title, "fix history names");
        let src = include_str!("client.rs");
        let walk = src
            .split("fn walk_session_md(")
            .nth(1)
            .and_then(|s| s.split("pub fn merge_grok_sessions(").next())
            .expect("walk_session_md");
        assert!(
            walk.contains("read_file_capped") && !walk.contains("read_to_string"),
            "session title scan must not slurp huge markdown: {walk}"
        );
    }

    #[test]
    fn parse_single_turn_stamps_session_and_text() {
        let raw = r#"{
            "text": "pong",
            "stopReason": "end_turn",
            "sessionId": "01a024f8-7606-74a2-8331-57a5177822eb",
            "thought": "say pong"
        }"#;
        let t = parse_single_turn(raw).expect("json");
        assert_eq!(t.session_id, "01a024f8-7606-74a2-8331-57a5177822eb");
        assert_eq!(t.text, "pong");
        assert_eq!(t.thought, "say pong");
        let spent = parse_single_turn(
            r#"{
            "text": "pong",
            "stopReason": "end_turn",
            "sessionId": "01a024f8-7606-74a2-8331-57a5177822eb",
            "usage": {"input_tokens": 18007, "output_tokens": 45, "reasoning_tokens": 40, "total_tokens": 18052},
            "num_turns": 1
        }"#,
        )
        .expect("usage");
        assert_eq!(spent.usage.reasoning_tokens, 40);
        assert_eq!(spent.usage.total_tokens, 18052);
        assert_eq!(spent.stop_reason, "end_turn");
        let noisy = format!("debug line\n{raw}\n");
        assert_eq!(parse_single_turn(&noisy).unwrap().text, "pong");
        assert!(parse_single_turn("{}").is_err());
        let src = include_str!("client.rs");
        assert!(
            src.contains("single_turn_args") && src.contains("GROK_HOME") && src.contains("-p"),
            "chat turns must use grok -p, not long-lived agent stdio: {src}"
        );
    }

    #[test]
    fn session_markdown_turns() {
        let md = "# Night cabin\n\n## User\nLook at the pane.\n\n## Assistant\nOn it.\n";
        let turns = parse_session_markdown(md);
        assert_eq!(turns.len(), 2);
        assert_eq!(turns[0], ("user".into(), "Look at the pane.".into()));
        assert_eq!(turns[1], ("assistant".into(), "On it.".into()));
        let export = "## User\n\nReply with exactly: alpha-ok\n\n## Assistant\n\nalpha-ok\n";
        let exported = parse_session_markdown(export);
        assert_eq!(exported.len(), 2);
        assert_eq!(exported[1].1.trim(), "alpha-ok");
        let row = split_session_row("01a01b0f-7e06-74b1-8f22-5236c9d57d45  Night cabin");
        assert_eq!(row.id, "01a01b0f-7e06-74b1-8f22-5236c9d57d45");
        assert_eq!(row.title, "Night cabin");
        let merged = merge_grok_sessions(
            &["abc  Hello".into()],
            vec![GrokSession {
                id: "abc".into(),
                title: "Hello".into(),
                path: Some(std::path::PathBuf::from("/tmp/x.md")),
                cwd: None,
                cabin: false,
            }],
        );
        assert_eq!(merged.len(), 1);
        assert!(merged[0].path.is_some());
        let mixed = merge_grok_sessions(
            &["aaa  Chat".into(), "bbb  Chat".into()],
            vec![GrokSession {
                id: "bbb".into(),
                title: "Chat".into(),
                path: Some(std::path::PathBuf::from("/tmp/bbb.md")),
                cwd: None,
                cabin: false,
            }],
        );
        assert_eq!(mixed.len(), 2, "same title must not attach the wrong transcript");
        assert_eq!(mixed[0].id, "aaa");
        assert!(mixed[0].path.is_none());
        assert_eq!(mixed[1].id, "bbb");
        assert!(mixed[1].path.is_some());
    }

    #[test]
    fn jsonrpc_error_extracts_message() {
        assert!(is_sigterm_status("agent closed (exit 143)"));
        assert!(is_sigterm_status("signal 15"));
        assert!(!is_sigterm_status("exit 1"));
        let obj = serde_json::json!({
            "code": -32603,
            "message": "ACP session/new failed: Failed to initialize session: Permission denied (os error 13)"
        });
        let text = jsonrpc_error_text(&obj);
        assert!(text.contains("Permission denied"), "{text}");
        assert!(!text.contains("\"code\""), "{text}");
        assert_eq!(
            jsonrpc_error_text(&serde_json::json!("no space left on device")),
            "no space left on device"
        );
    }

    #[test]
    fn handshake_error_names_the_cwd() {
        let cwd = PathBuf::from("/home/j/secret-tree");
        let disk = explain_handshake_error(
            "Failed to initialize session: No space left on device (os error 28)",
            &cwd,
        );
        assert!(disk.contains("disk is full"), "{disk}");
        assert!(disk.contains("/home/j/secret-tree"), "{disk}");
        let perm = explain_handshake_error(
            "ACP session/new failed: Failed to initialize session: Permission denied (os error 13)",
            &cwd,
        );
        assert!(perm.contains("cannot write"), "{perm}");
        assert!(perm.contains("GrokHub-Work"), "{perm}");
        assert!(is_session_cwd_error(&disk));
        assert!(is_session_cwd_error(&perm));
        assert!(!is_session_cwd_error("ACP handshake timed out"));
        assert!(
            !is_session_cwd_error("session not found: no such file"),
            "a dead session file must retry session/new in the bound tree"
        );
        let load = explain_handshake_error("session/load failed: session not found", &cwd);
        assert!(load.contains("session/load"), "{load}");
        assert!(!load.contains("session/new"), "{load}");
        let closed = explain_handshake_error("agent closed", &cwd);
        assert!(
            !closed.contains("session/new"),
            "a live agent exit must not look like session/new failed: {closed}"
        );
        let auth = explain_handshake_error(
            "Internal error: Unauthorized (401) from https://cli-chat-proxy.grok.com/v1/responses: Invalid or expired",
            &cwd,
        );
        assert!(auth.contains("401"), "{auth}");
        assert!(auth.to_ascii_lowercase().contains("grok login"), "{auth}");
        let rpc = jsonrpc_error_text(&serde_json::json!({
            "code": -32603,
            "message": "Internal error",
            "data": "Unauthorized (401) from https://cli-chat-proxy.grok.com/v1/responses: Invalid or expired"
        }));
        assert!(rpc.contains("401"), "{rpc}");
        assert!(rpc.contains("Internal error"), "{rpc}");
    }

    #[test]
    fn missing_resume_is_a_local_404_not_a_cwd_error() {
        let err = "grok -p failed (Session \"01a0400f-2bbc-7501-ba65-578617720d19\" not found locally, restoring conversation from remote...\n  [0.000s] Fetching session record — Loading restore metadata from the registry\nError: Failed to restore session from remote: fetching session record: session get failed: 404 Not Found)";
        assert!(
            session_resume_is_missing(err),
            "cabin GROK_HOME miss plus remote 404 must drop --resume: {err}"
        );
        assert!(!session_resume_is_missing("grok -p timed out"));
        assert!(!is_session_cwd_error(err));
        let root = std::env::temp_dir().join(format!(
            "grokhub-sess-home-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let id = "01a0400f-2bbc-7501-ba65-578617720d19";
        let dir = root
            .join("sessions")
            .join("%2Fhome%2Fviper%2FGrokHub-Work")
            .join(id);
        std::fs::create_dir_all(&dir).unwrap();
        assert!(session_id_in_home(&root, id));
        assert!(!session_id_in_home(&root, "01a0400f-0000-0000-0000-000000000000"));
        let src = include_str!("client.rs");
        let run = src
            .split("pub fn run_single_turn_full(")
            .nth(1)
            .and_then(|s| s.split("pub fn session_resume_is_missing(").next())
            .expect("run_single_turn_full");
        assert!(
            run.contains("session_resume_is_missing") && run.contains("resume: None"),
            "a 404 resume must retry grok -p without --resume: {run}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn stderr_tail_trims_on_char_boundaries() {
        // Every glyph grok uses for spinners and rules is multi-byte, so a byte-offset
        // drain lands mid-character and panics — which stops the drain and hangs the turn.
        for glyph in ["—", "⠋", "│", "の", "😀"] {
            let cap = 8;
            let mut held: String = glyph.repeat(200);
            let before = held.clone();
            trim_stderr_tail(&mut held, cap);
            assert!(held.len() <= cap * 2, "{glyph:?} tail not trimmed: {}", held.len());
            assert!(!held.is_empty(), "{glyph:?} trimmed to nothing");
            assert!(
                before.ends_with(&held),
                "{glyph:?} must keep a suffix of the original, got {held:?}"
            );
            assert!(
                held.chars().all(|c| glyph.starts_with(c)),
                "{glyph:?} trim split a character: {held:?}"
            );
        }

        // A tail under the trim threshold is left exactly as it was.
        let mut small = "warning: grok is fine".to_string();
        let untouched = small.clone();
        trim_stderr_tail(&mut small, STDERR_TAIL_CAP);
        assert_eq!(small, untouched);

        // Mixed ASCII and wide text still ends up valid UTF-8 of a bounded size.
        let mut mixed: String = "abc—def⠋".repeat(500);
        trim_stderr_tail(&mut mixed, 32);
        assert!(mixed.len() <= 64, "{}", mixed.len());
        assert!(std::str::from_utf8(mixed.as_bytes()).is_ok());
    }

    #[test]
    fn drop_waits_off_the_ui_thread() {
        let src = include_str!("client.rs");
        let drop = src
            .split("impl Drop for AcpHandle")
            .nth(1)
            .and_then(|s| s.split("fn looks_like_session_id").next())
            .expect("AcpHandle drop");
        let src = include_str!("client.rs");
        let drain = src
            .split("fn drain_stderr(")
            .nth(1)
            .and_then(|s| s.split("fn with_stderr(").next())
            .expect("drain_stderr");
        assert!(
            drain.contains("Interrupted"),
            "stderr drain must not exit on EINTR or grok dies on SIGPIPE: {drain}"
        );
        assert!(
            drain.contains("trim_stderr_tail"),
            "trimming must go through the char-boundary-safe helper: {drain}"
        );
        let stream = src
            .split("pub fn spawn_grok_p_stream(")
            .nth(1)
            .and_then(|s| s.split("fn grok_p_child(").next())
            .expect("spawn_grok_p_stream");
        assert!(
            stream.contains("child.stderr.take()") && stream.contains("drain_stderr"),
            "grok_p_child pipes stderr, so the live stream must drain it — an undrained \
             pipe fills at 64KB and grok blocks in write, ending the turn's output with \
             no error and no timeout: {stream}"
        );
        let connect = src
            .split("pub fn connect(")
            .nth(1)
            .and_then(|s| s.split("fn write_msg(").next())
            .unwrap_or(src);
        assert!(
            src.contains("ignore_sigpipe") && src.contains("pre_exec"),
            "grok child must ignore SIGPIPE so a closed log pipe cannot kill the turn: {connect}"
        );
        assert!(
            src.contains("isolate_spawned_grok")
                && src.contains("setsid")
                && src.contains("read_dir(\"/proc/self/fd\")"),
            "cabin DRI/Wayland fds must not leak into grok or the leader SIGTERMs it (exit 143): {connect}"
        );
        let live = src
            .split("let stderr_live = stderr_tail.clone();")
            .nth(1)
            .and_then(|s| s.split("impl AcpHandle").next())
            .expect("live reader");
        assert!(
            !live.contains("acp json"),
            "malformed ACP chatter must not abort the grok child: {live}"
        );
        assert!(
            live.contains("Cmd::Reject") && live.contains("Cmd::Ack") && live.contains("_x.ai/"),
            "live reader must ACK _x.ai/models/update, not method-not-found: {live}"
        );
        assert!(
            src.contains("rpc_reply_id"),
            "id:null notifications must not be treated as requests: {src}"
        );
        assert!(
            live.contains("wait_status_text"),
            "stdout EOF must reap grok so the chat names the wait status: {live}"
        );
        assert!(
            connect.contains("Cmd::Reject") && connect.contains("method_not_found"),
            "cmd thread must write method-not-found for unknown client-bound RPC: {connect}"
        );
        let spawn = drop.find("thread::spawn").expect("wait off thread");
        let wait = drop.find(".wait()").expect("child wait");
        assert!(
            spawn < wait && !drop.contains("self.child.wait()"),
            "tab switch must not freeze on grok agent teardown: {drop}"
        );
        let connect = src
            .split("pub fn connect(")
            .nth(1)
            .and_then(|s| s.split("impl AcpHandle").next())
            .expect("connect");
        let timeout_kill = connect.find("handshake timed out").expect("timeout");
        assert!(
            connect[timeout_kill.saturating_sub(200)..timeout_kill].contains("thread::spawn"),
            "handshake timeout must not block the spawn worker on child.wait: {connect}"
        );
    }

    #[test]
    fn session_cwd_probe_creates_and_cleans() {
        let dir = std::env::temp_dir().join(format!(
            "grokhub-cwd-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let got = ensure_session_cwd(&dir).expect("writable temp");
        assert_eq!(got, dir);
        assert!(dir.is_dir());
        assert!(!dir.join(".grokhub-cwd-ok").exists());
        let _ = std::fs::remove_dir_all(&dir);
        let src = include_str!("client.rs");
        let probe = src
            .split("pub fn ensure_session_cwd(")
            .nth(1)
            .and_then(|s| s.split("fn cwd_probe_cache(").next())
            .expect("ensure_session_cwd");
        assert!(
            probe.contains("cwd_probe_cache") && probe.contains("from_secs(5)"),
            "ACP cwd probe must not write .grokhub-cwd-ok on every send: {probe}"
        );
        assert!(
            probe.contains("thread::spawn") && probe.contains("inflight"),
            "stale ACP cwd probe must refresh off the caller: {probe}"
        );
    }

    #[test]
    fn with_resume_does_not_pass_cli_resume() {
        let opts = SpawnOpts {
            program: PathBuf::from("grok"),
            args: agent_args(false, None),
            cwd: PathBuf::from("/tmp"),
            api_key: None,
            xai_api_key: None,
            always_approve: false,
            auto: false,
            session_mode: SessionMode::Chat,
            reasoning_effort: None,
            extra_env: vec![],
            handshake_timeout: None,
            resume: None,
            skip_cabin_home: false,
            worktree: false,
        }
        .with_resume(Some("abc-123".into()));
        assert_eq!(opts.resume.as_deref(), Some("abc-123"));
        assert!(
            !opts.args.iter().any(|a| a == "--resume"),
            "CLI --resume plus session/new mixed sessions: {:?}",
            opts.args
        );
        let src = include_str!("client.rs");
        let handshake = src
            .split("fn read_until_result(")
            .nth(1)
            .and_then(|s| s.split("struct HandshakeOk").next())
            .expect("read_until_result");
        assert!(
            handshake.contains("session/request_permission") && handshake.contains("pending_perm"),
            "session/load must collect replay permission ids, not drop them unanswered: {handshake}"
        );
        let sid_pick = src
            .split("let session_id = created")
            .nth(1)
            .and_then(|s| s.split("Ok(HandshakeOk").next())
            .expect("session_id pick");
        assert!(
            sid_pick.contains(".or(resume_id)"),
            "alpha session/load may omit sessionId; resume id must be the fallback: {sid_pick}"
        );
        assert!(
            handshake.contains("method_not_found") && handshake.contains("write_msg"),
            "handshake must answer unknown client-bound RPC or grok exits: {handshake}"
        );
        let load = src
            .split("if method == \"session/request_permission\"")
            .nth(2)
            .and_then(|s| s.split("if msg.result.is_some()").next())
            .expect("live request_permission");
        assert!(
            load.contains("swallow_load")
                && load.contains("Cmd::Permission")
                && load.contains("allow: false"),
            "load-replay permission must be denied so the agent is not stuck: {load}"
        );
        assert!(
            src.contains("x.ai/mcp/elicit")
                && src.contains("Cmd::Elicit")
                && src.contains("parse_elicit"),
            "1.0.17 MCP elicitation must answer x.ai/mcp/elicit, not method-not-found: {src}"
        );
        let cancel = src
            .split("Cmd::Cancel =>")
            .nth(1)
            .and_then(|s| s.split("Cmd::Prompt").next())
            .expect("Cancel");
        assert!(
            cancel.contains("swallow_load.store(true"),
            "session/cancel must ignore leftover stream until the next prompt: {cancel}"
        );
        let done = src
            .split("if msg.result.is_some() || msg.error.is_some()")
            .nth(1)
            .and_then(|s| s.split("if msg.result.is_some()").next())
            .expect("prompt rpc gate");
        assert!(
            done.contains("prompt_r") && done.contains("rpc != prompt"),
            "session/cancel result must not finish the live turn: {done}"
        );
    }
}
