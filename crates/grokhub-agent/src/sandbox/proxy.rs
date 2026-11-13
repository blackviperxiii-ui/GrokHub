//! The egress proxy for sandboxed shell commands. It listens on loopback and
//! runs [`guard_egress`] on every host a command asks for, so offline mode and
//! the egress log cover `curl`, `git`, `cargo` and the rest the same way they
//! cover Loadout's own calls. A refused host gets a 403 with the reason.
//!
//! It handles `CONNECT host:port` (HTTPS and anything tunneled) and plain
//! `http://` requests. When the app itself runs behind a proxy, it goes
//! through that one, as the command would have.
//!
//! Before the guard, the host must be on the sandbox's network list
//! ([`net_allow`]). Both checks use the name the client sent, so a host that
//! isn't allowed is refused before any DNS lookup.
//!
//! On Linux with namespaces this proxy is a command's only way out (see
//! `netns`). Elsewhere it is routing by environment variable, and a program
//! that ignores the proxy variables still reaches the network directly.
//!
//! [`net_allow`]: super::net_allow
//!
//! [`guard_egress`]: crate::harness::egress::guard_egress

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use base64::Engine as _;

use crate::harness::GateOutcome;
use crate::harness::{guard_egress, EgressReq};

/// Requests the proxy let out, ever. A shell call that sees this move ran
/// while something fetched from the network, so its output is fenced.
static LET_OUT: AtomicU64 = AtomicU64::new(0);

/// How many requests the proxy has let out so far.
pub fn let_out_count() -> u64 {
    LET_OUT.load(Ordering::SeqCst)
}

#[cfg(test)]
pub fn note_let_out_for_tests() {
    LET_OUT.fetch_add(1, Ordering::SeqCst);
}

const HEAD_CAP: usize = 32 * 1024;
const HEAD_TIMEOUT: Duration = Duration::from_secs(15);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// Hosts a sandboxed command reaches without the proxy.
pub const NO_PROXY: &str = "localhost,127.0.0.1,::1";

/// The proxy for this config folder (one per folder, started on first use):
/// `127.0.0.1:port`.
pub fn address(config_dir: &Path) -> Result<String, String> {
    static RUNNING: OnceLock<Mutex<HashMap<PathBuf, String>>> = OnceLock::new();
    let mut running = RUNNING
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if let Some(addr) = running.get(config_dir) {
        return Ok(addr.clone());
    }
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| format!("egress proxy: {e}"))?;
    let addr = listener
        .local_addr()
        .map_err(|e| format!("egress proxy: {e}"))?
        .to_string();
    let dir = config_dir.to_path_buf();
    std::thread::Builder::new()
        .name("egress-proxy".into())
        .spawn(move || {
            for conn in listener.incoming().flatten() {
                let dir = dir.clone();
                let _ = std::thread::Builder::new()
                    .name("egress-proxy-conn".into())
                    .spawn(move || serve(conn, &dir));
            }
        })
        .map_err(|e| format!("egress proxy: {e}"))?;
    running.insert(config_dir.to_path_buf(), addr.clone());
    Ok(addr)
}

/// The Unix socket that reaches the proxy for this config folder, for a
/// command in its own network namespace (its relay connects here). The
/// socket sits in a fresh owner-only folder under the temp folder; each
/// connection is carried to the proxy's loopback port.
#[cfg(unix)]
pub fn unix_bridge(config_dir: &Path) -> Result<PathBuf, String> {
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::UnixListener;
    static RUNNING: OnceLock<Mutex<HashMap<PathBuf, PathBuf>>> = OnceLock::new();
    let mut running = RUNNING
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if let Some(path) = running.get(config_dir) {
        return Ok(path.clone());
    }
    let addr = address(config_dir)?;
    let err = |e: std::io::Error| format!("egress bridge: {e}");
    let dir = std::env::temp_dir().join(format!("egress-{}-{}", std::process::id(), running.len()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir(&dir).map_err(err)?;
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).map_err(err)?;
    let path = dir.join("s");
    let listener = UnixListener::bind(&path).map_err(err)?;
    std::thread::Builder::new()
        .name("egress-bridge".into())
        .spawn(move || {
            for conn in listener.incoming().flatten() {
                let addr = addr.clone();
                let _ = std::thread::Builder::new()
                    .name("egress-bridge-conn".into())
                    .spawn(move || {
                        if let Ok(server) = TcpStream::connect(&addr) {
                            bridge_pipe(conn, server);
                        }
                    });
            }
        })
        .map_err(err)?;
    running.insert(config_dir.to_path_buf(), path.clone());
    Ok(path)
}

#[cfg(unix)]
fn bridge_pipe(client: std::os::unix::net::UnixStream, server: TcpStream) {
    let (Ok(mut client_r), Ok(mut server_w)) = (client.try_clone(), server.try_clone()) else {
        return;
    };
    let up = std::thread::spawn(move || {
        let _ = std::io::copy(&mut client_r, &mut server_w);
        let _ = server_w.shutdown(Shutdown::Write);
    });
    let (mut server_r, mut client_w) = (server, client);
    let _ = std::io::copy(&mut server_r, &mut client_w);
    let _ = client_w.shutdown(Shutdown::Write);
    let _ = up.join();
}

/// The variables that point a command's HTTP clients at the proxy.
pub fn env(addr: &str) -> Vec<(&'static str, String)> {
    let url = format!("http://{addr}");
    let mut out = Vec::new();
    for key in [
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "http_proxy",
        "https_proxy",
        "npm_config_proxy",
        "npm_config_https_proxy",
        "YARN_HTTP_PROXY",
        "YARN_HTTPS_PROXY",
        "GLOBAL_AGENT_HTTP_PROXY",
        "GLOBAL_AGENT_HTTPS_PROXY",
    ] {
        out.push((key, url.clone()));
    }
    for key in [
        "NO_PROXY",
        "no_proxy",
        "npm_config_noproxy",
        "GLOBAL_AGENT_NO_PROXY",
    ] {
        out.push((key, NO_PROXY.to_string()));
    }
    out
}

/// Variables removed so nothing routes around the proxy (a SOCKS fallback).
pub const UNSET: &[&str] = &["ALL_PROXY", "all_proxy"];

fn serve(client: TcpStream, config_dir: &Path) {
    serve_with(client, &|host, url| {
        let basis = super::net_allow::verdict(config_dir, host)?;
        match guard_egress(config_dir, &EgressReq::new(url, &[])) {
            GateOutcome::Allow => Ok(Pass {
                inward_ok: basis.reaches_private(),
                #[cfg(unix)]
                unmask: super::mask::session().intercepts(host),
            }),
            GateOutcome::Park { reason, .. } | GateOutcome::Refuse { reason } => Err(reason),
        }
    });
}

/// What the guard lets a request do.
#[derive(Debug, Clone, Copy, Default)]
struct Pass {
    /// The host may resolve to a loopback or private address.
    inward_ok: bool,
    /// A masked token may go to this host: a tunnel is ended here and the
    /// real token put back (see `mitm`).
    #[cfg(unix)]
    unmask: bool,
}

/// `guard` gets the host as the client named it and the URL to log, and runs
/// before anything is dialed or looked up.
fn serve_with(mut client: TcpStream, guard: &dyn Fn(&str, &str) -> Result<Pass, String>) {
    let _ = client.set_read_timeout(Some(HEAD_TIMEOUT));
    let Some((head, extra)) = read_head(&mut client) else {
        return;
    };
    let _ = client.set_read_timeout(None);
    let Some(req) = parse_head(&head) else {
        refuse(&mut client, 400, "Loadout egress proxy: bad request");
        return;
    };
    let pass = match guard(&req.host, &req.guard_url()) {
        Ok(pass) => pass,
        Err(reason) => {
            refuse(
                &mut client,
                403,
                &format!("Loadout egress guard refused {}: {reason}", req.host),
            );
            return;
        }
    };
    LET_OUT.fetch_add(1, Ordering::SeqCst);
    let upstream = Upstream::from_env();
    let route = upstream.route(&req.host, req.connect);
    let server = match route {
        // The app's own upstream proxy is the person's setting, wherever it is.
        Some(proxy) => dial(&proxy.host, proxy.port, true),
        None => dial(&req.host, req.port, pass.inward_ok),
    };
    let Ok(mut server) = server else {
        refuse(
            &mut client,
            502,
            &format!("Loadout egress proxy: could not reach {}", req.host),
        );
        return;
    };
    let sent = if req.connect {
        match route {
            Some(proxy) => tunnel_via(&mut server, proxy, &req),
            None => Ok(()),
        }
        .and_then(|()| {
            client
                .write_all(b"HTTP/1.1 200 Connection established\r\n\r\n")
                .map_err(|e| e.to_string())
        })
    } else {
        let head = req.forward_head(route);
        server.write_all(head.as_bytes()).map_err(|e| e.to_string())
    };
    if let Err(why) = sent {
        refuse(
            &mut client,
            502,
            &format!("Loadout egress proxy: {} not reached: {why}", req.host),
        );
        return;
    }
    #[cfg(unix)]
    if req.connect && pass.unmask {
        if let Some(mitm) = super::mitm::session() {
            let _ = mitm.serve(super::mask::session(), &req.host, client, &extra, server);
            return;
        }
    }
    if !extra.is_empty() && server.write_all(&extra).is_err() {
        return;
    }
    pipe(client, server);
}

fn read_head(client: &mut TcpStream) -> Option<(String, Vec<u8>)> {
    let mut buf = Vec::with_capacity(1024);
    let mut chunk = [0u8; 4096];
    loop {
        let n = client.read(&mut chunk).ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let extra = buf.split_off(end + 4);
            return Some((String::from_utf8_lossy(&buf).into_owned(), extra));
        }
        if buf.len() > HEAD_CAP {
            return None;
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Request {
    connect: bool,
    method: String,
    host: String,
    port: u16,
    /// Origin-form path for a plain request; empty for CONNECT.
    path: String,
    /// The absolute target as the client sent it.
    target: String,
    version: String,
    headers: Vec<String>,
}

impl Request {
    fn guard_url(&self) -> String {
        let scheme = if self.connect { "https" } else { "http" };
        format!("{scheme}://{}:{}", host_for_url(&self.host), self.port)
    }

    /// The head sent on: origin-form direct, absolute-form to an upstream
    /// proxy. One request per connection, so a reused client connection
    /// can't reach a second host past the guard.
    fn forward_head(&self, upstream: Option<&ProxyAddr>) -> String {
        let target = if upstream.is_some() {
            &self.target
        } else {
            &self.path
        };
        let mut out = format!("{} {} {}\r\n", self.method, target, self.version);
        for line in &self.headers {
            let name = line.split(':').next().unwrap_or("").trim();
            if [
                "connection",
                "proxy-connection",
                "keep-alive",
                "proxy-authorization",
            ]
            .iter()
            .any(|h| name.eq_ignore_ascii_case(h))
            {
                continue;
            }
            out.push_str(line);
            out.push_str("\r\n");
        }
        if let Some(auth) = upstream.and_then(|p| p.auth.as_deref()) {
            out.push_str(&format!("Proxy-Authorization: Basic {auth}\r\n"));
        }
        out.push_str("Connection: close\r\n\r\n");
        out
    }
}

fn host_for_url(host: &str) -> String {
    if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]")
    } else {
        host.to_string()
    }
}

fn parse_head(head: &str) -> Option<Request> {
    let mut lines = head.split("\r\n");
    let first = lines.next()?;
    let mut parts = first.split(' ');
    let method = parts.next()?.to_string();
    let target = parts.next()?.to_string();
    let version = parts.next()?.to_string();
    if parts.next().is_some() || !version.starts_with("HTTP/1.") {
        return None;
    }
    let headers: Vec<String> = lines
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect();
    if method.eq_ignore_ascii_case("CONNECT") {
        let (host, port) = split_host_port(&target, None)?;
        return Some(Request {
            connect: true,
            method,
            host,
            port,
            path: String::new(),
            target,
            version,
            headers,
        });
    }
    let rest = target.strip_prefix("http://")?;
    let (authority, path) = match rest.find(['/', '?']) {
        Some(i) => (&rest[..i], rest[i..].to_string()),
        None => (rest, "/".to_string()),
    };
    let path = if path.starts_with('?') {
        format!("/{path}")
    } else {
        path
    };
    if authority.contains('@') || authority.contains('\\') {
        return None;
    }
    let (host, port) = split_host_port(authority, Some(80))?;
    Some(Request {
        connect: false,
        method,
        host,
        port,
        path,
        target,
        version,
        headers,
    })
}

fn split_host_port(authority: &str, default: Option<u16>) -> Option<(String, u16)> {
    let (host, port) = if let Some(v6) = authority.strip_prefix('[') {
        let (host, rest) = v6.split_once(']')?;
        let port = match rest.strip_prefix(':') {
            Some(p) => p.parse().ok()?,
            None if rest.is_empty() => default?,
            None => return None,
        };
        (host.to_string(), port)
    } else {
        match authority.rsplit_once(':') {
            Some((h, p)) => (h.to_string(), p.parse().ok()?),
            None => (authority.to_string(), default?),
        }
    };
    let ok = !host.is_empty()
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ':'));
    (ok && port != 0).then(|| (host.to_ascii_lowercase(), port))
}

/// Resolve and connect. Unless `inward_ok`, an address on loopback or the
/// local network is skipped, so an allowed name that resolves inward (DNS
/// rebinding) reaches nothing.
fn dial(host: &str, port: u16, inward_ok: bool) -> Result<TcpStream, String> {
    let addrs = (host, port).to_socket_addrs().map_err(|e| e.to_string())?;
    let mut last = String::from("no address");
    for addr in addrs {
        if !inward_ok && super::net_allow::is_inward(addr.ip()) {
            last = format!("{} is a local address", addr.ip());
            continue;
        }
        match TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT) {
            Ok(stream) => return Ok(stream),
            Err(e) => last = e.to_string(),
        }
    }
    Err(last)
}

fn tunnel_via(server: &mut TcpStream, proxy: &ProxyAddr, req: &Request) -> Result<(), String> {
    let mut head = format!(
        "CONNECT {0}:{1} HTTP/1.1\r\nHost: {0}:{1}\r\n",
        host_for_url(&req.host),
        req.port
    );
    if let Some(auth) = &proxy.auth {
        head.push_str(&format!("Proxy-Authorization: Basic {auth}\r\n"));
    }
    head.push_str("\r\n");
    server
        .write_all(head.as_bytes())
        .map_err(|e| e.to_string())?;
    let _ = server.set_read_timeout(Some(CONNECT_TIMEOUT));
    let (reply, extra) = read_head(server).ok_or("upstream proxy closed")?;
    let _ = server.set_read_timeout(None);
    let ok = reply
        .split(' ')
        .nth(1)
        .is_some_and(|code| code.starts_with('2'));
    if !ok || !extra.is_empty() {
        let status = reply.lines().next().unwrap_or("").trim();
        return Err(format!("the app's own proxy refused it ({status})"));
    }
    Ok(())
}

fn refuse(client: &mut TcpStream, code: u16, text: &str) {
    let reason = match code {
        400 => "Bad Request",
        403 => "Forbidden",
        _ => "Bad Gateway",
    };
    let body = format!("{text}\n");
    let _ = client.write_all(
        format!(
            "HTTP/1.1 {code} {reason}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .as_bytes(),
    );
    let _ = client.shutdown(Shutdown::Both);
}

fn pipe(client: TcpStream, server: TcpStream) {
    let (Ok(client_r), Ok(server_w)) = (client.try_clone(), server.try_clone()) else {
        return;
    };
    let up = std::thread::spawn(move || copy_then_close(client_r, server_w));
    copy_then_close(server, client);
    let _ = up.join();
}

fn copy_then_close(mut from: TcpStream, mut to: TcpStream) {
    let _ = std::io::copy(&mut from, &mut to);
    let _ = to.shutdown(Shutdown::Write);
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProxyAddr {
    host: String,
    port: u16,
    /// Base64 `user:pass` for Proxy-Authorization.
    auth: Option<String>,
}

/// The proxy the app itself was started behind, read from its own
/// environment (the command's copy is pointed at this proxy instead).
#[derive(Debug, Default, Clone)]
struct Upstream {
    https: Option<ProxyAddr>,
    http: Option<ProxyAddr>,
    no_proxy: Vec<String>,
}

impl Upstream {
    fn from_env() -> Self {
        static CACHED: OnceLock<Upstream> = OnceLock::new();
        CACHED
            .get_or_init(|| {
                let var = |a: &str, b: &str| std::env::var(a).or_else(|_| std::env::var(b)).ok();
                Upstream::parse(
                    var("HTTPS_PROXY", "https_proxy").as_deref(),
                    var("HTTP_PROXY", "http_proxy").as_deref(),
                    var("NO_PROXY", "no_proxy").as_deref(),
                )
            })
            .clone()
    }

    fn parse(https: Option<&str>, http: Option<&str>, no_proxy: Option<&str>) -> Self {
        Upstream {
            https: https.and_then(parse_proxy),
            http: http.and_then(parse_proxy),
            no_proxy: no_proxy
                .unwrap_or("")
                .split(',')
                .map(|s| s.trim().to_ascii_lowercase())
                .filter(|s| !s.is_empty())
                .collect(),
        }
    }

    fn route(&self, host: &str, connect: bool) -> Option<&ProxyAddr> {
        if self.bypass(host) {
            return None;
        }
        if connect {
            self.https.as_ref().or(self.http.as_ref())
        } else {
            self.http.as_ref()
        }
    }

    fn bypass(&self, host: &str) -> bool {
        let host = host.trim_matches(['[', ']']);
        if matches!(host, "localhost" | "127.0.0.1" | "::1") {
            return true;
        }
        self.no_proxy.iter().any(|entry| {
            if entry == "*" {
                return true;
            }
            let bare = entry.trim_start_matches("*.").trim_start_matches('.');
            host == bare || host.ends_with(&format!(".{bare}"))
        })
    }
}

fn parse_proxy(raw: &str) -> Option<ProxyAddr> {
    let raw = raw.trim();
    let rest = raw.strip_prefix("http://").unwrap_or(raw);
    if rest.contains("://") {
        // socks or https proxies aren't spoken here; go direct.
        return None;
    }
    let authority = rest.split('/').next().unwrap_or("");
    let (auth, hostport) = match authority.rsplit_once('@') {
        Some((userinfo, hp)) => (
            Some(base64::engine::general_purpose::STANDARD.encode(userinfo)),
            hp,
        ),
        None => (None, authority),
    };
    let (host, port) = split_host_port(hostport, Some(80))?;
    Some(ProxyAddr { host, port, auth })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connect_and_plain_requests_parse_and_bad_ones_do_not() {
        let r =
            parse_head("CONNECT example.com:443 HTTP/1.1\r\nHost: example.com:443\r\n").unwrap();
        assert!(r.connect);
        assert_eq!((r.host.as_str(), r.port), ("example.com", 443));
        assert_eq!(r.guard_url(), "https://example.com:443");

        let r = parse_head("GET http://Example.com/a?b=1 HTTP/1.1\r\nHost: example.com\r\nProxy-Connection: keep-alive\r\nAccept: */*\r\n").unwrap();
        assert!(!r.connect);
        assert_eq!(
            (r.host.as_str(), r.port, r.path.as_str()),
            ("example.com", 80, "/a?b=1")
        );
        let head = r.forward_head(None);
        assert!(head.starts_with("GET /a?b=1 HTTP/1.1\r\n"), "{head}");
        assert!(head.contains("Accept: */*\r\n"));
        assert!(!head.to_ascii_lowercase().contains("proxy-connection"));
        assert!(head.ends_with("Connection: close\r\n\r\n"));

        let r = parse_head("CONNECT [::1]:8443 HTTP/1.1\r\n").unwrap();
        assert_eq!((r.host.as_str(), r.port), ("::1", 8443));
        assert_eq!(r.guard_url(), "https://[::1]:8443");

        for bad in [
            "GET /local HTTP/1.1\r\n",
            "GET https://example.com/ HTTP/1.1\r\n",
            "GET http://user@evil.com/ HTTP/1.1\r\n",
            "GET http://evil.com\\@127.0.0.1/ HTTP/1.1\r\n",
            "CONNECT example.com HTTP/1.1\r\n",
            "CONNECT example.com:0 HTTP/1.1\r\n",
            "CONNECT ex ample.com:443 HTTP/1.1\r\n",
            "GET http://example.com/ HTTP/2\r\n",
        ] {
            assert!(parse_head(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn the_app_s_own_proxy_is_used_except_where_it_says_not_to() {
        let up = Upstream::parse(
            Some("http://user:pw@10.0.0.2:3128"),
            None,
            Some("internal.test, .corp.test,localhost"),
        );
        let proxy = up.route("example.com", true).expect("via upstream");
        assert_eq!((proxy.host.as_str(), proxy.port), ("10.0.0.2", 3128));
        assert_eq!(proxy.auth.as_deref(), Some("dXNlcjpwdw=="));
        assert!(
            up.route("example.com", false).is_none(),
            "no http proxy set"
        );
        assert!(up.route("internal.test", true).is_none());
        assert!(up.route("a.corp.test", true).is_none());
        assert!(up.route("127.0.0.1", true).is_none());
        assert!(Upstream::parse(Some("socks5://10.0.0.2:1080"), None, None)
            .route("example.com", true)
            .is_none());
    }

    #[test]
    fn the_command_env_points_every_client_at_the_proxy() {
        let env = env("127.0.0.1:4000");
        let get = |k: &str| {
            env.iter()
                .find(|(key, _)| *key == k)
                .map(|(_, v)| v.as_str())
        };
        assert_eq!(get("HTTPS_PROXY"), Some("http://127.0.0.1:4000"));
        assert_eq!(get("http_proxy"), Some("http://127.0.0.1:4000"));
        assert_eq!(get("npm_config_https_proxy"), Some("http://127.0.0.1:4000"));
        assert_eq!(get("NO_PROXY"), Some(NO_PROXY));
    }

    /// A config folder whose rules let the proxy reach loopback, where the
    /// test servers listen.
    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("gh-proxy-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("permission-rules.json"),
            r#"{"allow":["WebFetch(domain:127.0.0.1)"]}"#,
        )
        .unwrap();
        dir
    }

    /// A plain request through the proxy reaches a loopback server and is
    /// sent on in origin form; the reply comes back.
    #[test]
    fn a_plain_request_goes_through_the_guard_and_comes_back() {
        let dir = scratch("plain");
        let server = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = server.local_addr().unwrap().port();
        let seen = std::thread::spawn(move || {
            let (mut conn, _) = server.accept().unwrap();
            let (head, _) = read_head(&mut conn).unwrap();
            conn.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok")
                .unwrap();
            head
        });
        let addr = address(&dir).unwrap();
        let mut client = TcpStream::connect(&addr).unwrap();
        client
            .write_all(
                format!("GET http://127.0.0.1:{port}/x HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n")
                    .as_bytes(),
            )
            .unwrap();
        let mut reply = String::new();
        client.read_to_string(&mut reply).unwrap();
        assert!(reply.ends_with("ok"), "{reply}");
        let head = seen.join().unwrap();
        assert!(head.starts_with("GET /x HTTP/1.1\r\n"), "{head}");
        assert_eq!(address(&dir).unwrap(), addr, "one proxy per folder");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A host the guard refuses gets a 403 with the reason, and nothing is dialed.
    #[test]
    fn a_refused_host_gets_a_403_with_the_reason() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let asked = std::sync::Arc::new(Mutex::new(Vec::<String>::new()));
        let log = std::sync::Arc::clone(&asked);
        std::thread::spawn(move || {
            let (conn, _) = listener.accept().unwrap();
            serve_with(conn, &|_host, url| -> Result<Pass, String> {
                log.lock().unwrap().push(url.to_string());
                Err("offline only is on".into())
            });
        });
        let mut client = TcpStream::connect(addr).unwrap();
        client
            .write_all(b"CONNECT api.example.test:443 HTTP/1.1\r\n\r\n")
            .unwrap();
        let mut reply = String::new();
        client.read_to_string(&mut reply).unwrap();
        assert!(reply.starts_with("HTTP/1.1 403 Forbidden\r\n"), "{reply}");
        assert!(
            reply.contains("refused api.example.test: offline only is on"),
            "{reply}"
        );
        assert_eq!(*asked.lock().unwrap(), ["https://api.example.test:443"]);
    }

    /// A host off the network list is refused with the list's reason before
    /// anything is looked up: the 403 names the list, not a failed lookup.
    #[test]
    fn a_host_off_the_list_is_refused_before_any_lookup() {
        let dir = scratch("offlist");
        let mut client = TcpStream::connect(address(&dir).unwrap()).unwrap();
        client
            .write_all(b"CONNECT c2VjcmV0.exfil.invalid:443 HTTP/1.1\r\n\r\n")
            .unwrap();
        let mut reply = String::new();
        client.read_to_string(&mut reply).unwrap();
        assert!(reply.starts_with("HTTP/1.1 403 Forbidden\r\n"), "{reply}");
        assert!(
            reply.contains("not on the sandbox's network list"),
            "{reply}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// An allowed name that resolves to loopback is not dialed unless the
    /// person named it (DNS rebinding).
    #[test]
    fn an_allowed_name_that_resolves_inward_is_not_dialed() {
        let server = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = server.local_addr().unwrap().port();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let (conn, _) = listener.accept().unwrap();
            serve_with(conn, &|_host, _url| Ok(Pass::default()));
        });
        let mut client = TcpStream::connect(addr).unwrap();
        client
            .write_all(format!("CONNECT localhost:{port} HTTP/1.1\r\n\r\n").as_bytes())
            .unwrap();
        let mut reply = String::new();
        client.read_to_string(&mut reply).unwrap();
        assert!(reply.starts_with("HTTP/1.1 502"), "{reply}");
        server.set_nonblocking(true).unwrap();
        assert!(server.accept().is_err(), "nothing reached the server");
    }

    /// A tunnel to a loopback server carries bytes both ways.
    #[test]
    fn a_connect_tunnel_carries_bytes_both_ways() {
        let dir = scratch("tunnel");
        let server = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = server.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let (mut conn, _) = server.accept().unwrap();
            let mut buf = [0u8; 4];
            conn.read_exact(&mut buf).unwrap();
            conn.write_all(&buf.map(|b| b.to_ascii_uppercase()))
                .unwrap();
        });
        let before = let_out_count();
        let mut client = TcpStream::connect(address(&dir).unwrap()).unwrap();
        client
            .write_all(format!("CONNECT 127.0.0.1:{port} HTTP/1.1\r\n\r\nping").as_bytes())
            .unwrap();
        let mut reply = String::new();
        client.read_to_string(&mut reply).unwrap();
        assert_eq!(reply, "HTTP/1.1 200 Connection established\r\n\r\nPING");
        assert!(let_out_count() > before, "a let-out request is counted");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
