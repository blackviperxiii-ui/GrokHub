//! llama.cpp `llama-server` sidecar for the on-device runtime (offline card PR A).
//!
//! The child listens on **127.0.0.1 only**, with a random free port and a
//! bearer token. Health is polled until ready; a dead child can be restarted.
//! Production builds resolve the binary and a downloaded GGUF; tests drive a
//! fake server through [`HttpLocalRuntime`](super::local_http::HttpLocalRuntime).

use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use rand_token::random_token;

/// How long [`Sidecar::wait_healthy`] waits for `/health`.
pub const HEALTH_TIMEOUT: Duration = Duration::from_secs(30);
/// Gap between health polls.
pub const HEALTH_POLL: Duration = Duration::from_millis(50);

/// Resolved paths for a production sidecar launch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SidecarPaths {
    pub binary: PathBuf,
    pub model: PathBuf,
}

/// Live child: loopback host, port, token.
#[derive(Debug)]
pub struct Sidecar {
    child: Mutex<Child>,
    pub host: String,
    pub port: u16,
    pub token: String,
    pub binary: PathBuf,
    pub model: PathBuf,
    restarts: AtomicU64,
}

impl Sidecar {
    /// Base URL for OpenAI-compatible calls (`http://127.0.0.1:<port>`).
    pub fn base_url(&self) -> String {
        format!("http://{}:{}", self.host, self.port)
    }

    /// Restarts so far (crash recovery).
    pub fn restart_count(&self) -> u64 {
        self.restarts.load(Ordering::SeqCst)
    }

    /// `true` when `/health` answers 200 with the bearer token.
    pub fn healthy(&self) -> bool {
        health_ok(&self.host, self.port, &self.token, Duration::from_millis(400))
    }

    /// Block until healthy or [`HEALTH_TIMEOUT`].
    pub fn wait_healthy(&self) -> Result<(), String> {
        let deadline = Instant::now() + HEALTH_TIMEOUT;
        while Instant::now() < deadline {
            if self.healthy() {
                return Ok(());
            }
            if !self.alive() {
                return Err("local model server stopped before it was ready".into());
            }
            std::thread::sleep(HEALTH_POLL);
        }
        Err("local model server did not become healthy in time".into())
    }

    /// Child still running.
    pub fn alive(&self) -> bool {
        let mut child = self.child.lock().expect("sidecar lock");
        match child.try_wait() {
            Ok(None) => true,
            Ok(Some(_)) | Err(_) => false,
        }
    }

    /// If the child has exited, spawn it again on the same port and token.
    pub fn ensure_alive(&self) -> Result<(), String> {
        if self.alive() {
            return Ok(());
        }
        let child = spawn_llama(&self.binary, &self.model, &self.host, self.port, &self.token)?;
        *self.child.lock().expect("sidecar lock") = child;
        self.restarts.fetch_add(1, Ordering::SeqCst);
        self.wait_healthy()
    }

    /// Stop the child (best effort). Called from [`Drop`].
    pub fn stop(&self) {
        let mut child = self.child.lock().expect("sidecar lock");
        let _ = child.kill();
        let _ = child.wait();
    }
}

impl Drop for Sidecar {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Pick a free port on 127.0.0.1 (released before the child binds it).
pub fn free_loopback_port() -> Result<u16, String> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).map_err(|e| format!("loopback bind: {e}"))?;
    let port = listener.local_addr().map_err(|e| format!("loopback addr: {e}"))?.port();
    Ok(port)
}

/// Assert a listener is on loopback only (contract for tests and docs).
pub fn is_loopback_host(host: &str) -> bool {
    matches!(host, "127.0.0.1" | "localhost" | "::1" | "[::1]")
}

/// Command line for llama-server: host locked to loopback, API key required.
pub fn llama_args(model: &Path, host: &str, port: u16, token: &str) -> Vec<String> {
    vec![
        "-m".into(),
        model.display().to_string(),
        "--host".into(),
        host.into(),
        "--port".into(),
        port.to_string(),
        "--api-key".into(),
        token.into(),
    ]
}

fn spawn_llama(binary: &Path, model: &Path, host: &str, port: u16, token: &str) -> Result<Child, String> {
    if !is_loopback_host(host) {
        return Err(format!("local model server must bind loopback, not {host}"));
    }
    let mut cmd = Command::new(binary);
    cmd.args(llama_args(model, host, port, token)).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    grokhub_core::proc_util::hide_windows_console(&mut cmd);
    cmd.spawn().map_err(|e| format!("could not start local model server: {e}"))
}

/// Start llama-server on a fresh loopback port.
pub fn start(paths: &SidecarPaths) -> Result<Sidecar, String> {
    if !paths.binary.is_file() {
        return Err(format!("local model binary missing: {}", paths.binary.display()));
    }
    if !paths.model.is_file() {
        return Err(format!("local model file missing: {}", paths.model.display()));
    }
    let host = "127.0.0.1".to_string();
    let port = free_loopback_port()?;
    let token = random_token();
    let child = spawn_llama(&paths.binary, &paths.model, &host, port, &token)?;
    let side = Sidecar {
        child: Mutex::new(child),
        host,
        port,
        token,
        binary: paths.binary.clone(),
        model: paths.model.clone(),
        restarts: AtomicU64::new(0),
    };
    side.wait_healthy()?;
    Ok(side)
}

/// GET `/health` with `Authorization: Bearer <token>`.
pub fn health_ok(host: &str, port: u16, token: &str, timeout: Duration) -> bool {
    if !is_loopback_host(host) {
        return false;
    }
    let url = format!("http://{host}:{port}/health");
    let agent = ureq::AgentBuilder::new().timeout_connect(timeout).timeout_read(timeout).build();
    match agent.get(&url).set("Authorization", &format!("Bearer {token}")).call() {
        Ok(resp) => resp.status() == 200,
        Err(_) => false,
    }
}

/// Resolve binary + GGUF from the environment or the usual install layout.
///
/// - `GROKHUB_LLAMA_SERVER`: path to `llama-server`
/// - `GROKHUB_LOCAL_MODEL`: path to a `.gguf`
/// - else: `{config}/bin/llama-server` and the first `*.gguf` under `{config}/models`
pub fn resolve_paths(config_dir: &Path) -> Option<SidecarPaths> {
    let binary = std::env::var_os("GROKHUB_LLAMA_SERVER")
        .map(PathBuf::from)
        .filter(|p| p.is_file())
        .or_else(|| {
            let p = config_dir.join("bin").join(llama_server_name());
            p.is_file().then_some(p)
        })?;
    let model = std::env::var_os("GROKHUB_LOCAL_MODEL")
        .map(PathBuf::from)
        .filter(|p| p.is_file())
        .or_else(|| first_gguf(&config_dir.join("models")))?;
    Some(SidecarPaths { binary, model })
}

fn llama_server_name() -> &'static str {
    if cfg!(windows) {
        "llama-server.exe"
    } else {
        "llama-server"
    }
}

fn first_gguf(dir: &Path) -> Option<PathBuf> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("gguf"))
        .collect();
    entries.sort();
    entries.into_iter().next()
}

/// Process-wide sidecar. Started lazily when paths resolve.
static GLOBAL: OnceLock<Mutex<Option<Sidecar>>> = OnceLock::new();

fn global_slot() -> &'static Mutex<Option<Sidecar>> {
    GLOBAL.get_or_init(|| Mutex::new(None))
}

/// Ensure a healthy global sidecar when assets exist under `config_dir`.
pub fn ensure_global(config_dir: &Path) -> Result<(), String> {
    let paths = resolve_paths(config_dir).ok_or_else(|| "local model binary or GGUF is not installed".to_string())?;
    let mut slot = global_slot().lock().expect("sidecar global");
    if let Some(side) = slot.as_ref() {
        side.ensure_alive()?;
        return Ok(());
    }
    *slot = Some(start(&paths)?);
    Ok(())
}

/// Base URL + token for the global sidecar, when running.
pub fn global_endpoint() -> Option<(String, String)> {
    let slot = global_slot().lock().expect("sidecar global");
    slot.as_ref().map(|s| (s.base_url(), s.token.clone()))
}

/// Stop the global sidecar (tests / quit).
pub fn stop_global() {
    let mut slot = global_slot().lock().expect("sidecar global");
    if let Some(side) = slot.take() {
        side.stop();
    }
}

/// Whether production assets for a sidecar are present (binary + GGUF).
pub fn assets_present(config_dir: &Path) -> bool {
    resolve_paths(config_dir).is_some()
}

/// Tiny token (32 hex chars) without pulling a new crate.
mod rand_token {
    use std::time::{SystemTime, UNIX_EPOCH};

    pub fn random_token() -> String {
        let t = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(1);
        let mut x = t ^ 0x9e37_79b9_7f4a_7c15;
        let mut out = String::with_capacity(32);
        for _ in 0..8 {
            x = x.wrapping_mul(0x2545_f491_4f6c_dd1d).wrapping_add(1);
            out.push_str(&format!("{:08x}", (x >> 32) as u32));
        }
        out
    }
}

/// Minimal HTTP peer for contract tests: answers `/health` and
/// `/v1/chat/completions` on 127.0.0.1, optional exit after N completions.
#[cfg(test)]
pub mod fake_server {
    use super::*;
    use std::io::{Read, Write};
    use std::net::{Shutdown, TcpStream};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering as AtomicOrdering};
    use std::thread::JoinHandle;

    pub struct FakeServer {
        pub host: String,
        pub port: u16,
        pub token: String,
        stop: std::sync::Arc<AtomicBool>,
        hits: std::sync::Arc<AtomicUsize>,
        join: Option<JoinHandle<()>>,
    }

    impl FakeServer {
        pub fn start(exit_after_completions: Option<usize>) -> Self {
            let listener = TcpListener::bind(("127.0.0.1", 0)).expect("fake bind");
            let port = listener.local_addr().expect("addr").port();
            let token = random_token();
            let stop = std::sync::Arc::new(AtomicBool::new(false));
            let hits = std::sync::Arc::new(AtomicUsize::new(0));
            let stop_t = stop.clone();
            let hits_t = hits.clone();
            let token_t = token.clone();
            let join = std::thread::spawn(move || serve_loop(listener, token_t, stop_t, hits_t, exit_after_completions));
            Self { host: "127.0.0.1".into(), port, token, stop, hits, join: Some(join) }
        }

        pub fn base_url(&self) -> String {
            format!("http://{}:{}", self.host, self.port)
        }

        pub fn completion_hits(&self) -> usize {
            self.hits.load(AtomicOrdering::SeqCst)
        }
    }

    impl Drop for FakeServer {
        fn drop(&mut self) {
            self.stop.store(true, AtomicOrdering::SeqCst);
            let _ = TcpStream::connect((self.host.as_str(), self.port)).map(|s| {
                let _ = s.shutdown(Shutdown::Both);
            });
            if let Some(j) = self.join.take() {
                let _ = j.join();
            }
        }
    }

    fn serve_loop(
        listener: TcpListener,
        token: String,
        stop: std::sync::Arc<AtomicBool>,
        hits: std::sync::Arc<AtomicUsize>,
        exit_after: Option<usize>,
    ) {
        listener.set_nonblocking(true).ok();
        while !stop.load(AtomicOrdering::SeqCst) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let _ = handle_one(&mut stream, &token, &hits);
                    if let Some(n) = exit_after {
                        if hits.load(AtomicOrdering::SeqCst) >= n {
                            break;
                        }
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(_) => break,
            }
        }
    }

    fn handle_one(stream: &mut TcpStream, token: &str, hits: &std::sync::Arc<AtomicUsize>) -> Result<(), ()> {
        stream.set_read_timeout(Some(Duration::from_millis(500))).ok();
        let mut buf = [0u8; 8192];
        let n = stream.read(&mut buf).map_err(|_| ())?;
        let req = String::from_utf8_lossy(&buf[..n]);
        let first = req.lines().next().unwrap_or("");
        let auth_ok = req.lines().any(|l| l.eq_ignore_ascii_case(&format!("Authorization: Bearer {token}")));
        if first.starts_with("GET /health") {
            let body = if auth_ok { "ok" } else { "unauthorized" };
            let code = if auth_ok { "200 OK" } else { "401 Unauthorized" };
            write_resp(stream, code, body);
            return Ok(());
        }
        if first.starts_with("POST /v1/chat/completions") {
            if !auth_ok {
                write_resp(stream, "401 Unauthorized", r#"{"error":"bad token"}"#);
                return Ok(());
            }
            hits.fetch_add(1, AtomicOrdering::SeqCst);
            let inner = if req.contains("at most") || req.contains("summary") {
                serde_json::json!({"summary": "fake summary"})
            } else if req.contains("difficulty") {
                serde_json::json!({"d": 0.5})
            } else if req.contains("\"role\":\"user\"") && !req.contains("JSON object") {
                // plain chat: return text, not a helper object
                serde_json::Value::String("hello from local".into())
            } else {
                serde_json::json!({"label": "routine", "score": 0.91})
            };
            let content = match inner {
                serde_json::Value::String(s) => s,
                other => other.to_string(),
            };
            let body = serde_json::json!({
                "choices": [{"message": {"role": "assistant", "content": content}}],
                "usage": {"prompt_tokens": 1, "completion_tokens": 1}
            })
            .to_string();
            write_resp(stream, "200 OK", &body);
            return Ok(());
        }
        write_resp(stream, "404 Not Found", "missing");
        Ok(())
    }

    fn write_resp(stream: &mut TcpStream, status: &str, body: &str) {
        let resp = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = stream.write_all(resp.as_bytes());
        let _ = stream.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::route::local::LocalRuntime;
    use fake_server::FakeServer;

    #[test]
    fn llama_args_pin_loopback_and_api_key() {
        let args = llama_args(Path::new("/m.gguf"), "127.0.0.1", 4711, "tok");
        assert!(args.windows(2).any(|w| w == ["--host", "127.0.0.1"]));
        assert!(args.windows(2).any(|w| w == ["--port", "4711"]));
        assert!(args.windows(2).any(|w| w == ["--api-key", "tok"]));
        assert!(args.windows(2).any(|w| w == ["-m", "/m.gguf"]));
    }

    #[test]
    fn free_port_is_on_loopback_only() {
        let port = free_loopback_port().expect("port");
        assert!(port > 0);
        assert!(is_loopback_host("127.0.0.1"));
        assert!(!is_loopback_host("0.0.0.0"));
        assert!(!is_loopback_host("192.168.1.1"));
    }

    #[test]
    fn fake_server_health_and_chat_contract() {
        let fake = FakeServer::start(None);
        assert!(health_ok(&fake.host, fake.port, &fake.token, Duration::from_secs(2)));
        assert!(!health_ok(&fake.host, fake.port, "wrong", Duration::from_secs(1)));
        let rt = super::super::local_http::HttpLocalRuntime::new(fake.base_url(), fake.token.clone());
        let v = rt.classify("hello", &["routine", "sensitive"]).expect("classify");
        assert_eq!(v["label"], "routine");
        assert_eq!(v["score"], 0.91);
        let s = rt.summarize("long text", 40).expect("summarize");
        assert_eq!(s["summary"], "fake summary");
        let mut deltas = String::new();
        let chat = rt
            .chat_stream(&[super::super::local::ChatMessage { role: "user".into(), content: "hi".into() }], &mut |d| deltas.push_str(d))
            .expect("chat");
        assert!(!chat.is_empty());
        assert!(!deltas.is_empty());
        assert!(fake.completion_hits() >= 3);
    }

    #[test]
    fn resolve_paths_needs_binary_and_gguf() {
        let dir = std::env::temp_dir().join(format!("gh-sidecar-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        std::fs::create_dir_all(dir.join("models")).unwrap();
        assert!(resolve_paths(&dir).is_none());
        std::fs::write(dir.join("bin").join(llama_server_name()), b"x").unwrap();
        assert!(resolve_paths(&dir).is_none());
        std::fs::write(dir.join("models").join("a.gguf"), b"gguf").unwrap();
        let p = resolve_paths(&dir).expect("paths");
        assert!(p.binary.ends_with(llama_server_name()));
        assert!(p.model.ends_with("a.gguf"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
