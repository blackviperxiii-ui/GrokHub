//! The app's one HTTP client (LU-12). Call sites build an agent from [`Opts`]
//! here instead of naming the HTTP crate, so the crate can change in one place.
//!
//! Stages 1-4 put every call site in `crates/` on this facade, backed by
//! ureq 2. Stage 5 swaps the inside to ureq 3 (`~3.5`) and keeps what callers
//! see: the same defaults ([`Opts::default`] is still ureq 2's), the same
//! redirect counting, an error that keeps a 4xx/5xx answer's code, body and
//! `Retry-After` (ureq 3 runs with `http_status_as_error(false)`), the same
//! [`TransportKind`] for each failure and message text in ureq 2's shape, a
//! body read that times out as `io::ErrorKind::TimedOut`, and no env proxy
//! unless asked ([`Opts::proxy_from_env`], never for `public_only`). Each URL
//! is parsed as ureq 2 parsed it (WHATWG, the `url` crate) before ureq 3 sees
//! it, so the host dialed is the host the egress checks read. TLS is rustls
//! with the ring provider and the webpki roots.
//!
//! This is a factory, not a guard: every caller still runs its own egress
//! check (or loopback check) before it calls, and the Offline only scans
//! (`offline_guard.rs`, `egress_coverage.rs`) count each `agent` call at the
//! call site. `public_only` dials through [`crate::ssrf::public_only_resolve`],
//! which refuses private addresses and every host while Offline only is on.

use std::fmt;
use std::io::{self, Read};
use std::net::{SocketAddr, ToSocketAddrs};
use std::time::Duration;

use ureq::http::{Method, Uri};
use ureq::unversioned::resolver::{ResolvedSocketAddrs, Resolver};
use ureq::unversioned::transport::{
    ConnectionDetails, Connector, DefaultConnector, NextTimeout, Transport,
};

/// Bytes of an error answer's body kept on [`Error::Status`].
pub const STATUS_BODY_CAP: usize = 64 * 1024;

/// Most a body read with [`Response::into_string`] may be (ureq 2's limit).
const INTO_STRING_LIMIT: usize = 10 * 1024 * 1024;

/// Most addresses ureq 3 keeps from one lookup.
const MAX_ADDRS: usize = 16;

/// A resolver: `host:port` in, the addresses to dial out.
pub type Resolve = fn(&str) -> io::Result<Vec<SocketAddr>>;

/// How an agent dials and how long it waits. Not `PartialEq`: a resolver is a
/// function pointer, and those don't compare meaningfully.
#[derive(Clone, Copy, Debug)]
pub struct Opts {
    /// Time to open the connection, TLS handshake included.
    pub connect: Duration,
    /// Longest wait for any one read; `None` waits as long as `overall` allows.
    /// Each read starts the wait again, so a stream that keeps sending (SSE)
    /// is never cut by it, only one that goes quiet.
    pub read: Option<Duration>,
    /// Longest wait for any one write; `None` waits as long as `overall` allows.
    pub write: Option<Duration>,
    /// Whole request, connect to last body byte; `None` has no cap.
    pub overall: Option<Duration>,
    /// Redirect limit, counted as ureq 2 counts it: `0` hands the 3xx back as
    /// the answer; `n` follows up to `n - 1` and fails on the `n`th.
    pub redirects: u32,
    /// Dial public addresses only (and nothing while Offline only is on).
    pub public_only: bool,
    /// Dial through the proxy named in `ALL_PROXY`, `HTTPS_PROXY` or
    /// `HTTP_PROXY` (either case, first set wins), read when the agent is
    /// built. Ignored when `public_only` is set: see [`Opts::reads_env_proxy`].
    pub proxy_from_env: bool,
    /// Dial through this resolver instead of the system's. Ignored when
    /// `public_only` or `loopback_self_signed` is set, which dial through
    /// their own.
    pub resolve: Option<Resolve>,
    /// For a game client's API on this machine whose certificate is
    /// self-signed: accept any certificate, but dial loopback addresses only
    /// and `https` only, so the relaxed check can't reach another host.
    /// `public_only` wins (it refuses loopback, so nothing is dialed).
    pub loopback_self_signed: bool,
}

impl Opts {
    /// Whether [`agent`] reads the env proxy: only when asked and not
    /// `public_only`. A proxy is dialed instead of the target, so the target's
    /// address would never reach the public-only resolver; `public_only` wins.
    pub const fn reads_env_proxy(&self) -> bool {
        self.proxy_from_env && !self.public_only && !self.loopback_self_signed
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
            loopback_self_signed: false,
        }
    }
}

/// An agent built from [`Opts`]. Cheap to clone; clones share connections.
#[derive(Clone)]
pub struct Agent {
    inner: ureq::Agent,
    redirects: u32,
}

/// Build an agent. An env proxy is read only when [`Opts::reads_env_proxy`]
/// says so: a proxy would be dialed instead of the target and skip the
/// public-only resolver.
pub fn agent(opts: Opts) -> Agent {
    let proxy = if opts.reads_env_proxy() {
        env_proxy()
    } else {
        None
    };
    let mut config = ureq::Agent::config_builder()
        // A 4xx/5xx comes back as a response, so its body and Retry-After
        // can be read; `answer` turns it into `Error::Status`.
        .http_status_as_error(false)
        // ureq 3 reads the env proxy by default; only `env_proxy` may set one.
        .proxy(proxy)
        // ureq 2 counted the first request as a hop: see `Opts::redirects`.
        .max_redirects(opts.redirects.saturating_sub(1))
        .timeout_connect(Some(opts.connect))
        .timeout_per_read(opts.read)
        .timeout_per_write(opts.write)
        .timeout_global(opts.overall);
    let resolve: Resolve = if opts.public_only {
        crate::ssrf::public_only_resolve
    } else if opts.loopback_self_signed {
        config = config.https_only(true).tls_config(
            ureq::tls::TlsConfig::builder()
                .disable_verification(true)
                .build(),
        );
        loopback_only_resolve
    } else {
        opts.resolve.unwrap_or(system_resolve)
    };
    let inner = ureq::Agent::with_parts(
        config.build(),
        TagConnect(DefaultConnector::new()),
        FnResolver(resolve),
    );
    Agent {
        inner,
        redirects: opts.redirects,
    }
}

/// The proxy named in the env, read as ureq 2 read it: the first of these
/// that is set and parses, and no `NO_PROXY`.
fn env_proxy() -> Option<ureq::Proxy> {
    [
        "ALL_PROXY",
        "all_proxy",
        "HTTPS_PROXY",
        "https_proxy",
        "HTTP_PROXY",
        "http_proxy",
    ]
    .iter()
    .filter_map(|name| std::env::var(name).ok())
    .find_map(|v| ureq::Proxy::new(&v).ok())
}

/// The system resolver, as ureq 2 used it.
fn system_resolve(netloc: &str) -> io::Result<Vec<SocketAddr>> {
    Ok(netloc.to_socket_addrs()?.collect())
}

/// Loopback addresses only: anything else is refused before it is dialed.
fn loopback_only_resolve(netloc: &str) -> io::Result<Vec<SocketAddr>> {
    let all = system_resolve(netloc)?;
    if all.iter().any(|a| !a.ip().is_loopback()) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("refused non-loopback address {netloc}"),
        ));
    }
    Ok(all)
}

/// A [`Resolve`] on ureq 3's resolver trait: `public_only_resolve` (the
/// public-only resolver), the loopback-only one, a caller's own, or the
/// system's. It is asked for every name ureq dials, a proxy's included. A
/// failure carries ureq 2's `resolve dns name '…'` text and counts as
/// [`TransportKind::Dns`].
struct FnResolver(Resolve);

impl fmt::Debug for FnResolver {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("FnResolver")
    }
}

impl Resolver for FnResolver {
    fn resolve(
        &self,
        uri: &Uri,
        _config: &ureq::config::Config,
        _timeout: NextTimeout,
    ) -> Result<ResolvedSocketAddrs, ureq::Error> {
        let (Some(scheme), Some(authority)) = (uri.scheme_str(), uri.authority()) else {
            return Err(ureq::Error::BadUri(uri.to_string()));
        };
        let port = match (authority.port_u16(), scheme) {
            (Some(port), _) => port,
            (None, "http") => 80,
            (None, "https") => 443,
            _ => return Err(ureq::Error::BadUri(format!("unknown scheme: {scheme}"))),
        };
        let netloc = format!("{}:{port}", authority.host());
        let addrs = (self.0)(&netloc)
            .map_err(|e| Fail::Dns(format!("resolve dns name '{netloc}': {e}")).into_ureq())?;
        let mut out = self.empty();
        for addr in addrs.into_iter().take(MAX_ADDRS) {
            out.push(addr);
        }
        if out.is_empty() {
            let host = authority.host();
            return Err(Fail::Dns(format!("No ip address for {host}")).into_ureq());
        }
        Ok(out)
    }
}

/// ureq 3's own connector chain (CONNECT proxy, TCP, rustls), with each
/// failure to open the connection marked as one, as ureq 2 reported it. A
/// timeout keeps its own reason.
#[derive(Debug)]
struct TagConnect(DefaultConnector);

impl Connector<()> for TagConnect {
    type Out = Box<dyn Transport>;

    fn connect(
        &self,
        details: &ConnectionDetails,
        chained: Option<()>,
    ) -> Result<Option<Self::Out>, ureq::Error> {
        self.0.connect(details, chained).map_err(|e| match e {
            ureq::Error::Timeout(_)
            | ureq::Error::ConnectProxyFailed(_)
            | ureq::Error::Other(_) => e,
            other => Fail::Connect(other).into_ureq(),
        })
    }
}

/// What the resolver and connector above hand back inside `ureq::Error::Other`.
#[derive(Debug)]
enum Fail {
    /// Text after `Dns Failed: `.
    Dns(String),
    /// The error that kept the connection from opening.
    Connect(ureq::Error),
}

impl Fail {
    fn into_ureq(self) -> ureq::Error {
        ureq::Error::Other(Box::new(self))
    }
}

impl fmt::Display for Fail {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Fail::Dns(m) => write!(f, "Dns Failed: {m}"),
            Fail::Connect(e) => write!(f, "Connection Failed: {e}"),
        }
    }
}

impl std::error::Error for Fail {}

impl Agent {
    pub fn get(&self, url: &str) -> Request {
        self.request(Method::GET, url)
    }

    pub fn head(&self, url: &str) -> Request {
        self.request(Method::HEAD, url)
    }

    pub fn post(&self, url: &str) -> Request {
        self.request(Method::POST, url)
    }

    fn request(&self, method: Method, url: &str) -> Request {
        Request {
            agent: self.clone(),
            method,
            url: url.to_string(),
            headers: Vec::new(),
            timeout: None,
            any_status: false,
        }
    }
}

/// One request, not sent until [`Request::call`].
#[must_use = "a request does nothing until it is called"]
pub struct Request {
    agent: Agent,
    method: Method,
    url: String,
    /// Names keep the caller's case; a second value for a name replaces the first.
    headers: Vec<(String, String)>,
    timeout: Option<Duration>,
    any_status: bool,
}

impl Request {
    /// Set a header, replacing one of the same name (in any case).
    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.retain(|(n, _)| !n.eq_ignore_ascii_case(name));
        self.headers.push((name.to_string(), value.to_string()));
        self
    }

    /// Cap this request, connect to last body byte, at `limit`, in place of
    /// the agent's `overall`.
    pub fn timeout(self, limit: Duration) -> Self {
        Self {
            timeout: Some(limit),
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
        self.send(())
    }

    /// Send it with `body` as UTF-8 text. Sets no `Content-Type` of its own:
    /// name one with [`Request::header`]. Answers as [`Request::call`] does.
    pub fn send_string(self, body: &str) -> Result<Response, Error> {
        self.send(body)
    }

    /// Send it with `body` as is. Sets no `Content-Type` of its own: name one
    /// with [`Request::header`]. Answers as [`Request::call`] does.
    pub fn send_bytes(self, body: &[u8]) -> Result<Response, Error> {
        self.send(body)
    }

    /// Send `body` as compact JSON, with `Content-Type: application/json`
    /// unless one was named. Answers as [`Request::call`] does.
    pub fn send_json(self, body: impl serde::Serialize) -> Result<Response, Error> {
        let json = serde_json::to_vec(&body).map_err(|e| Error::Transport {
            kind: TransportKind::Other,
            message: format!("{}: Failed to serialize JSON: {e}", self.url),
        })?;
        let named = self
            .headers
            .iter()
            .any(|(n, _)| n.eq_ignore_ascii_case("content-type"));
        let req = if named {
            self
        } else {
            self.header("Content-Type", "application/json")
        };
        req.send(json)
    }

    fn send(self, body: impl ureq::AsSendBody) -> Result<Response, Error> {
        let Request {
            agent,
            method,
            url,
            headers,
            timeout,
            any_status,
        } = self;
        // Parsed the way ureq 2 parsed it, then handed on in its normal form;
        // a fragment is never sent.
        let mut parsed = url::Url::parse(&url).map_err(|e| Error::Transport {
            kind: TransportKind::Other,
            message: format!("Bad URL: failed to parse URL: {e}: {url}"),
        })?;
        parsed.set_fragment(None);
        let url = parsed.as_str();
        let other = |message: String| Error::Transport {
            kind: TransportKind::Other,
            message: format!("{url}: {message}"),
        };
        let mut builder = ureq::http::Request::builder().method(method).uri(url);
        for (name, value) in &headers {
            builder = builder.header(name.as_str(), value.as_str());
        }
        let req = builder
            .body(body)
            .map_err(|e| other(format!("Bad Header: {e}")))?;
        let req = match timeout {
            Some(limit) => agent
                .inner
                .configure_request(req)
                .timeout_global(Some(limit))
                .build(),
            None => req,
        };
        let resp = agent
            .inner
            .run(req)
            .map_err(|e| Error::from_ureq(url, agent.redirects, e))?;
        let code = resp.status().as_u16();
        // ureq 2 with a limit of 1 refused the first 3xx; ureq 3 at 0 hands it back.
        if agent.redirects == 1 && (300..399).contains(&code) {
            return Err(other(too_many_redirects(1)));
        }
        if code >= 400 && !any_status {
            return Err(Error::from_status(resp));
        }
        Ok(Response(resp))
    }
}

fn too_many_redirects(limit: u32) -> String {
    format!("Too Many Redirects: reached max redirects ({limit})")
}

/// An answer: 1xx-3xx, or any status after [`Request::any_status`].
pub struct Response(ureq::http::Response<ureq::Body>);

impl Response {
    pub fn status(&self) -> u16 {
        self.0.status().as_u16()
    }

    /// A header's value; the name is matched without regard to case. `None`
    /// when the value isn't visible ASCII.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.0.headers().get(name)?.to_str().ok()
    }

    /// `Content-Type` up to its first `;`, as sent (case kept, no trim);
    /// `text/plain` when the header is missing.
    pub fn content_type(&self) -> &str {
        self.header("content-type")
            .map(|v| v.split(';').next().unwrap_or(v))
            .unwrap_or("text/plain")
    }

    /// The body, unbounded: cap it with `Read::take`. A read that times out
    /// is `io::ErrorKind::TimedOut`.
    pub fn into_reader(self) -> Box<dyn Read + Send + Sync + 'static> {
        Box::new(BodyReader(self.0.into_body().into_reader()))
    }

    /// The body as text (invalid UTF-8 replaced), refused past 10 MB.
    pub fn into_string(self) -> io::Result<String> {
        let mut buf = Vec::new();
        self.into_reader()
            .take(INTO_STRING_LIMIT as u64 + 1)
            .read_to_end(&mut buf)?;
        if buf.len() > INTO_STRING_LIMIT {
            return Err(io::Error::other("response too big for into_string"));
        }
        Ok(String::from_utf8_lossy(&buf).into_owned())
    }

    /// The body parsed as JSON, unbounded. A parse failure reads
    /// `Failed to read JSON: …`; a read that timed out stays `TimedOut`.
    pub fn into_json<T: serde::de::DeserializeOwned>(self) -> io::Result<T> {
        serde_json::from_reader(self.into_reader()).map_err(|e| {
            if e.io_error_kind() == Some(io::ErrorKind::TimedOut) {
                return io::Error::new(io::ErrorKind::TimedOut, e.to_string());
            }
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Failed to read JSON: {e}"),
            )
        })
    }
}

/// ureq 3's body reader, with a timeout (any of ureq's, wrapped in an
/// `io::Error`) read back as `TimedOut` with ureq 2's text.
struct BodyReader(ureq::BodyReader<'static>);

impl Read for BodyReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.read(buf).map_err(|e| {
            let timed_out = e
                .get_ref()
                .and_then(|inner| inner.downcast_ref::<ureq::Error>())
                .is_some_and(|inner| matches!(inner, ureq::Error::Timeout(_)));
            if timed_out {
                io::Error::new(io::ErrorKind::TimedOut, "timed out reading response")
            } else {
                e
            }
        })
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
    /// The name didn't resolve, or the public-only (or loopback-only)
    /// resolver refused it.
    Dns,
    /// The connection couldn't be opened: refused, unreachable, timed out, or
    /// the TLS handshake failed (a bad certificate included).
    Connect,
    /// A read or write failed or timed out after connecting.
    Io,
    /// Anything else: bad URL or scheme, a malformed answer, too many
    /// redirects, a proxy that refused the tunnel.
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

    fn from_status(resp: ureq::http::Response<ureq::Body>) -> Self {
        use ureq::ResponseExt;
        let code = resp.status().as_u16();
        let message = format!("{}: status code {code}", resp.get_uri());
        let retry_after = resp
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        let mut buf = Vec::new();
        let _ = Response(resp)
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

    /// Kind and text as ureq 2 gave them: `{url}: {what failed}: {detail}`.
    fn from_ureq(url: &str, redirects: u32, err: ureq::Error) -> Self {
        use ureq::Timeout;
        use TransportKind::{Connect, Dns, Io, Other};
        let (kind, what) = match err {
            ureq::Error::Other(boxed) => match boxed.downcast::<Fail>() {
                Ok(fail) => match *fail {
                    Fail::Dns(m) => (Dns, format!("Dns Failed: {m}")),
                    Fail::Connect(ureq::Error::Io(e)) => {
                        (Connect, format!("Connection Failed: Connect error: {e}"))
                    }
                    Fail::Connect(e) => (Connect, format!("Connection Failed: {e}")),
                },
                Err(other) => (Other, other.to_string()),
            },
            ureq::Error::Timeout(Timeout::Connect) => (
                Connect,
                "Connection Failed: Connect error: connection timed out".to_string(),
            ),
            ureq::Error::Timeout(Timeout::Resolve) => {
                (Dns, "Dns Failed: timed out resolving".to_string())
            }
            ureq::Error::Timeout(Timeout::PerWrite | Timeout::SendRequest | Timeout::SendBody) => {
                (Io, "Network Error: timed out writing request".to_string())
            }
            ureq::Error::Timeout(_) => {
                (Io, "Network Error: timed out reading response".to_string())
            }
            ureq::Error::Io(e) => (Io, format!("Network Error: {e}")),
            ureq::Error::HostNotFound => (Dns, "Dns Failed".to_string()),
            ureq::Error::ConnectionFailed => (Connect, "Connection Failed".to_string()),
            ureq::Error::TooManyRedirects => (Other, too_many_redirects(redirects)),
            ureq::Error::BadUri(m) => (Other, format!("Bad URL: {m}")),
            ureq::Error::Protocol(e) => (Other, format!("Bad Status: {e}")),
            ureq::Error::Http(e) => (Other, format!("Bad Header: {e}")),
            ureq::Error::ConnectProxyFailed(m) => (Other, format!("Proxy failed to connect: {m}")),
            other => (Other, other.to_string()),
        };
        Error::Transport {
            kind,
            message: format!("{url}: {what}"),
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
    use std::io::{BufRead, Write};
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
        assert!(!o.loopback_self_signed);
    }

    /// ureq 3 writes header names in lower case (HTTP names are
    /// case-insensitive); the tests read what went on the wire that way.
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
            heads[0].contains("authorization: Bearer t0k\r\n"),
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
        assert!(req.contains("content-length: 12\r\n"), "{req:?}");
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
        assert!(req.contains("content-length: 19\r\n"), "{req:?}");
        // `serve` reads the body lossily: the 0xff byte shows as U+FFFD.
        assert!(
            req.ends_with("\r\n\r\n--b\r\n\u{fffd}\0raw\r\n--b--\r\n"),
            "{req:?}"
        );
    }

    /// Env proxy tests share the process environment; one at a time.
    static PROXY_ENV: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Every proxy variable ureq 2 or ureq 3 reads.
    const PROXY_VARS: [&str; 6] = [
        "ALL_PROXY",
        "all_proxy",
        "HTTPS_PROXY",
        "https_proxy",
        "HTTP_PROXY",
        "http_proxy",
    ];

    /// Points proxy variables at a loopback port until dropped, then puts
    /// the old values back.
    struct ProxyEnv {
        old: Vec<(&'static str, Option<std::ffi::OsString>)>,
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    impl ProxyEnv {
        /// `ALL_PROXY` only (read before the other proxy variables).
        fn set(port: u16) -> Self {
            Self::set_vars(&["ALL_PROXY"], port)
        }

        fn set_vars(names: &[&'static str], port: u16) -> Self {
            let lock = PROXY_ENV.lock().unwrap_or_else(|e| e.into_inner());
            let old = names
                .iter()
                .map(|&name| {
                    let was = std::env::var_os(name);
                    unsafe { std::env::set_var(name, format!("http://127.0.0.1:{port}")) };
                    (name, was)
                })
                .collect();
            Self { old, _lock: lock }
        }
    }

    impl Drop for ProxyEnv {
        fn drop(&mut self) {
            for (name, was) in self.old.drain(..) {
                match was {
                    Some(v) => unsafe { std::env::set_var(name, v) },
                    None => unsafe { std::env::remove_var(name) },
                }
            }
        }
    }

    /// Reads one request head (to the blank line) off `sock`.
    fn read_head(sock: &mut std::net::TcpStream) -> String {
        let mut head = Vec::new();
        let mut byte = [0u8; 1];
        while !head.ends_with(b"\r\n\r\n") && sock.read(&mut byte).unwrap_or(0) == 1 {
            head.push(byte[0]);
        }
        String::from_utf8_lossy(&head).into_owned()
    }

    /// ureq 3 tunnels through an HTTP proxy with `CONNECT`, for `http`
    /// targets too (ureq 2 sent those as absolute-form requests).
    #[test]
    fn proxy_from_env_dials_the_env_proxy() {
        let proxy = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = proxy.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut sock, _) = proxy.accept().unwrap();
            let connect = read_head(&mut sock);
            sock.write_all(b"HTTP/1.1 200 Connection established\r\n\r\n")
                .unwrap();
            let get = read_head(&mut sock);
            sock.write_all(reply("200 OK", "", "via proxy").as_bytes())
                .unwrap();
            (connect, get)
        });
        let env = ProxyEnv::set(port);
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
        let (connect, get) = server.join().unwrap();
        assert!(
            connect.starts_with("CONNECT proxied.invalid:80 HTTP/1.1\r\n"),
            "{connect:?}"
        );
        assert!(get.starts_with("GET /x HTTP/1.1\r\n"), "{get:?}");
    }

    /// ureq 3 reads every proxy variable by default; the facade sets none
    /// unless asked, so with all six pointing at a live port the default
    /// agent still dials the target itself.
    #[test]
    fn every_env_proxy_is_ignored_by_default() {
        let proxy = TcpListener::bind("127.0.0.1:0").unwrap();
        proxy.set_nonblocking(true).unwrap();
        let _env = ProxyEnv::set_vars(&PROXY_VARS, proxy.local_addr().unwrap().port());
        let (port, server) = serve(vec![reply("200 OK", "", "direct"); 2]);
        let url = format!("http://127.0.0.1:{port}/direct");
        let plain = agent(Opts::default()).get(&url).call().unwrap();
        assert_eq!(plain.into_string().unwrap(), "direct");
        let public_only = agent(Opts {
            public_only: true,
            proxy_from_env: true,
            resolve: Some(resolve_test),
            ..Opts::default()
        });
        // public_only refuses loopback, so this proves only that no proxy
        // was dialed in its place.
        assert!(public_only.get(&url).call().is_err());
        let direct = agent(Opts {
            resolve: Some(resolve_test),
            ..Opts::default()
        });
        let via = direct
            .get(&format!("http://elsewhere.test:{port}/direct"))
            .call()
            .unwrap();
        assert_eq!(via.into_string().unwrap(), "direct");
        let heads = server.join().unwrap();
        assert!(
            heads
                .iter()
                .all(|h| h.starts_with("GET /direct HTTP/1.1\r\n")),
            "{heads:?}"
        );
        assert!(proxy.accept().is_err(), "an env proxy must not be dialed");
    }

    /// The xAI stream's idle limit: `read` caps each read, not the whole
    /// body, so a stream that keeps dripping outlives it, and one that goes
    /// quiet ends as `TimedOut` (loopback only).
    #[test]
    fn read_timeout_lets_a_slow_drip_run_and_ends_a_stall() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut sock, _) = listener.accept().unwrap();
            read_head(&mut sock);
            sock.write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
            )
            .unwrap();
            for i in 0..8 {
                std::thread::sleep(Duration::from_millis(100));
                sock.write_all(format!("data: {i}\n\n").as_bytes()).unwrap();
            }
            std::thread::sleep(Duration::from_secs(3));
            drop(sock);
        });
        let started = Instant::now();
        let resp = agent(Opts {
            read: Some(Duration::from_millis(400)),
            ..Opts::default()
        })
        .get(&format!("http://127.0.0.1:{port}/v1/responses"))
        .call()
        .unwrap();
        let mut events = Vec::new();
        let mut lines = io::BufReader::new(resp.into_reader()).lines();
        let err = loop {
            match lines.next() {
                Some(Ok(line)) if line.is_empty() => {}
                Some(Ok(line)) => events.push(line),
                Some(Err(e)) => break e,
                None => panic!("the stream ended with no timeout: {events:?}"),
            }
        };
        let took = started.elapsed();
        let want: Vec<String> = (0..8).map(|i| format!("data: {i}")).collect();
        assert_eq!(events, want);
        assert_eq!(err.kind(), io::ErrorKind::TimedOut, "{err}");
        assert_eq!(err.to_string(), "timed out reading response");
        assert!(
            took >= Duration::from_millis(800),
            "the drip outlived one read limit: {took:?}"
        );
        assert!(
            took < Duration::from_secs(3),
            "the stall ended it: {took:?}"
        );
        server.join().unwrap();
    }

    #[test]
    fn redirect_limit_counts_as_ureq_2_did() {
        // A limit of 1 refuses the first 3xx.
        let (port, server) = serve(vec![reply("302 Found", "Location: /next\r\n", "")]);
        let err = agent(Opts {
            redirects: 1,
            ..Opts::default()
        })
        .get(&format!("http://127.0.0.1:{port}/start"))
        .call()
        .err()
        .unwrap();
        server.join().unwrap();
        assert!(
            matches!(
                err,
                Error::Transport {
                    kind: TransportKind::Other,
                    ..
                }
            ),
            "{err:?}"
        );
        assert!(
            err.to_string()
                .ends_with("Too Many Redirects: reached max redirects (1)"),
            "{err}"
        );
        // A limit of 2 follows one and refuses the second.
        let (port, server) = serve(vec![
            reply("302 Found", "Location: /a\r\n", ""),
            reply("302 Found", "Location: /b\r\n", ""),
        ]);
        let err = agent(Opts {
            redirects: 2,
            ..Opts::default()
        })
        .get(&format!("http://127.0.0.1:{port}/start"))
        .call()
        .err()
        .unwrap();
        assert_eq!(server.join().unwrap().len(), 2);
        assert!(
            err.to_string()
                .ends_with("Too Many Redirects: reached max redirects (2)"),
            "{err}"
        );
    }

    /// A URL is read as ureq 2 read it (WHATWG): a space or a non-ASCII
    /// character is escaped rather than refused, and the fragment stays home.
    #[test]
    fn urls_are_parsed_as_ureq_2_parsed_them() {
        let (port, server) = serve(vec![reply("200 OK", "", "ok")]);
        let resp = agent(Opts::default())
            .get(&format!("http://127.0.0.1:{port}/a b?q=\u{e9}#frag"))
            .call()
            .unwrap();
        assert_eq!(resp.into_string().unwrap(), "ok");
        let heads = server.join().unwrap();
        assert!(
            heads[0].starts_with("GET /a%20b?q=%C3%A9 HTTP/1.1\r\n"),
            "{heads:?}"
        );
    }

    #[test]
    fn head_sends_head() {
        let (port, server) = serve(vec![reply("204 No Content", "", "")]);
        let resp = agent(Opts::default())
            .head(&format!("http://127.0.0.1:{port}/models"))
            .call()
            .unwrap();
        assert_eq!(resp.status(), 204);
        assert!(server.join().unwrap()[0].starts_with("HEAD /models HTTP/1.1\r\n"));
    }

    /// The relaxed-certificate agent never leaves this machine and never
    /// speaks plain `http`; both refusals come before anything is dialed.
    #[test]
    fn loopback_self_signed_dials_loopback_https_only() {
        let a = agent(Opts {
            loopback_self_signed: true,
            proxy_from_env: true,
            overall: Some(Duration::from_secs(1)),
            ..Opts::default()
        });
        // TEST-NET-1: refused by the resolver, never dialed.
        let far = a.get("https://192.0.2.1:2999/x").call().err().unwrap();
        assert!(
            matches!(
                far,
                Error::Transport {
                    kind: TransportKind::Dns,
                    ..
                }
            ),
            "{far:?}"
        );
        assert!(
            far.to_string().contains("refused non-loopback address"),
            "{far}"
        );
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let plain = a
            .get(&format!("http://127.0.0.1:{port}/x"))
            .call()
            .err()
            .unwrap();
        assert!(
            matches!(
                plain,
                Error::Transport {
                    kind: TransportKind::Other,
                    ..
                }
            ),
            "{plain:?}"
        );
        assert!(listener.accept().is_err(), "plain http must not be dialed");
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
            heads[0].contains("content-type: application/json\r\n"),
            "{heads:?}"
        );
        assert!(
            heads[0].ends_with("\r\n\r\n{\"a\":1,\"b\":\"é\"}"),
            "{heads:?}"
        );
        assert!(
            heads[1].contains("content-type: application/vnd.x+json\r\n")
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
            heads[0].contains(&format!("host: mcp.example.test:{port}\r\n")),
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
