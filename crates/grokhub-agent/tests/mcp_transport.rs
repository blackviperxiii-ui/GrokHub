//! Stdio and loopback HTTP MCP transports. No network beyond 127.0.0.1.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use grokhub_agent::mcp::{self, ElicitNote};
use grokhub_agent::perm::ConfigGuard;
use grokhub_agent::{
    run_loop, CancelToken, ClientError, ClosedPermits, FunctionCall, Gate, HaltCheck, InputItem,
    LoopEvent, LoopIn, ModelClient, PermMode, SteerQueue, StreamEvent, TurnOutput, Usage,
};
use serde_json::{json, Value};

struct Clean(PathBuf);

impl Drop for Clean {
    fn drop(&mut self) {
        mcp::shutdown_all();
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn scratch(label: &str) -> (PathBuf, Clean, ConfigGuard) {
    let dir = std::env::temp_dir().join(format!(
        "gh-mcp-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let clean = Clean(dir.clone());
    let guard = ConfigGuard::set(&dir);
    (dir, clean, guard)
}

fn import_bin(bin: &str, env: &[(&str, &str)]) {
    let mut env_map = serde_json::Map::new();
    for (key, value) in env {
        env_map.insert((*key).to_string(), json!(value));
    }
    let doc = json!({
        "mcpServers": {
            "box": {
                "command": bin,
                "env": env_map,
                "startup_timeout_sec": 8,
                "tool_timeout_sec": 8
            }
        }
    });
    mcp::import_documents(&[doc.to_string()]).unwrap();
}

struct NoHalt;

impl HaltCheck for NoHalt {
    fn halted(&self) -> bool {
        false
    }
}

struct Once {
    name: String,
    turn: AtomicUsize,
}

impl ModelClient for Once {
    fn stream(
        &self,
        _req: &grokhub_agent::ResponsesRequest,
        _cancel: &CancelToken,
        sink: &mut dyn FnMut(StreamEvent),
    ) -> Result<TurnOutput, ClientError> {
        let n = self.turn.fetch_add(1, Ordering::SeqCst);
        if n == 0 {
            return Ok(TurnOutput {
                text: String::new(),
                reasoning: String::new(),
                calls: vec![FunctionCall {
                    call_id: "c1".into(),
                    name: self.name.clone(),
                    arguments: "{}".into(),
                }],
                usage: Usage::default(),
            });
        }
        sink(StreamEvent::TextDelta("done".into()));
        Ok(TurnOutput {
            text: "done".into(),
            reasoning: String::new(),
            calls: Vec::new(),
            usage: Usage::default(),
        })
    }
}

fn run_call(dir: &Path, attended: bool, on_event: &mut dyn FnMut(LoopEvent)) -> String {
    let client = Once {
        name: "box__echo".into(),
        turn: AtomicUsize::new(0),
    };
    let cancel = CancelToken::new();
    let steer = SteerQueue::new();
    let halt = NoHalt;
    let permits = ClosedPermits;
    let input = LoopIn {
        client: &client,
        workspace: dir,
        model: "grok-4.7",
        effort: None,
        system: "",
        conversation_id: "s",
        max_turns: 3,
        usage_base: Usage::default(),
        cancel: &cancel,
        steer: &steer,
        halt: &halt,
        gate: Gate {
            mode: PermMode::Always,
            readonly_session: false,
            attended,
            desktop: false,
        },
        desktop: None,
        permits: &permits,
        perms: None,
        context_length: 0,
        tasks: None,
        depth: 0,
        agent_id: None,
        shared_client: None,
        shared_permits: None,
        shared_desktop: None,
    };
    let mut history = Vec::new();
    let _ = run_loop(&input, &mut history, "go", None, on_event);
    history
        .iter()
        .find_map(|item| match item {
            InputItem::FunctionCallOutput { output, .. } => Some(output.clone()),
            _ => None,
        })
        .unwrap_or_default()
}

#[test]
fn stdio_server_lists_calls_paginates_and_reports_a_crash() {
    let bin = env!("CARGO_BIN_EXE_fake_mcp");
    let (dir, _clean, _guard) = scratch("stdio");
    import_bin(bin, &[]);
    let rows = mcp::doctor();
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].status, "connected", "{rows:?}");
    assert_eq!(rows[0].tool_count, 1, "{rows:?}");
    let output = run_call(&dir, true, &mut |_| {});
    assert!(output.contains("pid="), "{output}");

    mcp::shutdown_all();
    let (dir, _clean, _guard) = scratch("page");
    import_bin(bin, &[("FAKE_MCP_PAGE", "1")]);
    let rows = mcp::doctor();
    assert_eq!(rows[0].status, "connected", "{rows:?}");
    assert_eq!(rows[0].tool_count, 2, "{rows:?}");
    let _ = dir;

    mcp::shutdown_all();
    let (_dir, _clean, _guard) = scratch("crash");
    import_bin(bin, &[("FAKE_MCP_CRASH", "1")]);
    let rows = mcp::doctor();
    assert_eq!(rows[0].status, "error", "{rows:?}");
    assert!(!rows[0].last_error.is_empty(), "{rows:?}");
}

#[test]
fn a_saved_key_reaches_a_stdio_server_in_its_token_env() {
    let bin = env!("CARGO_BIN_EXE_fake_mcp");
    let (dir, _clean, _guard) = scratch("token-env");
    grokhub_agent::harness::use_key_store_for(&dir, Arc::new(grokhub_agent::harness::MemoryKeyStore::new()));
    // FAKE_MCP_PAGE=1 makes the fake server list a second tool, so the tool
    // count shows whether the key landed in that variable.
    let entry = json!({"command": bin, "tokenEnv": "FAKE_MCP_PAGE"});
    assert_eq!(mcp::install("box", &entry).unwrap(), mcp::Installed::Added);
    let rows = mcp::doctor();
    assert_eq!((rows[0].status.as_str(), rows[0].tool_count), ("connected", 1), "{rows:?}");

    assert_eq!(mcp::save_key("box", "1").unwrap(), "Saved the key for box");
    let rows = mcp::doctor();
    assert_eq!((rows[0].status.as_str(), rows[0].tool_count), ("connected", 2), "{rows:?}");
    let text = std::fs::read_to_string(dir.join("mcp.json")).unwrap();
    assert!(text.contains(r#""tokenEnv": "FAKE_MCP_PAGE""#) && !text.contains(r#""env""#), "{text}");
}

#[test]
fn stdio_restart_spawns_a_new_process() {
    let bin = env!("CARGO_BIN_EXE_fake_mcp");
    let (dir, _clean, _guard) = scratch("restart");
    import_bin(bin, &[]);
    let first = run_call(&dir, true, &mut |_| {});
    let first_pid = pid_of(&first);
    mcp::restart("box").unwrap();
    let second = run_call(&dir, true, &mut |_| {});
    let second_pid = pid_of(&second);
    assert_ne!(first_pid, second_pid, "{first} vs {second}");
}

#[cfg(unix)]
#[test]
fn stdio_kill_tree_reaps_the_child() {
    let bin = env!("CARGO_BIN_EXE_fake_mcp");
    let (dir, _clean, _guard) = scratch("tree");
    let file = dir.join("child-pid");
    let file_text = file.display().to_string();
    import_bin(
        bin,
        &[("FAKE_MCP_CHILD", "1"), ("FAKE_MCP_CHILD_FILE", &file_text)],
    );
    let rows = mcp::doctor();
    assert_eq!(rows[0].status, "connected", "{rows:?}");
    let pid: u32 = std::fs::read_to_string(&file)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert!(process_alive(pid), "child {pid} was not started");
    mcp::shutdown_all();
    let start = std::time::Instant::now();
    while process_alive(pid) && start.elapsed() < Duration::from_secs(2) {
        std::thread::sleep(Duration::from_millis(30));
    }
    assert!(!process_alive(pid), "child {pid} survived shutdown");
}

#[cfg(unix)]
fn process_alive(pid: u32) -> bool {
    let text = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap_or_default();
    if text.is_empty() {
        return false;
    }
    let state = text
        .rsplit(')')
        .next()
        .unwrap_or("")
        .split_whitespace()
        .next()
        .unwrap_or("");
    !state.is_empty() && state != "Z"
}

#[test]
fn stdio_elicitation_reaches_the_card_and_declines_unattended() {
    let bin = env!("CARGO_BIN_EXE_fake_mcp");
    let (dir, _clean, _guard) = scratch("elicit");
    import_bin(bin, &[("FAKE_MCP_ELICIT", "1")]);
    let (tx, inbox) = mcp::ElicitInbox::pair();
    mcp::attach_elicit("s", inbox);
    let seen = Mutex::new(Vec::new());
    let output = run_call(&dir, true, &mut |ev| {
        if let LoopEvent::Elicit(view) = ev {
            assert_eq!(view.message, "Need a value");
            assert_eq!(view.server_name, "box");
            let _ = tx.send(ElicitNote {
                id: view.id.clone(),
                action: "accept".into(),
                content: Some(json!({"note": "hi"})),
            });
            seen.lock().unwrap().push(view.message);
        }
    });
    assert_eq!(seen.lock().unwrap().len(), 1, "{output}");
    assert!(output.contains("action=accept"), "{output}");
    assert!(output.contains("hi"), "{output}");
    mcp::detach_elicit("s");
    mcp::shutdown_all();

    let (dir, _clean, _guard) = scratch("elicit-away");
    import_bin(bin, &[("FAKE_MCP_ELICIT", "1")]);
    let mut cards = 0;
    let output = run_call(&dir, false, &mut |ev| {
        if matches!(ev, LoopEvent::Elicit(_)) {
            cards += 1;
        }
    });
    assert_eq!(cards, 0, "{output}");
    assert!(output.contains("action=decline"), "{output}");
}

fn pid_of(text: &str) -> String {
    text.split("pid=")
        .nth(1)
        .unwrap_or("")
        .split_whitespace()
        .next()
        .unwrap_or("")
        .trim_end_matches(|ch: char| !ch.is_ascii_digit())
        .to_string()
}

#[test]
fn http_streamable_and_sse_on_loopback() {
    let (dir, _clean, _guard) = scratch("http");
    let stream = StreamServer::start(true);
    mcp::import_documents(&[json!({
        "mcpServers": {
            "box": {
                "url": stream.url.clone(),
                "type": "http",
                "headers": {"Authorization": "Bearer from-entry"},
                "startup_timeout_sec": 8,
                "tool_timeout_sec": 8
            }
        }
    })
    .to_string()])
    .unwrap();
    let rows = mcp::doctor();
    assert_eq!(rows[0].status, "connected", "{rows:?}");
    assert_eq!(rows[0].tool_count, 1, "{rows:?}");
    assert!(
        stream.saw_bearer.load(Ordering::SeqCst),
        "bearer header was not sent"
    );
    let output = run_call(&dir, false, &mut |_| {});
    assert!(output.contains("action=decline"), "{output}");
    drop(stream);
    mcp::shutdown_all();

    let (dir, _clean, _guard) = scratch("sse");
    let sse = SseServer::start();
    mcp::import_documents(&[json!({
        "mcpServers": {
            "box": {
                "url": sse.url.clone(),
                "type": "sse",
                "startup_timeout_sec": 8,
                "tool_timeout_sec": 8
            }
        }
    })
    .to_string()])
    .unwrap();
    let rows = mcp::doctor();
    assert_eq!(rows[0].status, "connected", "{rows:?}");
    assert_eq!(rows[0].tool_count, 1, "{rows:?}");
    let output = run_call(&dir, true, &mut |_| {});
    assert!(output.contains("sse-ok"), "{output}");
    drop(sse);
}

struct StreamServer {
    url: String,
    stop: Arc<AtomicBool>,
    saw_bearer: Arc<AtomicBool>,
    join: Option<std::thread::JoinHandle<()>>,
}

impl StreamServer {
    fn start(elicit: bool) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let stop = Arc::new(AtomicBool::new(false));
        let saw_bearer = Arc::new(AtomicBool::new(false));
        let pending: Arc<Mutex<Option<Sender<Value>>>> = Arc::new(Mutex::new(None));
        let stop2 = stop.clone();
        let bearer2 = saw_bearer.clone();
        let join = std::thread::spawn(move || {
            listener.set_nonblocking(true).unwrap();
            while !stop2.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((sock, _)) => {
                        let bearer = bearer2.clone();
                        let pending = pending.clone();
                        std::thread::spawn(move || handle_stream(sock, bearer, pending, elicit));
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(15));
                    }
                    Err(_) => break,
                }
            }
        });
        Self {
            url: format!("http://127.0.0.1:{port}/mcp"),
            stop,
            saw_bearer,
            join: Some(join),
        }
    }
}

impl Drop for StreamServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.join.take() {
            let _ = handle.join();
        }
    }
}

fn handle_stream(
    mut sock: TcpStream,
    saw_bearer: Arc<AtomicBool>,
    pending: Arc<Mutex<Option<Sender<Value>>>>,
    elicit: bool,
) {
    let _ = sock.set_read_timeout(Some(Duration::from_secs(3)));
    let Some(req) = read_http(&mut sock) else {
        return;
    };
    if header(&req.headers, "authorization") == Some("Bearer from-entry") {
        saw_bearer.store(true, Ordering::SeqCst);
    } else if req.body.contains("initialize") {
        write_status(&mut sock, 401, "", "missing bearer");
        return;
    }
    let Ok(msg) = serde_json::from_str::<Value>(&req.body) else {
        write_status(&mut sock, 400, "", "bad json");
        return;
    };
    if msg.get("method").is_none() {
        if let Some(tx) = pending.lock().unwrap().take() {
            let _ = tx.send(msg);
        }
        write_status(&mut sock, 202, "", "");
        return;
    }
    let method = msg["method"].as_str().unwrap_or("");
    let id = msg.get("id").cloned().unwrap_or(Value::Null);
    if method != "initialize" && header(&req.headers, "mcp-session-id") != Some("sess-1") {
        write_status(&mut sock, 400, "", "missing session");
        return;
    }
    if method == "tools/call" && elicit {
        let (tx, rx) = mpsc::channel();
        *pending.lock().unwrap() = Some(tx);
        let _ = sock.write_all(
            b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n",
        );
        let elicit_msg = json!({
            "jsonrpc": "2.0",
            "id": "elicit-1",
            "method": "elicitation/create",
            "params": {"message": "Need a value", "mode": "form"}
        });
        let _ = write!(sock, "event: message\r\ndata: {elicit_msg}\r\n\r\n");
        let _ = sock.flush();
        let answer = rx.recv_timeout(Duration::from_secs(5)).unwrap_or(json!({}));
        let action = answer
            .pointer("/result/action")
            .and_then(|v| v.as_str())
            .unwrap_or("decline");
        let result = json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {"content": [{"type": "text", "text": format!("action={action}")}]}
        });
        let _ = write!(sock, "event: message\r\ndata: {result}\r\n\r\n");
        let _ = sock.flush();
        return;
    }
    let result = match method {
        "initialize" => {
            json!({"protocolVersion": "2025-03-26", "capabilities": {}, "serverInfo": {"name": "http", "version": "0"}})
        }
        "tools/list" => {
            json!({"tools": [{"name": "echo", "description": "Echo", "inputSchema": {"type": "object", "properties": {}}}] })
        }
        "ping" => json!({}),
        "tools/call" => json!({"content": [{"type": "text", "text": "http-ok"}]}),
        _ => {
            write_status(
                &mut sock,
                200,
                "",
                &json!({"jsonrpc":"2.0","id": id, "error": {"code": -32601, "message": "nope"}})
                    .to_string(),
            );
            return;
        }
    };
    let body = json!({"jsonrpc": "2.0", "id": id, "result": result}).to_string();
    let extra = if method == "initialize" {
        "Mcp-Session-Id: sess-1\r\n"
    } else {
        ""
    };
    write_status(&mut sock, 200, extra, &body);
}

struct SseServer {
    url: String,
    stop: Arc<AtomicBool>,
    join: Option<std::thread::JoinHandle<()>>,
}

impl SseServer {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let stop = Arc::new(AtomicBool::new(false));
        let events: Arc<Mutex<Option<Sender<String>>>> = Arc::new(Mutex::new(None));
        let stop2 = stop.clone();
        let join = std::thread::spawn(move || {
            listener.set_nonblocking(true).unwrap();
            while !stop2.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((sock, _)) => {
                        let events = events.clone();
                        let stop3 = stop2.clone();
                        std::thread::spawn(move || handle_sse(sock, port, events, stop3));
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(15));
                    }
                    Err(_) => break,
                }
            }
        });
        Self {
            url: format!("http://127.0.0.1:{port}/sse"),
            stop,
            join: Some(join),
        }
    }
}

impl Drop for SseServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.join.take() {
            let _ = handle.join();
        }
    }
}

fn handle_sse(
    mut sock: TcpStream,
    port: u16,
    events: Arc<Mutex<Option<Sender<String>>>>,
    stop: Arc<AtomicBool>,
) {
    let _ = sock.set_read_timeout(Some(Duration::from_millis(400)));
    let Some(req) = read_http(&mut sock) else {
        return;
    };
    if req.method == "GET" {
        let (tx, rx) = mpsc::channel::<String>();
        *events.lock().unwrap() = Some(tx);
        let _ = sock.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n");
        let endpoint = format!("http://127.0.0.1:{port}/message");
        let _ = write!(sock, "event: endpoint\r\ndata: {endpoint}\r\n\r\n");
        let _ = sock.flush();
        while !stop.load(Ordering::SeqCst) {
            match rx.recv_timeout(Duration::from_millis(200)) {
                Ok(data) => {
                    let _ = write!(sock, "event: message\r\ndata: {data}\r\n\r\n");
                    let _ = sock.flush();
                }
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        return;
    }
    let Ok(msg) = serde_json::from_str::<Value>(&req.body) else {
        write_status(&mut sock, 400, "", "bad json");
        return;
    };
    let method = msg.get("method").and_then(|v| v.as_str()).unwrap_or("");
    if method.is_empty() || method.starts_with("notifications/") {
        write_status(&mut sock, 202, "", "");
        return;
    }
    let id = msg.get("id").cloned().unwrap_or(Value::Null);
    let result = match method {
        "initialize" => {
            json!({"protocolVersion": "2025-03-26", "capabilities": {}, "serverInfo": {"name": "sse", "version": "0"}})
        }
        "tools/list" => {
            json!({"tools": [{"name": "echo", "description": "Echo", "inputSchema": {"type": "object", "properties": {}}}] })
        }
        "ping" => json!({}),
        "tools/call" => json!({"content": [{"type": "text", "text": "sse-ok"}]}),
        _ => json!({}),
    };
    let frame = json!({"jsonrpc": "2.0", "id": id, "result": result}).to_string();
    if let Some(tx) = events.lock().unwrap().as_ref() {
        let _ = tx.send(frame);
    }
    write_status(&mut sock, 202, "", "");
}

struct HttpReq {
    method: String,
    headers: Vec<(String, String)>,
    body: String,
}

fn read_http(sock: &mut TcpStream) -> Option<HttpReq> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    let header_end = loop {
        match sock.read(&mut tmp) {
            Ok(0) => return None,
            Ok(n) => {
                buf.extend_from_slice(&tmp[..n]);
                if let Some(pos) = find_sep(&buf) {
                    break pos;
                }
                if buf.len() > 1024 * 1024 {
                    return None;
                }
            }
            Err(err)
                if err.kind() == std::io::ErrorKind::WouldBlock
                    || err.kind() == std::io::ErrorKind::TimedOut =>
            {
                if buf.is_empty() {
                    return None;
                }
                continue;
            }
            Err(_) => return None,
        }
    };
    let head = String::from_utf8_lossy(&buf[..header_end]).to_string();
    let mut lines = head.split("\r\n");
    let request = lines.next().unwrap_or("");
    let method = request.split_whitespace().next().unwrap_or("").to_string();
    let mut headers = Vec::new();
    for line in lines {
        if let Some((key, value)) = line.split_once(':') {
            headers.push((key.trim().to_string(), value.trim().to_string()));
        }
    }
    let len = header(&headers, "content-length")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(0);
    let mut body = buf[header_end + 4..].to_vec();
    while body.len() < len {
        match sock.read(&mut tmp) {
            Ok(0) | Err(_) => break,
            Ok(n) => body.extend_from_slice(&tmp[..n]),
        }
    }
    body.truncate(len);
    Some(HttpReq {
        method,
        headers,
        body: String::from_utf8_lossy(&body).to_string(),
    })
}

fn find_sep(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|window| window == b"\r\n\r\n")
}

fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

fn write_status(sock: &mut TcpStream, code: u16, extra: &str, body: &str) {
    let text = if code == 200 {
        "OK"
    } else if code == 202 {
        "Accepted"
    } else if code == 401 {
        "Unauthorized"
    } else {
        "Bad Request"
    };
    let _ = write!(
        sock,
        "HTTP/1.1 {code} {text}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n{extra}\r\n{body}",
        body.len()
    );
    let _ = sock.flush();
}

struct NoCabinCua;

impl Drop for NoCabinCua {
    fn drop(&mut self) {
        mcp::set_cabin_cua(None);
    }
}

#[test]
fn the_cabin_cua_gate_is_a_native_server_only_while_the_cabin_sets_it() {
    let bin = env!("CARGO_BIN_EXE_fake_mcp");
    let (dir, _clean, _guard) = scratch("cabin-cua");
    let _reset = NoCabinCua;
    // A user entry under the cabin's name is never honored.
    let doc = json!({"mcpServers": {"grokhub-cua": {"command": "/bin/false"}}});
    std::fs::write(dir.join("mcp.json"), doc.to_string()).unwrap();
    assert!(mcp::doctor().iter().all(|row| row.name != "grokhub-cua"));

    mcp::set_cabin_cua(Some(PathBuf::from(bin)));
    let rows = mcp::doctor();
    let row = rows.iter().find(|row| row.name == "grokhub-cua").expect("cabin Cua server");
    assert_eq!((row.status.as_str(), row.tool_count), ("connected", 1), "{rows:?}");
    assert_eq!(row.detail, format!("{bin} --mcp-cua"), "{rows:?}");
    let text = std::fs::read_to_string(dir.join("mcp.json")).unwrap();
    assert!(!text.contains("--mcp-cua"), "never written to mcp.json: {text}");

    mcp::set_cabin_cua(None);
    assert!(mcp::doctor().iter().all(|row| row.name != "grokhub-cua"));
}
