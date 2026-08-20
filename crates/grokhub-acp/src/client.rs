use crate::protocol::{
    encode_line, initialize_params, parse_permission, parse_session_update, permission_allow,
    permission_allow_always, permission_deny, pick_auth_method, prompt_params_with_image, request,
    session_load_params, session_new_params, AcpEvent, JsonRpc,
};
use crate::protocol::SessionMode;
use crate::{agent_args, find_grok, grok_home, grok_stdout_timeout};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStderr, Command, Stdio};
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
    Shutdown,
}

/// Long-lived `grok agent stdio` session.
pub struct AcpHandle {
    child: Option<Child>,
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
    pub extra_env: Vec<(String, String)>,
    pub handshake_timeout: Option<Duration>,
    pub resume: Option<String>,
}

impl SpawnOpts {
    pub fn grok(
        cwd: PathBuf,
        api_key: Option<String>,
        always_approve: bool,
        auto: bool,
        session_mode: SessionMode,
    ) -> Result<Self, String> {
        let program = find_grok().ok_or_else(|| {
            "Grok Build CLI missing — install from x.ai/cli or set GROKHUB_GROK".to_string()
        })?;
        Ok(Self {
            args: agent_args(always_approve),
            program,
            cwd,
            api_key,
            xai_api_key: None,
            always_approve,
            auto,
            session_mode,
            extra_env: Vec::new(),
            handshake_timeout: None,
            resume: None,
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
        self.args = agent_args(self.always_approve);
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
                Ok(n) => {
                    if let Ok(mut held) = slot.lock() {
                        let chunk = String::from_utf8_lossy(&buf[..n]);
                        for ch in chunk.chars() {
                            if ch == '\0' || ch == '\u{fffd}' {
                                continue;
                            }
                            held.push(ch);
                        }
                        const CAP: usize = 4096;
                        if held.len() > CAP * 2 {
                            let extra = held.len() - CAP;
                            held.drain(..extra);
                        }
                    }
                }
                Err(_) => break,
            }
        }
    });
    tail
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
    reader: &mut BufReader<impl Read>,
    want: u64,
    pending_perm: &mut Vec<Value>,
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
            if msg.result.is_none() && msg.error.is_none() {
                continue;
            }
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
    write_msg(&mut stdin, &request(next_id, "initialize", initialize_params()))?;
    let init = read_until_result(&mut reader, next_id, &mut pending_perm)?;
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
        let _ = read_until_result(&mut reader, next_id, &mut pending_perm)?;
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
        match read_until_result(&mut reader, next_id, &mut pending_perm) {
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
        let v = read_until_result(&mut reader, next_id, &mut pending_perm)?;
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
    })
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
    if let Some(key) = &opts.xai_api_key {
        if !key.is_empty() {
            cmd.env("XAI_API_KEY", key);
        }
    }
    for (k, v) in &opts.extra_env {
        cmd.env(k, v);
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
            }
        }
    });

    for id in pending_perm {
        let _ = cmd_tx.send(Cmd::Permission {
            id,
            allow: false,
            always: false,
        });
    }

    let cmd_tx_r = cmd_tx.clone();
    let prompt_r = prompt_rpc.clone();
    thread::spawn(move || {
        let mut buf = String::new();
        loop {
            buf.clear();
            match reader.read_line(&mut buf) {
                Ok(0) => {
                    let _ = evt_tx.send(AcpEvent::Err("agent closed".into()));
                    return;
                }
                Ok(_) => {
                    let line = buf.trim();
                    if line.is_empty() {
                        continue;
                    }
                    let msg: JsonRpc = match serde_json::from_str(line) {
                        Ok(m) => m,
                        Err(e) => {
                            let _ = evt_tx.send(AcpEvent::Err(format!("acp json: {e}")));
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
        child: Some(child),
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

    pub fn try_recv(&self) -> Result<AcpEvent, TryRecvError> {
        self.events.try_recv()
    }
}

impl Drop for AcpHandle {
    fn drop(&mut self) {
        let _ = self.cmd.send(Cmd::Shutdown);
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            thread::spawn(move || {
                let _ = child.wait();
            });
        }
    }
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
    toks.join(" ")
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

/// List Grok sessions via `grok sessions list` in `cwd` (sessions are per worktree).
pub fn list_sessions(bin: &Path, cwd: &Path) -> Result<Vec<String>, String> {
    let text = grok_stdout_timeout(bin, cwd, &["sessions", "list", "-n", "50"], 8).unwrap_or_default();
    let rows = parse_session_list(&text);
    if !rows.is_empty() {
        return Ok(rows);
    }
    let json = grok_stdout_timeout(bin, cwd, &["sessions", "list", "--json", "-n", "50"], 8)
        .unwrap_or_default();
    Ok(parse_session_list(&json))
}

/// Dump one Grok session transcript (`grok sessions show <id>`).
pub fn show_session(bin: &Path, cwd: &Path, id: &str) -> Result<String, String> {
    let id = id.trim();
    if id.is_empty() {
        return Err("empty session id".into());
    }
    grok_stdout_timeout(bin, cwd, &["sessions", "show", id], 12).or_else(|_| {
        grok_stdout_timeout(bin, cwd, &["session", "show", id], 12)
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrokSession {
    pub id: String,
    pub title: String,
    pub path: Option<PathBuf>,
    /// Worktree `grok sessions list` used when this row came from the CLI.
    pub cwd: Option<PathBuf>,
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
    out
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
            if hit.title == hit.id && f.title != f.id {
                hit.title = f.title;
            }
        } else {
            out.push(f);
        }
    }
    out
}

pub fn inspect_json(bin: &Path, cwd: &Path) -> Result<Value, String> {
    let text = grok_stdout_timeout(bin, cwd, &["inspect", "--json"], 12)?;
    serde_json::from_str(text.trim()).map_err(|e| {
        if text.trim().is_empty() {
            e.to_string()
        } else {
            e.to_string()
        }
    })
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
        std::env::set_var("GROKHUB_GROK", "/definitely/missing/grok");
        std::env::set_var("PATH", "/empty-grok-path");
        let err = SpawnOpts::grok(
            std::env::temp_dir(),
            None,
            false,
            false,
            SessionMode::Chat,
        )
        .unwrap_err();
        if let Some(p) = prev {
            std::env::set_var("GROKHUB_GROK", p);
        } else {
            std::env::remove_var("GROKHUB_GROK");
        }
        if let Some(p) = path_prev {
            std::env::set_var("PATH", p);
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
    fn session_markdown_turns() {
        let md = "# Night cabin\n\n## User\nLook at the pane.\n\n## Assistant\nOn it.\n";
        let turns = parse_session_markdown(md);
        assert_eq!(turns.len(), 2);
        assert_eq!(turns[0], ("user".into(), "Look at the pane.".into()));
        assert_eq!(turns[1], ("assistant".into(), "On it.".into()));
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
    }

    #[test]
    fn drop_waits_off_the_ui_thread() {
        let src = include_str!("client.rs");
        let drop = src
            .split("impl Drop for AcpHandle")
            .nth(1)
            .and_then(|s| s.split("fn looks_like_session_id").next())
            .expect("AcpHandle drop");
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
            args: agent_args(false),
            cwd: PathBuf::from("/tmp"),
            api_key: None,
            xai_api_key: None,
            always_approve: false,
            auto: false,
            session_mode: SessionMode::Chat,
            extra_env: vec![],
            handshake_timeout: None,
            resume: None,
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
