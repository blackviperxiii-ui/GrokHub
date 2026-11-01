//! The app's one HTTP client (LU-12). Call sites build an agent from [`Opts`]
//! here instead of naming the HTTP crate, so the crate can change in one place.
//!
//! Stage 1: backed by ureq 2, with the same defaults ureq 2 uses today, pinned
//! in [`Opts::default`] so a later swap cannot quietly change a timeout or the
//! redirect limit. Only what the moved call sites need is here: GET, and POST
//! with a string body, with headers; the status, a header, the content type,
//! the body as a reader or a string; and an error that keeps a 4xx/5xx
//! answer's code, body and `Retry-After`, or what kind of transport failure it
//! was. Stage 3 adds a body as bytes and an opt-in env proxy
//! ([`Opts::proxy_from_env`]), which a `public_only` agent never reads.
//! Stage 4 adds what the agent crate's call sites need: a write timeout, a
//! resolver of the caller's own, a per-request timeout, a JSON body and
//! answer, and [`Request::any_status`] for callers that read a 4xx/5xx
//! answer's headers or whole body themselves.
//!
//! This is a factory, not a guard: every caller still runs its own egress
//! check (or loopback check) before it calls, and the Offline only scans
//! (`offline_guard.rs`, `egress_coverage.rs`) count each `agent` call at the
//! call site. `public_only` dials through [`crate::ssrf::public_only_resolve`],
//! which refuses private addresses and every host while Offline only is on.

use std::fmt;
use std::io::{self, Read};
use std::net::SocketAddr;
use std::time::Duration;

/// Bytes of an error answer's body kept on [`Error::Status`].
pub const STATUS_BODY_CAP: usize = 64 * 1024;

/// A resolver: `host:port` in, the addresses to dial out.
pub type Resolve = fn(&str) -> io::Result<Vec<SocketAddr>>;

/// How an agent dials and how long it waits. Not `PartialEq`: a resolver is a
/// function pointer, and those don't compare meaningfully.
#[derive(Clone, Copy, Debug)]
pub struct Opts {
    /// Time to open the connection.
    pub connect: Duration,
    /// Longest wait for any one read; `None` waits as long as `overall` allows.
    pub read: Option<Duration>,
    /// Longest wait for any one write; `None` waits as long as `overall` allows.
    pub write: Option<Duration>,
    /// Whole request, connect to last body byte; `None` has no cap.
    pub overall: Option<Duration>,
    /// Redirects followed; `0` hands the 3xx back as the answer.
    pub redirects: u32,
    /// Dial public addresses only (and nothing while Offline only is on).
    pub public_only: bool,
    /// Dial through the proxy named in `ALL_PROXY`, `HTTPS_PROXY` or
    /// `HTTP_PROXY` (either case, first set wins), read when the agent is
    /// built. Ignored when `public_only` is set: see [`Opts::reads_env_proxy`].
    pub proxy_from_env: bool,
    /// Dial through this resolver instead of the system's. Ignored when
    /// `public_only` is set, which dials through its own.
    pub resolve: Option<Resolve>,
}

impl Opts {
    /// Whether [`agent`] reads the env proxy: only when asked and not
    /// `public_only`. A proxy is dialed instead of the target, so the target's
    /// address would never reach the public-only resolver; `public_only` wins.
    pub const fn reads_env_proxy(&self) -> bool {
        self.proxy_from_env && !self.public_only
    }
}

impl Default for Opts {
    /// ureq 2's own defaults: 30 s connect, no read, write or overall cap,
    /// 5 redirects, the system resolver.
    fn default() -> Self {
        Self {
            connect: Duration::from_secs(30),
            read: None,
            write: None,
            overall: None,
            redirects: 5,
            public_only: false,
            proxy_from_env: false,
            resolve: None,
        }
    }
}

/// An agent built from [`Opts`]. Cheap to clone; clones share connections.
#[derive(Clone)]
pub struct Agent(ureq::Agent);

/// Build an agent. An env proxy is read only when [`Opts::reads_env_proxy`]
/// says so: a proxy would be dialed instead of the target and skip the
/// public-only resolver.
pub fn agent(opts: Opts) -> Agent {
    let mut b = ureq::AgentBuilder::new()
        .try_proxy_from_env(opts.reads_env_proxy())
        .timeout_connect(opts.connect)
        .redirects(opts.redirects);
    if let Some(read) = opts.read {
        b = b.timeout_read(read);
    }
    if let Some(write) = opts.write {
        b = b.timeout_write(write);
    }
    if let Some(overall) = opts.overall {
        b = b.timeout(overall);
    }
    if opts.public_only {
        b = b.resolver(crate::ssrf::public_only_resolve);
    } else if let Some(resolve) = opts.resolve {
        b = b.resolver(resolve);
    }
    Agent(b.build())
}

impl Agent {
    pub fn get(&self, url: &str) -> Request {
        Request::new(self.0.get(url))
    }

    pub fn post(&self, url: &str) -> Request {
        Request::new(self.0.post(url))
    }
}

/// One request, not sent until [`Request::call`].
#[must_use = "a request does nothing until it is called"]
pub struct Request {
    req: ureq::Request,
    any_status: bool,
}

impl Request {
    fn new(req: ureq::Request) -> Self {
        Self {
            req,
            any_status: false,
        }
    }

    pub fn header(self, name: &str, value: &str) -> Self {
        Self {
            req: self.req.set(name, value),
            ..self
        }
    }

    /// Cap this request, connect to last body byte, at `limit`, in place of
    /// the agent's `overall`.
    pub fn timeout(self, limit: Duration) -> Self {
        Self {
            req: self.req.timeout(limit),
            ..self
        }
    }

    /// Answer a 4xx/5xx as `Ok` too, so the caller reads its status, headers
    /// and whole body as it would a 2xx. No answer is still [`Error::Transport`].
    pub fn any_status(self) -> Self {
        Self {
            any_status: true,
            ..self
        }
    }

    /// Send it. A 4xx/5xx answer is [`Error::Status`] (unless
    /// [`Request::any_status`]); a 1xx-3xx is `Ok`.
    pub fn call(self) -> Result<Response, Error> {
        answer(self.any_status, self.req.call())
    }

    /// Send it with `body` as UTF-8 text. Sets no `Content-Type` of its own:
    /// name one with [`Request::header`]. Answers as [`Request::call`] does.
    pub fn send_string(self, body: &str) -> Result<Response, Error> {
        answer(self.any_status, self.req.send_string(body))
    }

    /// Send it with `body` as is. Sets no `Content-Type` of its own: name one
    /// with [`Request::header`]. Answers as [`Request::call`] does.
    pub fn send_bytes(self, body: &[u8]) -> Result<Response, Error> {
        answer(self.any_status, self.req.send_bytes(body))
    }

    /// Send `body` as compact JSON, with `Content-Type: application/json`
    /// unless one was named. Answers as [`Request::call`] does.
    pub fn send_json(self, body: impl serde::Serialize) -> Result<Response, Error> {
        answer(self.any_status, self.req.send_json(body))
    }
}

fn answer(any_status: bool, sent: Result<ureq::Response, ureq::Error>) -> Result<Response, Error> {
    match sent {
        Ok(resp) => Ok(Response(resp)),
        Err(ureq::Error::Status(_, resp)) if any_status => Ok(Response(resp)),
        Err(err) => Err(Error::from_ureq(err)),
    }
}

/// An answer: 1xx-3xx, or any status after [`Request::any_status`].
pub struct Response(ureq::Response);

impl Response {
    pub fn status(&self) -> u16 {
        self.0.status()
    }

    /// A header's value; the name is matched without regard to case.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.0.header(name)
    }

    /// `Content-Type` up to its first `;`, as sent (case kept, no trim);
    /// `text/plain` when the header is missing.
    pub fn content_type(&self) -> &str {
        self.0.content_type()
    }

    /// The body, unbounded: cap it with `Read::take`.
    pub fn into_reader(self) -> Box<dyn Read + Send + Sync + 'static> {
        self.0.into_reader()
    }

    /// The body as text, refused past 10 MB (ureq 2's `into_string` limit).
    pub fn into_string(self) -> io::Result<String> {
        self.0.into_string()
    }

    /// The body parsed as JSON, unbounded. A parse failure reads
    /// `Failed to read JSON: …`; a read that timed out stays `TimedOut`.
    pub fn into_json<T: serde::de::DeserializeOwned>(self) -> io::Result<T> {
        self.0.into_json()
    }
}

/// Why a request has no 1xx-3xx answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// The server answered 4xx/5xx.
    Status {
        code: u16,
        /// The first [`STATUS_BODY_CAP`] bytes, lossy UTF-8.
        body: String,
        /// The `Retry-After` header as sent (seconds or an HTTP date).
        retry_after: Option<String>,
        /// What `Display` shows: `{url}: status code {code}`.
        message: String,
    },
    /// No answer: bad URL, refused address, DNS, connect, TLS, timeout or read.
    Transport {
        kind: TransportKind,
        message: String,
    },
}

/// What failed when there was no answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransportKind {
    /// The name didn't resolve, or the public-only resolver refused it.
    Dns,
    /// The connection couldn't be opened.
    Connect,
    /// A read or write failed or timed out after connecting.
    Io,
    /// Anything else: bad URL or scheme, TLS, a malformed answer, too many redirects.
    Other,
}

impl Error {
    /// The 4xx/5xx code, if the server answered.
    pub fn status(&self) -> Option<u16> {
        match self {
            Error::Status { code, .. } => Some(*code),
            Error::Transport { .. } => None,
        }
    }

    fn from_ureq(err: ureq::Error) -> Self {
        // ureq's own text, so messages built from `{err}` read as before.
        let message = err.to_string();
        match err {
            ureq::Error::Status(code, resp) => {
                let retry_after = resp.header("retry-after").map(str::to_string);
                let mut buf = Vec::new();
                let _ = resp
                    .into_reader()
                    .take(STATUS_BODY_CAP as u64)
                    .read_to_end(&mut buf);
                Error::Status {
                    code,
                    body: String::from_utf8_lossy(&buf).into_owned(),
                    retry_after,
                    message,
                }
            }
            ureq::Error::Transport(t) => Error::Transport {
                kind: match t.kind() {
                    ureq::ErrorKind::Dns => TransportKind::Dns,
                    ureq::ErrorKind::ConnectionFailed => TransportKind::Connect,
                    ureq::ErrorKind::Io => TransportKind::Io,
                    _ => TransportKind::Other,
                },
                message,
            },
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Status { message, .. } | Error::Transport { message, .. } => {
                f.write_str(message)
            }
        }
    }
}

impl std::error::Error for Error {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::net::TcpListener;
    use std::thread::JoinHandle;
    use std::time::Instant;

    /// Loopback server: answers `replies` in order, one per connection, and
    /// returns each request's head followed by its `Content-Length` body.
    fn serve(replies: Vec<String>) -> (u16, JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = std::thread::spawn(move || {
            let mut heads = Vec::new();
            for reply in replies {
                let (mut sock, _) = listener.accept().unwrap();
                let mut head = Vec::new();
                let mut byte = [0u8; 1];
                while !head.ends_with(b"\r\n\r\n") && sock.read(&mut byte).unwrap_or(0) == 1 {
                    head.push(byte[0]);
                }
                let text = String::from_utf8_lossy(&head).to_ascii_lowercase();
                let len = text
                    .lines()
                    .find_map(|l| l.strip_prefix("content-length:"))
                    .map_or(0, |n| n.trim().parse::<usize>().unwrap());
                let mut body = vec![0u8; len];
                sock.read_exact(&mut body).unwrap();
                head.extend_from_slice(&body);
                heads.push(String::from_utf8_lossy(&head).into_owned());
                sock.write_all(reply.as_bytes()).unwrap();
            }
            heads
        });
        (port, handle)
    }

    fn reply(status: &str, headers: &str, body: &str) -> String {
        format!(
            "HTTP/1.1 {status}\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    #[test]
    fn defaults_are_ureq2s_own() {
        let o = Opts::default();
        assert_eq!(o.connect, Duration::from_secs(30));
        assert_eq!((o.read, o.write, o.overall), (None, None, None));
        assert_eq!(o.redirects, 5);
        assert!(!o.public_only);
        assert!(!o.proxy_from_env);
        assert!(!o.reads_env_proxy());
        assert!(o.resolve.is_none());
    }

    #[test]
    fn ok_answer_reads_status_header_body_and_sends_headers() {
        let (port, server) = serve(vec![reply("200 OK", "X-Probe: yes\r\n", "hello")]);
        let resp = agent(Opts::default())
            .get(&format!("http://127.0.0.1:{port}/props"))
            .header("Authorization", "Bearer t0k")
            .call()
            .unwrap();
        assert_eq!(resp.status(), 200);
        assert_eq!(resp.header("x-probe"), Some("yes"));
        assert_eq!(resp.into_string().unwrap(), "hello");
        let heads = server.join().unwrap();
        assert!(heads[0].starts_with("GET /props HTTP/1.1\r\n"), "{heads:?}");
        assert!(
            heads[0].contains("Authorization: Bearer t0k\r\n"),
            "{heads:?}"
        );
    }

    #[test]
    fn status_error_keeps_code_body_and_retry_after() {
        let (port, server) = serve(vec![reply(
            "429 Too Many Requests",
            "Retry-After: 7\r\n",
            "slow down",
        )]);
        let url = format!("http://127.0.0.1:{port}/x");
        let err = agent(Opts::default()).get(&url).call().err().unwrap();
        server.join().unwrap();
        assert_eq!(err.status(), Some(429));
        assert_eq!(err.to_string(), format!("{url}: status code 429"));
        match err {
            Error::Status {
                code,
                body,
                retry_after,
                ..
            } => {
                assert_eq!(code, 429);
                assert_eq!(body, "slow down");
                assert_eq!(retry_after.as_deref(), Some("7"));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn status_error_body_is_capped() {
        let big = "a".repeat(STATUS_BODY_CAP + 100);
        let (port, server) = serve(vec![reply("500 Internal Server Error", "", &big)]);
        let err = agent(Opts::default())
            .get(&format!("http://127.0.0.1:{port}/"))
            .call()
            .err()
            .unwrap();
        server.join().unwrap();
        match err {
            Error::Status {
                body, retry_after, ..
            } => {
                assert_eq!(body.len(), STATUS_BODY_CAP);
                assert_eq!(retry_after, None);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn redirects_zero_hands_back_the_3xx() {
        let (port, server) = serve(vec![reply("302 Found", "Location: /next\r\n", "")]);
        let resp = agent(Opts {
            redirects: 0,
            ..Opts::default()
        })
        .get(&format!("http://127.0.0.1:{port}/start"))
        .call()
        .unwrap();
        assert_eq!(resp.status(), 302);
        assert_eq!(resp.header("location"), Some("/next"));
        assert_eq!(server.join().unwrap().len(), 1, "one request, no follow");
    }

    #[test]
    fn default_agent_follows_a_redirect() {
        let (port, server) = serve(vec![
            reply("302 Found", "Location: /next\r\n", ""),
            reply("200 OK", "", "there"),
        ]);
        let resp = agent(Opts::default())
            .get(&format!("http://127.0.0.1:{port}/start"))
            .call()
            .unwrap();
        assert_eq!(resp.into_string().unwrap(), "there");
        let heads = server.join().unwrap();
        assert!(heads[1].starts_with("GET /next "), "{heads:?}");
    }

    #[test]
    fn public_only_refuses_loopback_before_dialing() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let err = agent(Opts {
            public_only: true,
            overall: Some(Duration::from_secs(1)),
            ..Opts::default()
        })
        .get(&format!("http://127.0.0.1:{port}/"))
        .call()
        .err()
        .unwrap();
        // ureq reports a resolver refusal as a DNS failure.
        assert!(
            matches!(
                err,
                Error::Transport {
                    kind: TransportKind::Dns,
                    ..
                }
            ),
            "{err:?}"
        );
        assert!(listener.accept().is_err(), "nothing may connect");
    }

    #[test]
    fn overall_timeout_ends_a_stalled_answer() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (sock, _) = listener.accept().unwrap();
            std::thread::sleep(Duration::from_secs(2));
            drop(sock);
        });
        let started = Instant::now();
        let err = agent(Opts {
            overall: Some(Duration::from_millis(200)),
            ..Opts::default()
        })
        .get(&format!("http://127.0.0.1:{port}/"))
        .call()
        .err()
        .unwrap();
        assert!(
            matches!(
                err,
                Error::Transport {
                    kind: TransportKind::Io,
                    ..
                }
            ),
            "{err:?}"
        );
        assert!(started.elapsed() < Duration::from_secs(2), "timed out late");
        server.join().unwrap();
    }

    #[test]
    fn post_sends_the_string_body_with_the_named_content_type() {
        let (port, server) = serve(vec![reply("200 OK", "", "{\"ok\":true}")]);
        let resp = agent(Opts::default())
            .post(&format!("http://127.0.0.1:{port}/token"))
            .header("content-type", "application/x-www-form-urlencoded")
            .send_string("a=1&b=%C3%A9")
            .unwrap();
        assert_eq!(resp.into_string().unwrap(), "{\"ok\":true}");
        let req = &server.join().unwrap()[0];
        assert!(req.starts_with("POST /token HTTP/1.1\r\n"), "{req:?}");
        assert!(
            req.contains("content-type: application/x-www-form-urlencoded\r\n"),
            "{req:?}"
        );
        assert!(req.contains("Content-Length: 12\r\n"), "{req:?}");
        assert!(req.ends_with("\r\n\r\na=1&b=%C3%A9"), "{req:?}");
    }

    #[test]
    fn post_status_error_keeps_the_body() {
        let (port, server) = serve(vec![reply(
            "400 Bad Request",
            "",
            "{\"error\":\"authorization_pending\"}",
        )]);
        let err = agent(Opts::default())
            .post(&format!("http://127.0.0.1:{port}/token"))
            .send_string("x")
            .err()
            .unwrap();
        server.join().unwrap();
        match err {
            Error::Status { code, body, .. } => {
                assert_eq!(code, 400);
                assert_eq!(body, "{\"error\":\"authorization_pending\"}");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn content_type_drops_parameters_and_defaults_to_text_plain() {
        let (port, server) = serve(vec![
            reply("200 OK", "Content-Type: Text/HTML ; charset=utf-8\r\n", ""),
            reply("200 OK", "", ""),
        ]);
        let a = agent(Opts::default());
        let url = format!("http://127.0.0.1:{port}/");
        assert_eq!(a.get(&url).call().unwrap().content_type(), "Text/HTML ");
        assert_eq!(a.get(&url).call().unwrap().content_type(), "text/plain");
        server.join().unwrap();
    }

    #[test]
    fn send_bytes_sends_the_body_as_is() {
        let (port, server) = serve(vec![reply("200 OK", "", "ok")]);
        let body = b"--b\r\n\xff\x00raw\r\n--b--\r\n";
        let resp = agent(Opts::default())
            .post(&format!("http://127.0.0.1:{port}/stt"))
            .header("content-type", "multipart/form-data; boundary=b")
            .send_bytes(body)
            .unwrap();
        assert_eq!(resp.into_string().unwrap(), "ok");
        let req = &server.join().unwrap()[0];
        assert!(req.starts_with("POST /stt HTTP/1.1\r\n"), "{req:?}");
        assert!(
            req.contains("content-type: multipart/form-data; boundary=b\r\n"),
            "{req:?}"
        );
        assert!(req.contains("Content-Length: 19\r\n"), "{req:?}");
        // `serve` reads the body lossily: the 0xff byte shows as U+FFFD.
        assert!(
            req.ends_with("\r\n\r\n--b\r\n\u{fffd}\0raw\r\n--b--\r\n"),
            "{req:?}"
        );
    }

    /// Env proxy tests share the process environment; one at a time.
    static PROXY_ENV: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Points `ALL_PROXY` (read before the other proxy variables) at a
    /// loopback port until dropped, then puts the old value back.
    struct ProxyEnv {
        old: Option<std::ffi::OsString>,
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    impl ProxyEnv {
        fn set(port: u16) -> Self {
            let lock = PROXY_ENV.lock().unwrap_or_else(|e| e.into_inner());
            let old = std::env::var_os("ALL_PROXY");
            std::env::set_var("ALL_PROXY", format!("http://127.0.0.1:{port}"));
            Self { old, _lock: lock }
        }
    }

    impl Drop for ProxyEnv {
        fn drop(&mut self) {
            match self.old.take() {
                Some(v) => std::env::set_var("ALL_PROXY", v),
                None => std::env::remove_var("ALL_PROXY"),
            }
        }
    }

    #[test]
    fn proxy_from_env_dials_the_env_proxy() {
        let (proxy, server) = serve(vec![reply("200 OK", "", "via proxy")]);
        let env = ProxyEnv::set(proxy);
        let opts = Opts {
            proxy_from_env: true,
            ..Opts::default()
        };
        assert!(opts.reads_env_proxy());
        let a = agent(opts);
        drop(env);
        // The name never resolves: only the proxy can have answered.
        let resp = a.get("http://proxied.invalid/x").call().unwrap();
        assert_eq!(resp.into_string().unwrap(), "via proxy");
        let req = &server.join().unwrap()[0];
        assert!(
            req.starts_with("GET http://proxied.invalid/x HTTP/1.1\r\n"),
            "{req:?}"
        );
    }

    #[test]
    fn env_proxy_is_not_read_unless_asked() {
        let proxy = TcpListener::bind("127.0.0.1:0").unwrap();
        proxy.set_nonblocking(true).unwrap();
        let _env = ProxyEnv::set(proxy.local_addr().unwrap().port());
        let (port, server) = serve(vec![reply("200 OK", "", "direct")]);
        let resp = agent(Opts::default())
            .get(&format!("http://127.0.0.1:{port}/direct"))
            .call()
            .unwrap();
        assert_eq!(resp.into_string().unwrap(), "direct");
        let req = &server.join().unwrap()[0];
        assert!(req.starts_with("GET /direct HTTP/1.1\r\n"), "{req:?}");
        assert!(proxy.accept().is_err(), "the env proxy must not be dialed");
    }

    #[test]
    fn public_only_wins_over_proxy_from_env() {
        let proxy = TcpListener::bind("127.0.0.1:0").unwrap();
        proxy.set_nonblocking(true).unwrap();
        let proxy_port = proxy.local_addr().unwrap().port();
        let _env = ProxyEnv::set(proxy_port);
        let target = TcpListener::bind("127.0.0.1:0").unwrap();
        target.set_nonblocking(true).unwrap();
        let port = target.local_addr().unwrap().port();
        let opts = Opts {
            public_only: true,
            proxy_from_env: true,
            overall: Some(Duration::from_secs(1)),
            ..Opts::default()
        };
        assert!(!opts.reads_env_proxy());
        let err = agent(opts)
            .get(&format!("http://127.0.0.1:{port}/"))
            .call()
            .err()
            .unwrap();
        // The resolver was asked for the target itself, not for a proxy.
        let msg = err.to_string();
        assert!(
            matches!(
                err,
                Error::Transport {
                    kind: TransportKind::Dns,
                    ..
                }
            ),
            "{err:?}"
        );
        assert!(
            msg.contains(&format!("resolve dns name '127.0.0.1:{port}'")),
            "{msg}"
        );
        assert!(!msg.contains(&format!("'127.0.0.1:{proxy_port}'")), "{msg}");
        assert!(proxy.accept().is_err(), "the env proxy must not be dialed");
        assert!(target.accept().is_err(), "nothing may connect");
    }

    #[test]
    fn transport_kind_tells_connect_from_a_bad_url() {
        let port = {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.local_addr().unwrap().port()
        };
        let refused = agent(Opts::default())
            .get(&format!("http://127.0.0.1:{port}/"))
            .call()
            .err()
            .unwrap();
        assert!(
            matches!(
                refused,
                Error::Transport {
                    kind: TransportKind::Connect,
                    ..
                }
            ),
            "{refused:?}"
        );
        assert_eq!(refused.status(), None);
        let bad = agent(Opts::default())
            .get("not a url")
            .call()
            .err()
            .unwrap();
        assert!(
            matches!(
                bad,
                Error::Transport {
                    kind: TransportKind::Other,
                    ..
                }
            ),
            "{bad:?}"
        );
    }

    #[test]
    fn any_status_hands_back_a_4xx_with_its_headers_and_whole_body() {
        let big = "b".repeat(STATUS_BODY_CAP + 10);
        let (port, server) = serve(vec![reply(
            "401 Unauthorized",
            "WWW-Authenticate: Bearer resource_metadata=\"x\"\r\n",
            &big,
        )]);
        let resp = agent(Opts::default())
            .post(&format!("http://127.0.0.1:{port}/mcp"))
            .any_status()
            .send_string("{}")
            .unwrap();
        assert_eq!(resp.status(), 401);
        assert_eq!(
            resp.header("www-authenticate"),
            Some("Bearer resource_metadata=\"x\"")
        );
        assert_eq!(resp.into_string().unwrap().len(), STATUS_BODY_CAP + 10);
        server.join().unwrap();
    }

    #[test]
    fn any_status_still_errs_with_no_answer() {
        let port = {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.local_addr().unwrap().port()
        };
        let err = agent(Opts::default())
            .get(&format!("http://127.0.0.1:{port}/"))
            .any_status()
            .call()
            .err()
            .unwrap();
        assert!(
            matches!(
                err,
                Error::Transport {
                    kind: TransportKind::Connect,
                    ..
                }
            ),
            "{err:?}"
        );
    }

    #[test]
    fn request_timeout_caps_one_request_on_an_uncapped_agent() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (sock, _) = listener.accept().unwrap();
            std::thread::sleep(Duration::from_secs(2));
            drop(sock);
        });
        let started = Instant::now();
        let err = agent(Opts::default())
            .get(&format!("http://127.0.0.1:{port}/"))
            .timeout(Duration::from_millis(200))
            .call()
            .err()
            .unwrap();
        assert!(
            matches!(
                err,
                Error::Transport {
                    kind: TransportKind::Io,
                    ..
                }
            ),
            "{err:?}"
        );
        assert!(started.elapsed() < Duration::from_secs(2), "timed out late");
        server.join().unwrap();
    }

    #[test]
    fn write_timeout_ends_a_send_nobody_reads() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (sock, _) = listener.accept().unwrap();
            std::thread::sleep(Duration::from_secs(3));
            drop(sock);
        });
        let started = Instant::now();
        let body = vec![b'x'; 64 * 1024 * 1024];
        let err = agent(Opts {
            write: Some(Duration::from_millis(200)),
            ..Opts::default()
        })
        .post(&format!("http://127.0.0.1:{port}/"))
        .send_bytes(&body)
        .err()
        .unwrap();
        assert!(
            matches!(
                err,
                Error::Transport {
                    kind: TransportKind::Io,
                    ..
                }
            ),
            "{err:?}"
        );
        assert!(started.elapsed() < Duration::from_secs(3), "timed out late");
        server.join().unwrap();
    }

    #[test]
    fn send_json_names_json_and_sends_compact_json() {
        let (port, server) = serve(vec![
            reply("200 OK", "Content-Type: application/json\r\n", "{\"n\":2}"),
            reply("200 OK", "", "not json"),
        ]);
        let url = format!("http://127.0.0.1:{port}/v1");
        let resp = agent(Opts::default())
            .post(&url)
            .send_json(serde_json::json!({"a": 1, "b": "é"}))
            .unwrap();
        let v: serde_json::Value = resp.into_json().unwrap();
        assert_eq!(v, serde_json::json!({"n": 2}));
        let bad = agent(Opts::default())
            .post(&url)
            .header("Content-Type", "application/vnd.x+json")
            .send_json(serde_json::json!([1]))
            .unwrap()
            .into_json::<serde_json::Value>()
            .err()
            .unwrap();
        assert_eq!(bad.kind(), io::ErrorKind::InvalidData);
        assert!(
            bad.to_string().starts_with("Failed to read JSON: "),
            "{bad}"
        );
        let heads = server.join().unwrap();
        assert!(
            heads[0].contains("Content-Type: application/json\r\n"),
            "{heads:?}"
        );
        assert!(
            heads[0].ends_with("\r\n\r\n{\"a\":1,\"b\":\"é\"}"),
            "{heads:?}"
        );
        assert!(
            heads[1].contains("Content-Type: application/vnd.x+json\r\n")
                && !heads[1].contains("application/json"),
            "{heads:?}"
        );
        assert!(heads[1].ends_with("\r\n\r\n[1]"), "{heads:?}");
    }

    /// Every name dials loopback on the port asked for.
    fn resolve_test(netloc: &str) -> io::Result<Vec<SocketAddr>> {
        let (_, port) = netloc.rsplit_once(':').unwrap();
        Ok(vec![SocketAddr::from((
            [127, 0, 0, 1],
            port.parse().unwrap(),
        ))])
    }

    #[test]
    fn resolve_dials_through_the_callers_resolver() {
        let (port, server) = serve(vec![reply("200 OK", "", "via")]);
        let resp = agent(Opts {
            resolve: Some(resolve_test),
            ..Opts::default()
        })
        .get(&format!("http://mcp.example.test:{port}/x"))
        .call()
        .unwrap();
        assert_eq!(resp.into_string().unwrap(), "via");
        let heads = server.join().unwrap();
        assert!(
            heads[0].contains(&format!("Host: mcp.example.test:{port}\r\n")),
            "{heads:?}"
        );
    }

    #[test]
    fn public_only_wins_over_the_callers_resolver() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let err = agent(Opts {
            public_only: true,
            resolve: Some(resolve_test),
            overall: Some(Duration::from_secs(1)),
            ..Opts::default()
        })
        .get(&format!("http://127.0.0.1:{port}/"))
        .call()
        .err()
        .unwrap();
        assert!(
            matches!(
                err,
                Error::Transport {
                    kind: TransportKind::Dns,
                    ..
                }
            ),
            "{err:?}"
        );
        assert!(listener.accept().is_err(), "nothing may connect");
    }
}
