use crate::protocol::{
    encode_line, initialize_params, parse_permission, parse_session_update, permission_allow,
    permission_deny, pick_auth_method, prompt_params, request, session_new_params, AcpEvent,
    JsonRpc,
};
use crate::protocol::SessionMode;
use crate::{agent_args, find_grok, grok_stdout};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStderr, Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

/// Default cap on `initialize` / `authenticate` / `session/new` so a silent
/// `grok` cannot freeze the cabin UI thread.
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(12);

enum Cmd {
    Prompt(String),
    Cancel,
    Permission { id: Value, allow: bool },
    Shutdown,
}

/// Long-lived `grok agent stdio` session.
pub struct AcpHandle {
    child: Child,
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
    pub always_approve: bool,
    pub auto: bool,
    pub session_mode: SessionMode,
    pub extra_env: Vec<(String, String)>,
    pub handshake_timeout: Option<Duration>,
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
            always_approve,
            auto,
            session_mode,
            extra_env: Vec::new(),
            handshake_timeout: None,
        })
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
    early: &mut Vec<AcpEvent>,
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
                if let Some(ev) = parse_session_update(msg.params.as_ref().unwrap_or(&json!({}))) {
                    early.push(ev);
                }
                continue;
            }
        }
        if msg.id.as_ref().and_then(|v| v.as_u64()) == Some(want) {
            if let Some(err) = msg.error {
                return Err(err.to_string());
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
    early: Vec<AcpEvent>,
}

fn handshake(
    mut stdin: std::process::ChildStdin,
    stdout: std::process::ChildStdout,
    api_key: &str,
    cwd: &str,
    always_approve: bool,
    auto: bool,
    session_mode: SessionMode,
) -> Result<HandshakeOk, String> {
    let mut reader = BufReader::new(stdout);
    let mut next_id = 1u64;
    let mut early = Vec::new();
    write_msg(&mut stdin, &request(next_id, "initialize", initialize_params()))?;
    let init = read_until_result(&mut reader, next_id, &mut early)?;
    next_id += 1;
    let methods = init.get("authMethods").cloned().unwrap_or(json!([]));
    if let Some(method_id) = pick_auth_method(&methods, !api_key.is_empty()) {
        write_msg(
            &mut stdin,
            &request(
                next_id,
                "authenticate",
                json!({ "methodId": method_id, "_meta": { "headless": true } }),
            ),
        )?;
        let _ = read_until_result(&mut reader, next_id, &mut early)?;
        next_id += 1;
    }
    write_msg(
        &mut stdin,
        &request(
            next_id,
            "session/new",
            session_new_params(cwd, always_approve, auto, session_mode),
        ),
    )?;
    let created = read_until_result(&mut reader, next_id, &mut early)?;
    next_id += 1;
    let session_id = created
        .get("sessionId")
        .or_else(|| created.get("session_id"))
        .and_then(|v| v.as_str())
        .ok_or("session/new missing sessionId")?
        .to_string();
    Ok(HandshakeOk {
        stdin,
        reader,
        session_id,
        next_id,
        early,
    })
}

/// Spawn and handshake. Puts `XAI_API_KEY` on the child when provided.
pub fn connect(opts: SpawnOpts) -> Result<AcpHandle, String> {
    let timeout = opts.handshake_timeout.unwrap_or(HANDSHAKE_TIMEOUT);
    let mut cmd = Command::new(&opts.program);
    cmd.args(&opts.args)
        .current_dir(&opts.cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("GROK_NO_AUTO_UPDATE", "1");
    if let Some(key) = &opts.api_key {
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
    let cwd = opts.cwd.display().to_string();
    let always_approve = opts.always_approve;
    let auto = opts.auto;
    let session_mode = opts.session_mode;

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
        ));
    });
    let hs = match rx.recv_timeout(timeout) {
        Ok(Ok(hs)) => hs,
        Ok(Err(e)) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(match &stderr_tail {
                Some(t) => with_stderr(e, t),
                None => e,
            });
        }
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
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
        early,
    } = hs;

    let stdin = Arc::new(Mutex::new(stdin));
    let id_gen = Arc::new(Mutex::new(next_id));
    let (cmd_tx, cmd_rx) = mpsc::channel::<Cmd>();
    let (evt_tx, evt_rx) = mpsc::channel();
    for ev in early {
        let _ = evt_tx.send(ev);
    }
    let _ = evt_tx.send(AcpEvent::Ready {
        session_id: session_id.clone(),
    });

    let sid = session_id.clone();
    let stdin_w = stdin.clone();
    let ids = id_gen.clone();
    thread::spawn(move || {
        for cmd in cmd_rx {
            let mut stdin = match stdin_w.lock() {
                Ok(s) => s,
                Err(_) => return,
            };
            match cmd {
                Cmd::Shutdown => return,
                Cmd::Cancel => {
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
                Cmd::Prompt(text) => {
                    let id = {
                        let mut n = ids.lock().unwrap();
                        let id = *n;
                        *n += 1;
                        id
                    };
                    let _ = write_msg(
                        &mut *stdin,
                        &request(id, "session/prompt", prompt_params(&sid, &text)),
                    );
                }
                Cmd::Permission { id, allow } => {
                    let msg = if allow {
                        permission_allow(id)
                    } else {
                        permission_deny(id)
                    };
                    let _ = write_msg(&mut *stdin, &msg);
                }
            }
        }
    });

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
                            if let Some(ev) =
                                parse_session_update(msg.params.as_ref().unwrap_or(&json!({})))
                            {
                                let _ = evt_tx.send(ev);
                            }
                            continue;
                        }
                        if method == "session/request_permission" {
                            if let Some(id) = msg.id {
                                let _ = evt_tx.send(AcpEvent::Permission(parse_permission(
                                    id,
                                    msg.params.as_ref().unwrap_or(&json!({})),
                                )));
                            }
                            continue;
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
                        let _ = evt_tx.send(AcpEvent::Err(err.to_string()));
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
        child,
        cmd: cmd_tx,
        events: evt_rx,
        session_id,
        cwd: opts.cwd,
    })
}

impl AcpHandle {
    pub fn prompt(&self, text: &str) -> Result<(), String> {
        self.cmd
            .send(Cmd::Prompt(text.to_string()))
            .map_err(|e| e.to_string())
    }

    pub fn cancel(&self) -> Result<(), String> {
        self.cmd.send(Cmd::Cancel).map_err(|e| e.to_string())
    }

    pub fn answer_permission(&self, id: Value, allow: bool) -> Result<(), String> {
        self.cmd
            .send(Cmd::Permission { id, allow })
            .map_err(|e| e.to_string())
    }

    pub fn try_recv(&self) -> Result<AcpEvent, TryRecvError> {
        self.events.try_recv()
    }
}

impl Drop for AcpHandle {
    fn drop(&mut self) {
        let _ = self.cmd.send(Cmd::Shutdown);
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn looks_like_session_id(id: &str) -> bool {
    let dashes = id.bytes().filter(|b| *b == b'-').count();
    dashes >= 2 && id.len() >= 16 && id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-')
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
        if let Some(arr) = v.as_array() {
            return arr
                .iter()
                .filter_map(|x| {
                    x.get("id")
                        .or_else(|| x.get("title"))
                        .and_then(|s| s.as_str())
                        .map(|s| s.to_string())
                })
                .collect();
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

/// List Grok sessions via `grok sessions list` in `cwd` (sessions are per worktree).
pub fn list_sessions(bin: &Path, cwd: &Path) -> Result<Vec<String>, String> {
    let text = grok_stdout(bin, cwd, &["sessions", "list", "-n", "50"])?;
    Ok(parse_session_list(&text))
}

pub fn inspect_json(bin: &Path, cwd: &Path) -> Result<Value, String> {
    let out = Command::new(bin)
        .args(["inspect", "--json"])
        .current_dir(cwd)
        .output()
        .map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&out.stdout);
    serde_json::from_str(text.trim()).map_err(|e| {
        if text.trim().is_empty() {
            String::from_utf8_lossy(&out.stderr).trim().to_string()
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
            SessionMode::Code,
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
        assert_eq!(parse_session_list(json), vec!["abc-def-ghi-jkl-mnop".to_string()]);
    }
}
